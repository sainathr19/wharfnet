//! A minimal JSON-RPC client for a running bitcoind/litecoind regtest chain.
//!
//! Bitcoin Core's RPC requires HTTP Basic auth and serves wallet methods under a
//! `/wallet/<name>` path (node methods live at `/`). A dependency-free raw socket
//! keeps this in step with the orchestrator's readiness probe rather than pulling
//! in an HTTP client (mirroring [`crate::solana::rpc`]).
//!
//! Bitcoin returns its RPC errors as a JSON `error` object — with a non-200 status
//! for a 1.0 request, or 200 in 2.0 mode — so the body is parsed regardless of the
//! HTTP status and the `error` member is what decides success.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::runtime::manifest::ChainEntry;

/// Fixed dev RPC credentials, matched in the compose template's daemon flags.
/// A localnet regtest node — throwaway creds, safe to hardcode.
pub(crate) const RPC_USER: &str = "wharfnet";
pub(crate) const RPC_PASS: &str = "wharfnet";
/// `Authorization` header value: `Basic base64("wharfnet:wharfnet")`. Precomputed
/// so the health probe (a `&'static str` in the [`Engine`](crate::runtime::engine)
/// trait) and this client share one constant.
pub(crate) const RPC_AUTH_HEADER: &str = "Basic d2hhcmZuZXQ6d2hhcmZuZXQ=";
/// The wallet wharfnet creates and funds at boot; faucet/control target it.
pub(crate) const WALLET: &str = "wharfnet";

/// Call a JSON-RPC `method` with positional `params` on `chain`, at an optional
/// wallet endpoint (`Some("wharfnet")` → `/wallet/wharfnet`; `None` → node root).
/// Returns the parsed `result`, or bails on a transport failure or an RPC error.
pub(crate) fn call(
    chain: &ChainEntry,
    wallet: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value> {
    let path = match wallet {
        Some(name) => format!("/wallet/{name}"),
        None => "/".to_string(),
    };
    // bitcoin-cli sends JSON-RPC 1.0; keep parity. Errors are read from the body,
    // not the status, so the version choice doesn't affect error handling.
    let body = serde_json::json!({
        "jsonrpc": "1.0",
        "id": "wharfnet",
        "method": method,
        "params": params,
    })
    .to_string();
    let resp = post(chain, &path, &body)?;
    let parsed: Value = serde_json::from_str(&resp)
        .with_context(|| format!("{} returned invalid JSON: {resp}", chain.kind))?;
    if let Some(err) = parsed.get("error").filter(|e| !e.is_null()) {
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        bail!("{} rpc `{method}` failed: {message}", chain.kind);
    }
    Ok(parsed.get("result").cloned().unwrap_or(Value::Null))
}

/// POST a raw JSON-RPC `body` to `chain`'s RPC endpoint at `path`, with the fixed
/// Basic auth header, and return the response body text. The manifest rpc url may
/// embed `user:pass@`, so any userinfo is stripped before dialing.
fn post(chain: &ChainEntry, path: &str, body: &str) -> Result<String> {
    let rest = chain
        .rpc
        .strip_prefix("http://")
        .or_else(|| chain.rpc.strip_prefix("https://"))
        .unwrap_or(&chain.rpc);
    // Drop a leading `user:pass@` and any trailing path — we dial host:port and
    // send our own `path`.
    let authority = rest.split('/').next().unwrap_or(rest);
    let authority = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h,
            p.parse::<u16>()
                .with_context(|| format!("invalid port in rpc url '{}'", chain.rpc))?,
        ),
        None => (authority, 18443),
    };
    let mut stream = TcpStream::connect(format!("{host}:{port}")).with_context(|| {
        format!(
            "connecting to {} at {host}:{port} — is the localnet up?",
            chain.kind
        )
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nAuthorization: {RPC_AUTH_HEADER}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(request.as_bytes())?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    let (head, raw_body) = resp
        .split_once("\r\n\r\n")
        .context("node returned a malformed HTTP response")?;
    // A 401 means the creds didn't match; there's no JSON error body to surface.
    if head.starts_with("HTTP/1.1 401") {
        bail!("{} rpc rejected the credentials (401)", chain.kind);
    }
    let out = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(raw_body)?
    } else {
        raw_body.to_string()
    };
    Ok(out)
}

/// Decode an HTTP/1.1 chunked body: `<hex-size>\r\n<data>\r\n` segments ending in a
/// zero-size chunk. Chunk extensions (after a `;`) are ignored.
fn dechunk(body: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let (size_line, after) = rest
            .split_once("\r\n")
            .context("malformed chunked response from node")?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .with_context(|| format!("invalid chunk size '{size_hex}' from node"))?;
        if size == 0 {
            break;
        }
        let chunk = after
            .get(..size)
            .context("truncated chunk in node response")?;
        out.push_str(chunk);
        rest = after.get(size + 2..).unwrap_or("");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dechunk_reassembles_a_chunked_body() {
        let json = r#"{"result":"ok","error":null,"id":"wharfnet"}"#;
        let chunked = format!("{:x}\r\n{json}\r\n0\r\n\r\n", json.len());
        assert_eq!(dechunk(&chunked).unwrap(), json);
    }

    #[test]
    fn auth_header_is_basic_of_the_dev_creds() {
        use base64_check::encode;
        assert_eq!(
            RPC_AUTH_HEADER,
            format!("Basic {}", encode(format!("{RPC_USER}:{RPC_PASS}")))
        );
    }

    /// A tiny standard-base64 encoder, only to prove the hardcoded header matches
    /// the dev creds — not worth a dependency.
    mod base64_check {
        pub fn encode(s: impl AsRef<[u8]>) -> String {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let b = s.as_ref();
            let mut out = String::new();
            for chunk in b.chunks(3) {
                let n = (chunk[0] as u32) << 16
                    | (*chunk.get(1).unwrap_or(&0) as u32) << 8
                    | (*chunk.get(2).unwrap_or(&0) as u32);
                out.push(T[(n >> 18 & 63) as usize] as char);
                out.push(T[(n >> 12 & 63) as usize] as char);
                out.push(if chunk.len() > 1 {
                    T[(n >> 6 & 63) as usize] as char
                } else {
                    '='
                });
                out.push(if chunk.len() > 2 {
                    T[(n & 63) as usize] as char
                } else {
                    '='
                });
            }
            out
        }
    }
}

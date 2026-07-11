//! A minimal JSON-RPC client for a running `starknet-devnet` chain's `/rpc`
//! endpoint. devnet's cheat methods (`devnet_mint`, `devnet_createBlock`,
//! `devnet_increaseTime`, …) live there alongside the standard Starknet methods,
//! so both the [faucet](super::faucet) and [control](super::control) drive them
//! through here. A dependency-free raw socket keeps this in step with the
//! orchestrator's readiness probe rather than pulling in an HTTP client.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use starknet_rust::providers::Url;

use crate::runtime::manifest::ChainEntry;

/// POST a raw JSON-RPC `body` to `chain`'s `/rpc` endpoint and return the
/// response body text. Host, port, and path are derived from the manifest's rpc
/// url. Bails on a transport failure, a non-200 response, or a JSON-RPC `error`.
pub(crate) fn post(chain: &ChainEntry, body: &str) -> Result<String> {
    let url = Url::parse(&chain.rpc).with_context(|| format!("invalid rpc url '{}'", chain.rpc))?;
    let host = url.host_str().unwrap_or("127.0.0.1");
    let port = url
        .port_or_known_default()
        .context("rpc url has no port to reach devnet on")?;
    let path = url.path();
    let mut stream = TcpStream::connect(format!("{host}:{port}"))
        .with_context(|| format!("connecting to devnet at {host}:{port} — is the localnet up?"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(request.as_bytes())?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    let (head, raw_body) = resp
        .split_once("\r\n\r\n")
        .context("devnet returned a malformed HTTP response")?;
    if !head.starts_with("HTTP/1.1 200") {
        bail!(
            "devnet rpc call failed: {}",
            head.lines().next().unwrap_or("").trim()
        );
    }
    // The pinned image replies with a Content-Length body, but decode a chunked
    // one too so a server that streams the response doesn't feed chunk framing
    // to the JSON parser.
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(raw_body)?
    } else {
        raw_body.to_string()
    };
    // JSON-RPC returns HTTP 200 even for method errors, so surface those too.
    // Check for a top-level `error` member rather than grepping for the substring,
    // which would false-positive on a result that legitimately contains "error"
    // (e.g. a contract ABI with an `error` entry).
    let parsed: Value = serde_json::from_str(&body)
        .with_context(|| format!("devnet returned invalid JSON: {body}"))?;
    if let Some(err) = parsed.get("error") {
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        bail!("devnet rpc returned an error: {message}");
    }
    Ok(body)
}

/// Decode an HTTP/1.1 chunked body: a run of `<hex-size>\r\n<data>\r\n` segments
/// terminated by a zero-size chunk. Chunk extensions (after a `;`) are ignored.
fn dechunk(body: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let (size_line, after) = rest
            .split_once("\r\n")
            .context("malformed chunked response from devnet")?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .with_context(|| format!("invalid chunk size '{size_hex}' from devnet"))?;
        if size == 0 {
            break;
        }
        let chunk = after
            .get(..size)
            .context("truncated chunk in devnet response")?;
        out.push_str(chunk);
        rest = after.get(size + 2..).unwrap_or("");
    }
    Ok(out)
}

/// Call a JSON-RPC `method` with `params` on `chain` and return the parsed
/// `result` value. A convenience over [`post`] for callers that need the result
/// (e.g. reading a block number); errors are already surfaced by `post`.
pub(crate) fn call(chain: &ChainEntry, method: &str, params: Value) -> Result<Value> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    })
    .to_string();
    let resp = post(chain, &body)?;
    let parsed: Value = serde_json::from_str(&resp)
        .with_context(|| format!("parsing {method} response: {resp}"))?;
    Ok(parsed.get("result").cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dechunk_reassembles_a_chunked_body() {
        assert_eq!(
            dechunk("4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap(),
            "Wikipedia"
        );
        // A single chunk carrying a JSON-RPC result.
        let json = r#"{"jsonrpc":"2.0","id":1,"result":42}"#;
        let chunked = format!("{:x}\r\n{json}\r\n0\r\n\r\n", json.len());
        assert_eq!(dechunk(&chunked).unwrap(), json);
    }
}

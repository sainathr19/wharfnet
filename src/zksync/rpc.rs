//! A minimal JSON-RPC client for a running anvil-zksync chain's RPC endpoint.
//!
//! anvil-zksync serves an Anvil-compatible JSON-RPC — the `eth_*` reads plus the
//! `evm_*`/`anvil_*` cheat methods — at the root path, and its container image
//! ships no `cast`, so [control](super::control) and the [faucet](super::faucet)
//! drive it through here rather than exec-ing a CLI inside the container (the EVM
//! approach). A dependency-free raw socket keeps this in step with the
//! orchestrator's readiness probe, mirroring the Solana [`rpc`] client.
//!
//! [`rpc`]: crate::solana::rpc

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::runtime::manifest::ChainEntry;

/// POST a raw JSON-RPC `body` to `chain`'s RPC endpoint and return the response
/// body text. Host, port, and path are derived from the manifest's rpc url. Bails
/// on a transport failure, a non-200 response, or a JSON-RPC `error`.
pub(crate) fn post(chain: &ChainEntry, body: &str) -> Result<String> {
    // The rpc url is a plain `http://host:port` (root path); parse it by hand to
    // avoid pulling in a URL crate for something this simple.
    let rest = chain
        .rpc
        .strip_prefix("http://")
        .or_else(|| chain.rpc.strip_prefix("https://"))
        .unwrap_or(&chain.rpc);
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h,
            p.parse::<u16>()
                .with_context(|| format!("invalid port in rpc url '{}'", chain.rpc))?,
        ),
        None => (authority, 8011),
    };
    let mut stream = TcpStream::connect(format!("{host}:{port}")).with_context(|| {
        format!("connecting to anvil-zksync at {host}:{port} — is the localnet up?")
    })?;
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
        .context("anvil-zksync returned a malformed HTTP response")?;
    if !head.starts_with("HTTP/1.1 200") {
        bail!(
            "anvil-zksync rpc call failed: {}",
            head.lines().next().unwrap_or("").trim()
        );
    }
    // Decode a chunked body if the server streams the response, so chunk framing
    // never reaches the JSON parser.
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(raw_body)?
    } else {
        raw_body.to_string()
    };
    // JSON-RPC returns HTTP 200 even for method errors, so surface those. Check
    // for a top-level `error` member rather than grepping the substring, which
    // would false-positive on a result that legitimately contains "error".
    let parsed: Value = serde_json::from_str(&body)
        .with_context(|| format!("anvil-zksync returned invalid JSON: {body}"))?;
    if let Some(err) = parsed.get("error") {
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        let detail = err.get("data").and_then(Value::as_str);
        match detail {
            Some(d) => bail!("anvil-zksync rpc returned an error: {message}: {d}"),
            None => bail!("anvil-zksync rpc returned an error: {message}"),
        }
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
            .context("malformed chunked response from anvil-zksync")?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .with_context(|| format!("invalid chunk size '{size_hex}' from anvil-zksync"))?;
        if size == 0 {
            break;
        }
        let chunk = after
            .get(..size)
            .context("truncated chunk in anvil-zksync response")?;
        out.push_str(chunk);
        rest = after.get(size + 2..).unwrap_or("");
    }
    Ok(out)
}

/// Call a JSON-RPC `method` with `params` on `chain` and return the parsed
/// `result` value. Params are a positional array (e.g. `json!(["0x1"])`). Errors
/// are surfaced by [`post`].
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
        let json = r#"{"jsonrpc":"2.0","id":1,"result":"0x104"}"#;
        let chunked = format!("{:x}\r\n{json}\r\n0\r\n\r\n", json.len());
        assert_eq!(dechunk(&chunked).unwrap(), json);
    }
}

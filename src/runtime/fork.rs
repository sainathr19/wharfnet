//! Shared fork helpers.
//!
//! Every chain kind forks the same way — mirror a live network from an RPC URL,
//! optionally pinned to a block. The redaction (which keeps an embedded API key
//! out of logs and the manifest) and the human-readable description are identical
//! across kinds, so they live here rather than being duplicated per engine. Only
//! the actual CLI flags differ (`--fork-url` for Anvil, `--fork-network` for
//! starknet-devnet), and those stay in each engine.

/// Keep only `scheme://host` from a URL, dropping the path and query so an
/// embedded API key is never recorded or printed.
pub fn redact_url(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let host = rest.split(['/', '?']).next().unwrap_or(rest);
            format!("{scheme}://{host}")
        }
        None => url.split(['/', '?']).next().unwrap_or(url).to_string(),
    }
}

/// A redacted, human-readable description of a fork source — `scheme://host @
/// block N` (or `@ latest` when unpinned). Safe to record in the manifest and
/// print, since [`redact_url`] has dropped any credential.
pub fn describe(url: &str, block: Option<u64>) -> String {
    let source = redact_url(url);
    match block {
        Some(block) => format!("{source} @ block {block}"),
        None => format!("{source} @ latest"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_url_keeps_only_scheme_and_host() {
        assert_eq!(
            redact_url("https://eth-mainnet.g.alchemy.com/v2/KEY123"),
            "https://eth-mainnet.g.alchemy.com"
        );
        assert_eq!(redact_url("http://localhost:8545"), "http://localhost:8545");
        assert_eq!(redact_url("weird?q=1"), "weird");
    }

    #[test]
    fn describe_redacts_and_notes_the_block() {
        assert_eq!(
            describe("https://eth.example.com/v2/SECRET", Some(123)),
            "https://eth.example.com @ block 123"
        );
        assert_eq!(
            describe("https://sepolia.example.com/rpc/KEY", None),
            "https://sepolia.example.com @ latest"
        );
    }
}

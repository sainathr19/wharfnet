//! The endpoints manifest — the single machine-readable description of a
//! running localnet. Tests and tooling read this to discover RPC URLs, chain
//! IDs, and funded accounts. This is the generalized successor to a
//! hand-maintained `deployments.json`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Debug)]
pub struct Manifest {
    pub version: String,
    pub project: String,
    pub chains: Vec<ChainEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChainEntry {
    pub name: String,
    pub kind: String,
    pub rpc: String,
    /// Chain identifier as a string: a decimal number for EVM chains (e.g.
    /// "31337"), or a felt for Starknet (e.g. "0x534e5f5345504f4c4941"). It's a
    /// string because Starknet chain IDs are felts that overflow `u64`.
    pub chain_id: String,
    pub accounts: Vec<Account>,
    /// Test tokens pre-deployed on this chain at known addresses.
    #[serde(default)]
    pub tokens: Vec<Token>,
    /// Canonical infra contracts pre-deployed at their chain-agnostic addresses
    /// (Multicall3, Permit2, the CREATE2 deployer).
    #[serde(default)]
    pub contracts: Vec<Contract>,
    /// When this chain forks a live network, a redacted description of the
    /// source (host + pinned block); the RPC key is never recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork: Option<String>,
    /// URL of a bundled block explorer for this chain, when one was booted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explorer: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Account {
    pub address: String,
    pub private_key: String,
    pub balance: String,
}

/// A test token pre-deployed on a chain. `mint` is public so the faucet can
/// top up any address.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Token {
    pub symbol: String,
    pub name: String,
    pub address: String,
    pub decimals: u8,
}

/// A canonical infra contract pre-deployed at its real, chain-agnostic address
/// (e.g. Multicall3, Permit2) so tooling that hardcodes the address just works.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Contract {
    pub name: String,
    pub address: String,
}

impl Manifest {
    pub fn new(chains: Vec<ChainEntry>) -> Self {
        Manifest {
            version: "0.1".to_string(),
            project: "wharfnet".to_string(),
            chains,
        }
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)
            .with_context(|| format!("writing manifest to {}", path.display()))?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Manifest> {
        let data = std::fs::read_to_string(path)
            .with_context(|| format!("reading manifest from {}", path.display()))?;
        let manifest = serde_json::from_str(&data).context("parsing manifest")?;
        Ok(manifest)
    }

    /// Chains matching `selector` — either a chain kind (`evm`, `starknet`),
    /// selecting every chain of that kind, or a specific chain name (`anvil-1`).
    /// Errors, listing what is available, when nothing matches. Shared by the
    /// faucet and the per-kind chain-control commands.
    pub fn select(&self, selector: &str) -> Result<Vec<&ChainEntry>> {
        let matches: Vec<&ChainEntry> = self
            .chains
            .iter()
            .filter(|c| c.name == selector || c.kind == selector)
            .collect();
        if matches.is_empty() {
            let available = self
                .chains
                .iter()
                .map(|c| format!("{} ({})", c.name, c.kind))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("no chain matching '{selector}'. Available: {available}");
        }
        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample() -> Manifest {
        Manifest::new(vec![ChainEntry {
            name: "anvil-1".into(),
            kind: "evm".into(),
            rpc: "http://127.0.0.1:8545".into(),
            chain_id: "31337".into(),
            accounts: vec![Account {
                address: "0xabc".into(),
                private_key: "0xdef".into(),
                balance: "10000 ETH".into(),
            }],
            tokens: vec![Token {
                symbol: "USDC".into(),
                name: "USD Coin".into(),
                address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".into(),
                decimals: 6,
            }],
            contracts: vec![Contract {
                name: "Multicall3".into(),
                address: "0xcA11bde05977b3631167028862bE2a173976CA11".into(),
            }],
            fork: None,
            explorer: Some("http://127.0.0.1:5100".into()),
        }])
    }

    #[test]
    fn new_sets_version_and_project() {
        let m = sample();
        assert_eq!(m.version, "0.1");
        assert_eq!(m.project, "wharfnet");
        assert_eq!(m.chains.len(), 1);
    }

    #[test]
    fn write_then_read_roundtrips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wharfnet.json");
        sample().write(&path).unwrap();
        assert!(path.exists());

        let loaded = Manifest::read(&path).unwrap();
        assert_eq!(loaded.chains[0].name, "anvil-1");
        assert_eq!(loaded.chains[0].chain_id, "31337");
        assert_eq!(loaded.chains[0].accounts[0].address, "0xabc");
        assert_eq!(loaded.chains[0].tokens[0].symbol, "USDC");
        assert_eq!(loaded.chains[0].tokens[0].decimals, 6);
        assert_eq!(loaded.chains[0].contracts[0].name, "Multicall3");
        assert_eq!(
            loaded.chains[0].contracts[0].address,
            "0xcA11bde05977b3631167028862bE2a173976CA11"
        );
        assert_eq!(
            loaded.chains[0].explorer.as_deref(),
            Some("http://127.0.0.1:5100")
        );
    }

    #[test]
    fn explorer_is_omitted_when_absent() {
        let mut m = sample();
        m.chains[0].explorer = None;
        let json = serde_json::to_string(&m).unwrap();
        assert!(
            !json.contains("explorer"),
            "None explorer must not serialize"
        );

        // And a manifest without the field still parses (defaults to None).
        let loaded: Manifest = serde_json::from_str(&json).unwrap();
        assert!(loaded.chains[0].explorer.is_none());
    }

    #[test]
    fn tokens_default_to_empty_when_absent() {
        // Older manifests written before tokens existed must still parse.
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy.json");
        std::fs::write(
            &path,
            r#"{"version":"0.1","project":"wharfnet","chains":[
                {"name":"anvil-1","kind":"evm","rpc":"http://127.0.0.1:8545",
                 "chain_id":"31337","accounts":[]}]}"#,
        )
        .unwrap();
        let loaded = Manifest::read(&path).unwrap();
        assert!(loaded.chains[0].tokens.is_empty());
    }

    #[test]
    fn select_matches_by_kind_and_name_and_errors_otherwise() {
        let m = sample(); // one evm chain named "anvil-1"
        assert_eq!(m.select("evm").unwrap().len(), 1);
        assert_eq!(m.select("anvil-1").unwrap()[0].name, "anvil-1");
        let err = m.select("nope").unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
        assert!(err.to_string().contains("anvil-1 (evm)"), "{err}");
    }

    #[test]
    fn read_missing_file_errors() {
        let dir = tempdir().unwrap();
        assert!(Manifest::read(&dir.path().join("absent.json")).is_err());
    }

    #[test]
    fn write_to_nonexistent_dir_errors() {
        let bad = Path::new("/this/path/does/not/exist/wharfnet.json");
        assert!(sample().write(bad).is_err());
    }

    #[test]
    fn read_invalid_json_errors() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(Manifest::read(&path).is_err());
    }

    #[test]
    fn serializes_to_pretty_json() {
        let json = serde_json::to_string_pretty(&sample()).unwrap();
        assert!(json.contains("\"project\": \"wharfnet\""));
        assert!(json.contains("\"chain_id\": \"31337\""));
    }
}

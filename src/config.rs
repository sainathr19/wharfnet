//! Optional project config: `wharfnet.toml`.
//!
//! When a `wharfnet.toml` is present in the working directory it defines the
//! chain topology — which chains to boot and their ports, chain IDs, and block
//! times. Without one, the built-in defaults are used (two Anvil EVM chains), so
//! wharfnet stays zero-config.
//!
//! Accounts and test tokens are not configurable here: they come from the baked
//! Anvil state snapshot (see `engine.rs`).

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

/// Config file looked up in the current working directory.
pub const CONFIG_FILE: &str = "wharfnet.toml";

fn default_kind() -> String {
    "evm".to_string()
}

fn default_block_time() -> u64 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub chains: Vec<ChainConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainConfig {
    /// Service / container name, e.g. "anvil-1".
    pub name: String,
    /// Chain kind. Only "evm" is supported today.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Published host port for the RPC.
    pub port: u16,
    pub chain_id: u64,
    /// Block time in seconds (auto-mining interval).
    #[serde(default = "default_block_time")]
    pub block_time: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            chains: vec![
                ChainConfig {
                    name: "anvil-1".to_string(),
                    kind: "evm".to_string(),
                    port: 8545,
                    chain_id: 31337,
                    block_time: 1,
                },
                ChainConfig {
                    name: "anvil-2".to_string(),
                    kind: "evm".to_string(),
                    port: 8546,
                    chain_id: 31338,
                    block_time: 1,
                },
            ],
        }
    }
}

/// Load the config from the working directory, or the built-in defaults if no
/// `wharfnet.toml` is present.
pub fn load() -> Result<Config> {
    load_from(Path::new(CONFIG_FILE))
}

/// Load and validate the config at `path`, or return defaults if it's missing.
pub fn load_from(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let config: Config = toml::from_str(&text)
        .with_context(|| format!("parsing {} — check the TOML syntax", path.display()))?;
    validate(&config)?;
    Ok(config)
}

/// Reject configs that would produce a broken or unsupported localnet.
fn validate(config: &Config) -> Result<()> {
    if config.chains.is_empty() {
        bail!(
            "{CONFIG_FILE} declares no [[chains]] — add at least one, or delete the file to use the defaults"
        );
    }
    let mut names = HashSet::new();
    let mut ports = HashSet::new();
    let mut ids = HashSet::new();
    for c in &config.chains {
        if c.kind != "evm" {
            bail!(
                "chain '{}': kind '{}' is not supported yet (only 'evm')",
                c.name,
                c.kind
            );
        }
        if !names.insert(c.name.as_str()) {
            bail!("duplicate chain name '{}'", c.name);
        }
        if !ports.insert(c.port) {
            bail!(
                "duplicate host port {} — chains must publish on distinct ports",
                c.port
            );
        }
        if !ids.insert(c.chain_id) {
            bail!("duplicate chain_id {}", c.chain_id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
        let path = dir.path().join(CONFIG_FILE);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn default_is_two_anvil_chains() {
        let c = Config::default();
        assert_eq!(c.chains.len(), 2);
        assert_eq!(c.chains[0].name, "anvil-1");
        assert_eq!(c.chains[0].port, 8545);
        assert_eq!(c.chains[1].chain_id, 31338);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempdir().unwrap();
        let c = load_from(&dir.path().join(CONFIG_FILE)).unwrap();
        assert_eq!(c.chains.len(), 2);
    }

    #[test]
    fn parses_a_custom_topology_with_defaults_applied() {
        let dir = tempdir().unwrap();
        let path = write(
            &dir,
            r#"
            [[chains]]
            name = "main"
            port = 9000
            chain_id = 1337
            "#,
        );
        let c = load_from(&path).unwrap();
        assert_eq!(c.chains.len(), 1);
        assert_eq!(c.chains[0].name, "main");
        assert_eq!(c.chains[0].port, 9000);
        // kind + block_time defaulted.
        assert_eq!(c.chains[0].kind, "evm");
        assert_eq!(c.chains[0].block_time, 1);
    }

    #[test]
    fn rejects_duplicate_ports() {
        let dir = tempdir().unwrap();
        let path = write(
            &dir,
            r#"
            [[chains]]
            name = "a"
            port = 8545
            chain_id = 1
            [[chains]]
            name = "b"
            port = 8545
            chain_id = 2
            "#,
        );
        let err = load_from(&path).unwrap_err();
        assert!(err.to_string().contains("duplicate host port"), "{err}");
    }

    #[test]
    fn rejects_non_evm_kind() {
        let dir = tempdir().unwrap();
        let path = write(
            &dir,
            r#"
            [[chains]]
            name = "sol"
            kind = "solana"
            port = 8899
            chain_id = 1
            "#,
        );
        let err = load_from(&path).unwrap_err();
        assert!(err.to_string().contains("not supported yet"), "{err}");
    }

    #[test]
    fn rejects_empty_chains() {
        let dir = tempdir().unwrap();
        let path = write(&dir, "chains = []\n");
        let err = load_from(&path).unwrap_err();
        assert!(err.to_string().contains("no [[chains]]"), "{err}");
    }

    #[test]
    fn rejects_unknown_fields() {
        let dir = tempdir().unwrap();
        let path = write(
            &dir,
            r#"
            [[chains]]
            name = "a"
            port = 8545
            chain_id = 1
            bogus = true
            "#,
        );
        assert!(load_from(&path).is_err());
    }
}

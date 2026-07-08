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

/// Config file looked up in the current working directory by default.
pub const CONFIG_FILE: &str = "wharfnet.toml";
/// Environment variable that overrides the config path (below an explicit flag).
pub const CONFIG_ENV: &str = "WHARFNET_CONFIG";

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
    /// Fork this chain from a live RPC (Anvil `--fork-url`). `${VAR}` references
    /// are expanded from the environment on load, so an RPC key can stay out of
    /// the file. A forked chain mirrors real chain state instead of the baked
    /// test tokens.
    #[serde(default)]
    pub fork_url: Option<String>,
    /// Pin the fork to a block height (Anvil `--fork-block-number`). Requires
    /// `fork_url`.
    #[serde(default)]
    pub fork_block: Option<u64>,
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
                    fork_url: None,
                    fork_block: None,
                },
                ChainConfig {
                    name: "anvil-2".to_string(),
                    kind: "evm".to_string(),
                    port: 8546,
                    chain_id: 31338,
                    block_time: 1,
                    fork_url: None,
                    fork_block: None,
                },
            ],
        }
    }
}

/// Resolve and load the config. Precedence: an explicit `--config` path, then
/// the `WHARFNET_CONFIG` env var, then `./wharfnet.toml`.
///
/// A path requested explicitly (flag or env) **must** exist — a missing one is a
/// loud error. The default `./wharfnet.toml` is optional: absent, wharfnet falls
/// back to the built-in defaults so it stays zero-config.
pub fn load(explicit: Option<&Path>) -> Result<Config> {
    if let Some(path) = explicit {
        return load_required(path, "--config");
    }
    if let Some(env) = std::env::var_os(CONFIG_ENV) {
        return load_required(Path::new(&env), CONFIG_ENV);
    }
    load_from(Path::new(CONFIG_FILE))
}

/// Load a config path the user asked for explicitly; error if it's missing.
fn load_required(path: &Path, source: &str) -> Result<Config> {
    if !path.exists() {
        bail!("config file not found: {} (from {source})", path.display());
    }
    load_from(path)
}

/// Load and validate the config at `path`, or return defaults if it's missing.
pub fn load_from(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut config: Config = toml::from_str(&text)
        .with_context(|| format!("parsing {} — check the TOML syntax", path.display()))?;
    resolve_env(&mut config)?;
    validate(&config)?;
    Ok(config)
}

/// Expand `${VAR}` references in fork URLs from the environment, so an RPC key
/// never has to live in the file.
fn resolve_env(config: &mut Config) -> Result<()> {
    for c in &mut config.chains {
        if let Some(url) = &c.fork_url {
            c.fork_url = Some(
                expand_env(url)
                    .with_context(|| format!("chain '{}': resolving fork_url", c.name))?,
            );
        }
    }
    Ok(())
}

/// Substitute every `${VAR}` in `input` with the environment value, erroring on
/// an unterminated `${` or an unset variable.
fn expand_env(input: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("unterminated '${{' in '{input}'"))?;
        let var = &after[..end];
        let val = std::env::var(var)
            .map_err(|_| anyhow::anyhow!("environment variable '{var}' is not set"))?;
        out.push_str(&val);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
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
        if c.fork_block.is_some() && c.fork_url.is_none() {
            bail!("chain '{}': fork_block needs a fork_url to pin", c.name);
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
    fn explicit_path_must_exist() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope.toml");
        let err = load(Some(&missing)).unwrap_err();
        assert!(err.to_string().contains("config file not found"), "{err}");
    }

    #[test]
    fn explicit_path_is_loaded_when_present() {
        let dir = tempdir().unwrap();
        let path = write(
            &dir,
            r#"
            [[chains]]
            name = "solo"
            port = 7000
            chain_id = 99
            "#,
        );
        let c = load(Some(&path)).unwrap();
        assert_eq!(c.chains.len(), 1);
        assert_eq!(c.chains[0].name, "solo");
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
    fn parses_fork_fields() {
        let dir = tempdir().unwrap();
        let path = write(
            &dir,
            r#"
            [[chains]]
            name = "mainnet-fork"
            port = 8545
            chain_id = 1
            fork_url = "https://rpc.example/key"
            fork_block = 21000000
            "#,
        );
        let c = load_from(&path).unwrap();
        assert_eq!(
            c.chains[0].fork_url.as_deref(),
            Some("https://rpc.example/key")
        );
        assert_eq!(c.chains[0].fork_block, Some(21000000));
    }

    #[test]
    fn fork_url_without_fork_defaults_to_none() {
        let c = Config::default();
        assert!(c.chains[0].fork_url.is_none());
        assert!(c.chains[0].fork_block.is_none());
    }

    #[test]
    fn fork_block_without_url_is_rejected() {
        let dir = tempdir().unwrap();
        let path = write(
            &dir,
            r#"
            [[chains]]
            name = "a"
            port = 8545
            chain_id = 1
            fork_block = 123
            "#,
        );
        let err = load_from(&path).unwrap_err();
        assert!(
            err.to_string().contains("fork_block needs a fork_url"),
            "{err}"
        );
    }

    #[test]
    fn fork_url_expands_env_vars() {
        // Use PATH, which is always set, so the test needs no env mutation.
        let path_val = std::env::var("PATH").unwrap();
        let expanded = expand_env("https://rpc/${PATH}/x").unwrap();
        assert_eq!(expanded, format!("https://rpc/{path_val}/x"));
        // No placeholder is a passthrough.
        assert_eq!(
            expand_env("https://rpc/plain").unwrap(),
            "https://rpc/plain"
        );
    }

    #[test]
    fn fork_url_errors_on_unset_env_var() {
        let err = expand_env("${WHARFNET_DEFINITELY_UNSET_VAR_XYZ}").unwrap_err();
        assert!(err.to_string().contains("is not set"), "{err}");
        let err = expand_env("${UNTERMINATED").unwrap_err();
        assert!(err.to_string().contains("unterminated"), "{err}");
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

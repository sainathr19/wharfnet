//! Shared helpers for driving running EVM chains: resolve target chains from the
//! manifest and run `cast` inside their containers via `docker compose exec`.
//!
//! `cast` is already on the container's PATH and the node's RPC is reachable at a
//! stable internal address (`INTERNAL_RPC`) regardless of the published host port.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::runtime::manifest::{ChainEntry, Manifest};
use crate::runtime::orchestrator::{compose_path, manifest_path};

/// Anvil listens here inside its container irrespective of the published host
/// port. Kept in step with the engine's listen port by the assertion below, so
/// changing one without the other is a compile error rather than a silent break.
pub const INTERNAL_RPC: &str = "http://127.0.0.1:8545";
const _: () = assert!(crate::evm::engine::ANVIL_INTERNAL_PORT == 8545);

/// A handle to a running localnet: the parsed manifest plus how to drive its
/// containers.
#[derive(Debug)]
pub struct Session {
    manifest: Manifest,
    compose: PathBuf,
    project: String,
}

impl Session {
    /// Open the localnet described by `base`. Errors if it isn't running.
    pub fn open(base: &Path, project: &str) -> Result<Session> {
        let manifest_file = manifest_path(base);
        if !manifest_file.exists() {
            bail!("localnet is not running. Start it with `wharfnet up`.");
        }
        Ok(Session {
            manifest: Manifest::read(&manifest_file)?,
            compose: compose_path(base),
            project: project.to_string(),
        })
    }

    /// Chains matching `selector` — a kind (`evm`) or a specific name (`anvil-1`).
    pub fn targets(&self, selector: &str) -> Result<Vec<&ChainEntry>> {
        self.manifest.select(selector)
    }

    /// Run `cast <args>` inside the chain's container and return its stdout.
    pub fn cast(&self, chain: &ChainEntry, args: &[&str]) -> Result<String> {
        let output = Command::new("docker")
            .arg("compose")
            .arg("-f")
            .arg(&self.compose)
            .arg("-p")
            .arg(&self.project)
            .arg("exec")
            .arg("-T")
            .arg(&chain.name)
            .arg("cast")
            .args(args)
            .output()
            .context("running `docker compose exec … cast` — is the localnet up?")?;
        if !output.status.success() {
            bail!(
                "cast failed on {}:\n{}",
                chain.name,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// `cast rpc <method> [params…]` against the chain's internal RPC.
    pub fn cast_rpc(&self, chain: &ChainEntry, method: &str, params: &[&str]) -> Result<String> {
        let mut args = vec!["rpc", method];
        args.extend_from_slice(params);
        args.extend_from_slice(&["--rpc-url", INTERNAL_RPC]);
        self.cast(chain, &args)
    }

    /// The chain's current block number (decimal).
    pub fn block_number(&self, chain: &ChainEntry) -> Result<String> {
        Ok(self
            .cast(chain, &["block-number", "--rpc-url", INTERNAL_RPC])?
            .trim()
            .to_string())
    }
}

/// Ensure `chain` is an EVM chain — the only kind `cast`/`anvil` helpers support.
pub fn ensure_evm(chain: &ChainEntry) -> Result<()> {
    if chain.kind != "evm" {
        bail!(
            "'{}' is a {} chain; this command is only supported on EVM chains",
            chain.name,
            chain.kind
        );
    }
    Ok(())
}

/// Validate a 0x-prefixed 20-byte hex address.
pub fn validate_address(address: &str) -> Result<()> {
    let ok = address.len() == 42
        && address.starts_with("0x")
        && address[2..].chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        bail!("'{address}' is not a valid EVM address (expected 0x + 40 hex chars)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::{Account, ChainEntry, Token};
    use tempfile::tempdir;

    fn evm_chain() -> ChainEntry {
        ChainEntry {
            name: "anvil-1".into(),
            kind: "evm".into(),
            rpc: "http://127.0.0.1:8545".into(),
            ws: None,
            chain_id: "31337".into(),
            accounts: vec![Account {
                address: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into(),
                private_key: "0xac09".into(),
                balance: "10000 ETH".into(),
            }],
            tokens: vec![Token {
                symbol: "USDC".into(),
                name: "USD Coin".into(),
                address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".into(),
                decimals: 6,
            }],
            contracts: vec![],
            fork: None,
            explorer: None,
        }
    }

    const VALID_ADDR: &str = "0x000000000000000000000000000000000000dEaD";

    #[test]
    fn open_errors_when_not_running() {
        let dir = tempdir().unwrap();
        let err = Session::open(dir.path(), "p").unwrap_err();
        assert!(err.to_string().contains("not running"), "{err}");
    }

    #[test]
    fn targets_match_by_kind_and_name_and_error_otherwise() {
        let dir = tempdir().unwrap();
        Manifest::new(vec![evm_chain()])
            .write(&manifest_path(dir.path()))
            .unwrap();
        let session = Session::open(dir.path(), "p").unwrap();

        assert_eq!(session.targets("evm").unwrap().len(), 1);
        assert_eq!(session.targets("anvil-1").unwrap().len(), 1);
        let err = session.targets("nope").unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn ensure_evm_rejects_non_evm() {
        let mut solana = evm_chain();
        solana.kind = "solana".into();
        assert!(ensure_evm(&evm_chain()).is_ok());
        assert!(ensure_evm(&solana).is_err());
    }

    #[test]
    fn validate_address_accepts_and_rejects() {
        assert!(validate_address(VALID_ADDR).is_ok());
        assert!(validate_address("0x123").is_err());
        assert!(validate_address("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266").is_err());
        assert!(validate_address(&format!("0x{}", "z".repeat(40))).is_err());
    }
}

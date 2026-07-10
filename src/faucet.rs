//! The faucet coordinator: resolve target chains from the manifest and dispatch
//! each to its chain kind's funder.
//!
//! `wharfnet faucet <chain> <address> <amount> [--token SYMBOL]` funds an address
//! on a running localnet. `<chain>` matches either a chain kind (`evm`,
//! `starknet`) — funding every matching chain — or a specific chain name
//! (`anvil-1`). Each kind funds natively: EVM tops up ETH and mints its ERC-20s
//! (see [`crate::evm::faucet`]); Starknet mints ETH/STRK through devnet's cheat
//! and its Cairo test tokens through signed invokes (see
//! [`crate::starknet::faucet`]).

use anyhow::{Result, bail};
use std::path::Path;

use crate::runtime::manifest::{ChainEntry, Manifest};
use crate::runtime::orchestrator::{DEFAULT_PROJECT, DEFAULT_STATE_DIR, manifest_path};

pub fn run(chain: &str, address: &str, amount: u64, token: Option<&str>) -> Result<()> {
    run_in(
        Path::new(DEFAULT_STATE_DIR),
        DEFAULT_PROJECT,
        chain,
        address,
        amount,
        token,
    )
}

/// Testable core of [`run`]: read the manifest under `base`, resolve the chains
/// matching `selector`, and hand each to its kind's funder.
pub(crate) fn run_in(
    base: &Path,
    project: &str,
    selector: &str,
    address: &str,
    amount: u64,
    token: Option<&str>,
) -> Result<()> {
    let manifest_file = manifest_path(base);
    if !manifest_file.exists() {
        bail!("localnet is not running. Start it with `wharfnet up`.");
    }
    let manifest = Manifest::read(&manifest_file)?;
    for chain in targets(&manifest, selector)? {
        match chain.kind.as_str() {
            "evm" => crate::evm::faucet::fund_chain(base, project, chain, address, amount, token)?,
            "starknet" => crate::starknet::faucet::fund_chain(chain, address, amount, token)?,
            other => bail!(
                "faucet is not yet supported for {other} chains (chain '{}')",
                chain.name
            ),
        }
    }
    Ok(())
}

/// Chains matching `selector` — a kind (`evm`, `starknet`) or a specific name
/// (`anvil-1`). Errors, listing what is available, if nothing matches.
fn targets<'a>(manifest: &'a Manifest, selector: &str) -> Result<Vec<&'a ChainEntry>> {
    let targets: Vec<&ChainEntry> = manifest
        .chains
        .iter()
        .filter(|c| c.name == selector || c.kind == selector)
        .collect();
    if targets.is_empty() {
        let available = manifest
            .chains
            .iter()
            .map(|c| format!("{} ({})", c.name, c.kind))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("no chain matching '{selector}'. Available: {available}");
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::{Account, Token};
    use crate::runtime::orchestrator::manifest_path;
    use tempfile::tempdir;

    fn evm_chain() -> ChainEntry {
        ChainEntry {
            name: "anvil-1".into(),
            kind: "evm".into(),
            rpc: "http://127.0.0.1:8545".into(),
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

    fn write_manifest(base: &Path, chains: Vec<ChainEntry>) {
        Manifest::new(chains).write(&manifest_path(base)).unwrap();
    }

    const VALID_ADDR: &str = "0x000000000000000000000000000000000000dEaD";

    #[test]
    fn targets_match_by_kind_and_name_and_error_otherwise() {
        let manifest = Manifest::new(vec![evm_chain()]);
        assert_eq!(targets(&manifest, "evm").unwrap().len(), 1);
        assert_eq!(targets(&manifest, "anvil-1").unwrap().len(), 1);
        let err = targets(&manifest, "nope").unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    // ---- dispatch error paths, exercised through the public entry ----

    #[test]
    fn errors_when_localnet_not_running() {
        let dir = tempdir().unwrap();
        let err = run_in(dir.path(), "p", "evm", VALID_ADDR, 100, None).unwrap_err();
        assert!(err.to_string().contains("not running"), "{err}");
    }

    #[test]
    fn errors_when_no_chain_matches_selector() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(dir.path(), "p", "solana", VALID_ADDR, 100, None).unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn errors_on_unsupported_chain_kind() {
        let dir = tempdir().unwrap();
        let mut solana = evm_chain();
        solana.name = "solana-1".into();
        solana.kind = "solana".into();
        solana.tokens.clear();
        write_manifest(dir.path(), vec![solana]);
        let err = run_in(dir.path(), "p", "solana", VALID_ADDR, 100, None).unwrap_err();
        assert!(err.to_string().contains("not yet supported"), "{err}");
    }

    #[test]
    fn errors_on_invalid_evm_address() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(dir.path(), "p", "evm", "0xnothex", 100, None).unwrap_err();
        assert!(err.to_string().contains("valid EVM address"), "{err}");
    }

    #[test]
    fn errors_on_unknown_token() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(dir.path(), "p", "evm", VALID_ADDR, 100, Some("DAI")).unwrap_err();
        assert!(err.to_string().contains("not deployed"), "{err}");
    }
}

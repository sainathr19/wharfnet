//! The faucet coordinator: resolve target chains from the manifest and dispatch
//! each to its chain kind's funder.
//!
//! `wharfnet faucet <chain> <address> <amount> [--token SYMBOL]` funds an address
//! on a running localnet. `<chain>` matches either a chain kind (`evm`,
//! `starknet`, `solana`) — funding every matching chain — or a specific chain
//! name (`anvil-1`). Each kind funds natively: EVM tops up ETH and mints its
//! ERC-20s (see [`crate::evm::faucet`]); Starknet mints ETH/STRK through devnet's
//! cheat and its Cairo test tokens through signed invokes (see
//! [`crate::starknet::faucet`]); Solana airdrops SOL and tops up its SPL tokens
//! through cheatcodes (see [`crate::solana::faucet`]).

use anyhow::{Result, bail};
use std::path::Path;

use crate::runtime::manifest::Manifest;
use crate::runtime::orchestrator::{DEFAULT_PROJECT, DEFAULT_STATE_DIR, manifest_path};

pub fn run(chain: &str, address: &str, amount: &str, token: Option<&str>, raw: bool) -> Result<()> {
    run_in(
        Path::new(DEFAULT_STATE_DIR),
        DEFAULT_PROJECT,
        chain,
        address,
        amount,
        token,
        raw,
    )
}

/// Testable core of [`run`]: read the manifest under `base`, resolve the chains
/// matching `selector`, and hand each to its kind's funder.
pub(crate) fn run_in(
    base: &Path,
    project: &str,
    selector: &str,
    address: &str,
    amount: &str,
    token: Option<&str>,
    raw: bool,
) -> Result<()> {
    let manifest_file = manifest_path(base);
    if !manifest_file.exists() {
        bail!("localnet is not running. Start it with `wharfnet up`.");
    }
    let manifest = Manifest::read(&manifest_file)?;
    for chain in manifest.select(selector)? {
        match chain.kind.as_str() {
            "evm" => {
                crate::evm::faucet::fund_chain(base, project, chain, address, amount, token, raw)?
            }
            "starknet" => crate::starknet::faucet::fund_chain(chain, address, amount, token, raw)?,
            "solana" => crate::solana::faucet::fund_chain(chain, address, amount, token, raw)?,
            other => bail!(
                "faucet is not yet supported for {other} chains (chain '{}')",
                chain.name
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::{Account, ChainEntry, Token};
    use crate::runtime::orchestrator::manifest_path;
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

    fn write_manifest(base: &Path, chains: Vec<ChainEntry>) {
        Manifest::new(chains).write(&manifest_path(base)).unwrap();
    }

    const VALID_ADDR: &str = "0x000000000000000000000000000000000000dEaD";

    #[test]
    fn selector_matches_by_kind_and_name_and_errors_otherwise() {
        let manifest = Manifest::new(vec![evm_chain()]);
        assert_eq!(manifest.select("evm").unwrap().len(), 1);
        assert_eq!(manifest.select("anvil-1").unwrap().len(), 1);
        let err = manifest.select("nope").unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    // ---- dispatch error paths, exercised through the public entry ----

    #[test]
    fn errors_when_localnet_not_running() {
        let dir = tempdir().unwrap();
        let err = run_in(dir.path(), "p", "evm", VALID_ADDR, "100", None, false).unwrap_err();
        assert!(err.to_string().contains("not running"), "{err}");
    }

    #[test]
    fn errors_when_no_chain_matches_selector() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(dir.path(), "p", "solana", VALID_ADDR, "100", None, false).unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn errors_on_unsupported_chain_kind() {
        let dir = tempdir().unwrap();
        let mut aptos = evm_chain();
        aptos.name = "aptos-1".into();
        aptos.kind = "aptos".into();
        aptos.tokens.clear();
        write_manifest(dir.path(), vec![aptos]);
        let err = run_in(dir.path(), "p", "aptos", VALID_ADDR, "100", None, false).unwrap_err();
        assert!(err.to_string().contains("not yet supported"), "{err}");
    }

    #[test]
    fn errors_on_invalid_evm_address() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(dir.path(), "p", "evm", "0xnothex", "100", None, false).unwrap_err();
        assert!(err.to_string().contains("valid EVM address"), "{err}");
    }

    #[test]
    fn errors_on_unknown_token() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(
            dir.path(),
            "p",
            "evm",
            VALID_ADDR,
            "100",
            Some("DAI"),
            false,
        )
        .unwrap_err();
        assert!(err.to_string().contains("not deployed"), "{err}");
    }
}

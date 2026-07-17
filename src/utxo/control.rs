//! UTXO chain-control commands (exposed under `wharfnet bitcoin …` and
//! `wharfnet litecoin …`): drive a running regtest chain.
//!
//! Regtest produces blocks only on demand, so `mine` is the core control — it
//! calls `generatetoaddress` to add blocks to the boot wallet. There is no
//! time/warp or snapshot analogue here (a UTXO chain has no manipulable clock or
//! numbered snapshots), so this is deliberately just mining.

use anyhow::{Context, Result, bail};
use serde_json::json;
use std::path::Path;

use super::rpc::{self, WALLET};
use crate::runtime::manifest::{ChainEntry, Manifest};
use crate::runtime::orchestrator::{DEFAULT_STATE_DIR, manifest_path};

/// Read the localnet manifest under `base` and run `f` against every UTXO chain
/// matching `selector`. Errors if the localnet isn't running, nothing matches, or
/// a matched chain isn't a UTXO chain.
fn for_each_target<F>(base: &Path, selector: &str, mut f: F) -> Result<()>
where
    F: FnMut(&ChainEntry) -> Result<()>,
{
    let manifest_file = manifest_path(base);
    if !manifest_file.exists() {
        bail!("localnet is not running. Start it with `wharfnet up`.");
    }
    let manifest = Manifest::read(&manifest_file)?;
    for chain in manifest.select(selector)? {
        if chain.kind != "bitcoin" && chain.kind != "litecoin" {
            bail!(
                "chain control here is only supported on Bitcoin/Litecoin chains (chain '{}' is a {} chain)",
                chain.name,
                chain.kind
            );
        }
        f(chain)?;
    }
    Ok(())
}

pub fn mine(selector: &str, count: u64) -> Result<()> {
    mine_in(Path::new(DEFAULT_STATE_DIR), selector, count)
}

fn mine_in(base: &Path, selector: &str, count: u64) -> Result<()> {
    for_each_target(base, selector, |c| {
        // Mine `count` blocks to a fresh wallet address (coinbase rewards accrue
        // to the boot wallet). generatetoaddress returns the new block hashes.
        let address = rpc::call(c, Some(WALLET), "getnewaddress", json!([]))?
            .as_str()
            .context("getnewaddress did not return an address")?
            .to_string();
        rpc::call(
            c,
            Some(WALLET),
            "generatetoaddress",
            json!([count, address]),
        )?;
        let height = rpc::call(c, None, "getblockcount", json!([]))?
            .as_u64()
            .unwrap_or_default();
        println!("  {}: mined {count} block(s) → height {height}", c.name);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;
    use tempfile::tempdir;

    fn utxo_chain() -> ChainEntry {
        ChainEntry {
            name: "bitcoin-1".into(),
            kind: "bitcoin".into(),
            rpc: "http://wharfnet:wharfnet@127.0.0.1:18443".into(),
            ws: None,
            chain_id: "regtest".into(),
            accounts: vec![],
            tokens: vec![],
            contracts: vec![],
            fork: None,
            explorer: None,
        }
    }

    fn write_manifest(base: &Path, chains: Vec<ChainEntry>) {
        Manifest::new(chains).write(&manifest_path(base)).unwrap();
    }

    #[test]
    fn errors_when_not_running() {
        let dir = tempdir().unwrap();
        assert!(
            mine_in(dir.path(), "bitcoin", 1)
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
    }

    #[test]
    fn errors_when_no_chain_matches() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![utxo_chain()]);
        let err = mine_in(dir.path(), "nope", 1).unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn errors_on_non_utxo_chain() {
        let dir = tempdir().unwrap();
        let mut evm = utxo_chain();
        evm.name = "anvil-1".into();
        evm.kind = "evm".into();
        write_manifest(dir.path(), vec![evm]);
        let err = mine_in(dir.path(), "evm", 1).unwrap_err();
        assert!(
            err.to_string()
                .contains("only supported on Bitcoin/Litecoin"),
            "{err}"
        );
    }
}

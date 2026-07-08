//! EVM chain-control commands (exposed under `wharfnet evm …`): thin wrappers
//! over Anvil's cheat RPCs for driving a running localnet — mine blocks, travel
//! through time, impersonate accounts, and snapshot/revert state.
//!
//! Each command takes a `--chain` selector (`evm` to hit every EVM chain, or a
//! name like `anvil-1`) and runs against the container's internal RPC via
//! [`Session`].

use anyhow::Result;
use std::path::Path;

use crate::evm::{self, Session};
use crate::manifest::ChainEntry;
use crate::orchestrator::{DEFAULT_PROJECT, DEFAULT_STATE_DIR};

/// Open the localnet and run `f` against every EVM chain matching `selector`.
fn for_each_target<F>(base: &Path, project: &str, selector: &str, mut f: F) -> Result<()>
where
    F: FnMut(&Session, &ChainEntry) -> Result<()>,
{
    let session = Session::open(base, project)?;
    for chain in session.targets(selector)? {
        evm::ensure_evm(chain)?;
        f(&session, chain)?;
    }
    Ok(())
}

pub fn mine(selector: &str, count: u64) -> Result<()> {
    mine_in(
        Path::new(DEFAULT_STATE_DIR),
        DEFAULT_PROJECT,
        selector,
        count,
    )
}

fn mine_in(base: &Path, project: &str, selector: &str, count: u64) -> Result<()> {
    for_each_target(base, project, selector, |s, c| {
        s.cast_rpc(c, "anvil_mine", &[&format!("0x{count:x}")])?;
        println!(
            "  {}: mined {count} block(s) → block {}",
            c.name,
            s.block_number(c)?
        );
        Ok(())
    })
}

pub fn increase_time(selector: &str, seconds: u64) -> Result<()> {
    increase_time_in(
        Path::new(DEFAULT_STATE_DIR),
        DEFAULT_PROJECT,
        selector,
        seconds,
    )
}

fn increase_time_in(base: &Path, project: &str, selector: &str, seconds: u64) -> Result<()> {
    for_each_target(base, project, selector, |s, c| {
        // Advance the clock, then mine a block so the new time takes effect.
        s.cast_rpc(c, "evm_increaseTime", &[&seconds.to_string()])?;
        s.cast_rpc(c, "evm_mine", &[])?;
        println!(
            "  {}: advanced time by {seconds}s → now {}",
            c.name,
            block_timestamp(s, c)?
        );
        Ok(())
    })
}

pub fn warp(selector: &str, timestamp: u64) -> Result<()> {
    warp_in(
        Path::new(DEFAULT_STATE_DIR),
        DEFAULT_PROJECT,
        selector,
        timestamp,
    )
}

fn warp_in(base: &Path, project: &str, selector: &str, timestamp: u64) -> Result<()> {
    for_each_target(base, project, selector, |s, c| {
        s.cast_rpc(c, "evm_setNextBlockTimestamp", &[&timestamp.to_string()])?;
        s.cast_rpc(c, "evm_mine", &[])?;
        println!(
            "  {}: warped to timestamp {}",
            c.name,
            block_timestamp(s, c)?
        );
        Ok(())
    })
}

pub fn impersonate(selector: &str, address: &str, stop: bool) -> Result<()> {
    impersonate_in(
        Path::new(DEFAULT_STATE_DIR),
        DEFAULT_PROJECT,
        selector,
        address,
        stop,
    )
}

fn impersonate_in(
    base: &Path,
    project: &str,
    selector: &str,
    address: &str,
    stop: bool,
) -> Result<()> {
    evm::validate_address(address)?;
    for_each_target(base, project, selector, |s, c| {
        if stop {
            s.cast_rpc(c, "anvil_stopImpersonatingAccount", &[address])?;
            println!("  {}: stopped impersonating {address}", c.name);
        } else {
            s.cast_rpc(c, "anvil_impersonateAccount", &[address])?;
            println!(
                "  {}: impersonating {address}\n     send as it with: cast send <to> --from {address} --unlocked --rpc-url {}",
                c.name, c.rpc
            );
        }
        Ok(())
    })
}

pub fn snapshot(selector: &str) -> Result<()> {
    snapshot_in(Path::new(DEFAULT_STATE_DIR), DEFAULT_PROJECT, selector)
}

fn snapshot_in(base: &Path, project: &str, selector: &str) -> Result<()> {
    for_each_target(base, project, selector, |s, c| {
        // `cast rpc` returns the id as a JSON string (e.g. `"0x1"`); unwrap the
        // quotes so it's directly usable with `wharfnet revert`.
        let raw = s.cast_rpc(c, "evm_snapshot", &[])?;
        let id = raw.trim().trim_matches('"');
        println!(
            "  {}: snapshot {id}  (restore with: wharfnet evm revert {id} --chain {})",
            c.name, c.name
        );
        Ok(())
    })
}

pub fn revert(selector: &str, id: &str) -> Result<()> {
    revert_in(Path::new(DEFAULT_STATE_DIR), DEFAULT_PROJECT, selector, id)
}

fn revert_in(base: &Path, project: &str, selector: &str, id: &str) -> Result<()> {
    for_each_target(base, project, selector, |s, c| {
        let ok = s.cast_rpc(c, "evm_revert", &[id])?;
        if ok.trim() == "true" {
            println!("  {}: reverted to snapshot {id}", c.name);
        } else {
            println!("  {}: snapshot {id} not found (already reverted?)", c.name);
        }
        Ok(())
    })
}

/// The `latest` block's Unix timestamp (decimal).
fn block_timestamp(session: &Session, chain: &ChainEntry) -> Result<String> {
    Ok(session
        .cast(
            chain,
            &[
                "block",
                "latest",
                "--field",
                "timestamp",
                "--rpc-url",
                evm::INTERNAL_RPC,
            ],
        )?
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Account, ChainEntry, Manifest};
    use crate::orchestrator::manifest_path;
    use tempfile::tempdir;

    fn evm_chain() -> ChainEntry {
        ChainEntry {
            name: "anvil-1".into(),
            kind: "evm".into(),
            rpc: "http://127.0.0.1:8545".into(),
            chain_id: 31337,
            accounts: vec![Account {
                address: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".into(),
                private_key: "0xac09".into(),
                balance: "10000 ETH".into(),
            }],
            tokens: vec![],
            contracts: vec![],
            explorer: None,
        }
    }

    fn write_manifest(base: &Path, chains: Vec<ChainEntry>) {
        Manifest::new(chains).write(&manifest_path(base)).unwrap();
    }

    // All of these fail before ever shelling out to docker.

    #[test]
    fn commands_error_when_not_running() {
        let dir = tempdir().unwrap();
        assert!(
            mine_in(dir.path(), "p", "evm", 1)
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
        assert!(
            snapshot_in(dir.path(), "p", "evm")
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
    }

    #[test]
    fn errors_when_no_chain_matches() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = mine_in(dir.path(), "p", "nope", 1).unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn errors_on_non_evm_chain() {
        let dir = tempdir().unwrap();
        let mut solana = evm_chain();
        solana.name = "solana-1".into();
        solana.kind = "solana".into();
        write_manifest(dir.path(), vec![solana]);
        let err = mine_in(dir.path(), "p", "solana", 1).unwrap_err();
        assert!(err.to_string().contains("only supported on EVM"), "{err}");
    }

    #[test]
    fn impersonate_validates_address_first() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = impersonate_in(dir.path(), "p", "evm", "0xnothex", false).unwrap_err();
        assert!(err.to_string().contains("valid EVM address"), "{err}");
    }
}

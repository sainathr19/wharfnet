//! EVM chain-control commands (exposed under `wharfnet evm …`): thin wrappers
//! over Anvil's cheat RPCs for driving a running localnet — mine blocks, travel
//! through time, impersonate accounts, and snapshot/revert state.
//!
//! Each command takes a `--chain` selector (`evm` to hit every EVM chain, or a
//! name like `anvil-1`) and runs against the container's internal RPC via
//! [`Session`].

use anyhow::Result;
use std::path::Path;

use super::session::{self as evm, Session};
use crate::runtime::manifest::ChainEntry;
use crate::runtime::orchestrator::{DEFAULT_PROJECT, DEFAULT_STATE_DIR};

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
        // Mine the block at the target timestamp in a single call, so the
        // interval miner can't slip a block between setting the time and mining.
        s.cast_rpc(c, "evm_mine", &[&format!("{{\"timestamp\":{timestamp}}}")])?;
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
    use crate::runtime::manifest::{Account, ChainEntry, Manifest};
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
            tokens: vec![],
            contracts: vec![],
            fork: None,
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

    // ---- docker-backed end-to-end run against a live Anvil chain ----
    //
    // The tests above only cover the pre-flight checks; these drive each cheat
    // RPC against a real chain and assert the observable effect (blocks mined,
    // clock advanced, state rolled back). Self-skips without Docker.

    use crate::harness::{Localnet, docker_available};

    /// Leading integer from `cast` output, accepting decimal or `0x` hex.
    fn parse_u64(out: &str) -> u64 {
        let tok = out.split_whitespace().next().expect("cast produced output");
        match tok.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16).unwrap(),
            None => tok.parse().unwrap(),
        }
    }

    fn block(session: &Session, chain: &ChainEntry) -> u64 {
        parse_u64(&session.block_number(chain).unwrap())
    }

    fn timestamp(session: &Session, chain: &ChainEntry) -> u64 {
        parse_u64(&block_timestamp(session, chain).unwrap())
    }

    #[test]
    fn evm_controls_drive_a_live_chain() {
        if !docker_available() {
            eprintln!("skipping evm control e2e: docker unavailable");
            return;
        }
        // A large block time freezes auto-mining, so every block below comes
        // from an explicit cheat RPC and the counts stay deterministic.
        let net = Localnet::boot("t-control", 18546, 313380, 3600);
        let (base, project, name) = (net.base(), net.project(), net.chain());

        let session = Session::open(base, project).unwrap();
        let chains = session.targets(name).unwrap();
        let chain = chains[0];

        // mine: block height advances by exactly the requested count.
        let start = block(&session, chain);
        mine_in(base, project, name, 5).unwrap();
        assert_eq!(block(&session, chain), start + 5, "mine 5 → +5 blocks");

        // snapshot / revert: capture a marker, move past it, then roll back to it.
        // (The command only prints the id, so grab it directly for the revert.)
        let id = session
            .cast_rpc(chain, "evm_snapshot", &[])
            .unwrap()
            .trim()
            .trim_matches('"')
            .to_string();
        mine_in(base, project, name, 3).unwrap();
        assert_eq!(block(&session, chain), start + 8);
        revert_in(base, project, name, &id).unwrap();
        assert_eq!(
            block(&session, chain),
            start + 5,
            "revert rolls state back to the snapshot"
        );
        // And the snapshot command itself runs cleanly against a live chain.
        snapshot_in(base, project, name).unwrap();

        // increase-time: mines a block and moves the clock forward by >= delta.
        let t0 = timestamp(&session, chain);
        increase_time_in(base, project, name, 3600).unwrap();
        assert!(
            timestamp(&session, chain) >= t0 + 3600,
            "clock advances by at least the requested seconds"
        );

        // warp: pins the next block's timestamp to an absolute value.
        let target = timestamp(&session, chain) + 100_000;
        warp_in(base, project, name, target).unwrap();
        assert_eq!(
            timestamp(&session, chain),
            target,
            "warp sets the exact timestamp"
        );

        // impersonate: starting and stopping both succeed against a live chain.
        let whale = "0x000000000000000000000000000000000000dEaD";
        impersonate_in(base, project, name, whale, false).unwrap();
        impersonate_in(base, project, name, whale, true).unwrap();
    }
}

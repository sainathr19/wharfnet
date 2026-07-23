//! zkSync chain-control commands (exposed under `wharfnet zksync …`): thin
//! wrappers over anvil-zksync's Anvil-compatible cheat JSON-RPC methods for
//! driving a running localnet — mine blocks, travel through time, impersonate
//! accounts, and snapshot/revert state.
//!
//! Each command takes a `--chain` selector (`zksync` to hit every zkSync chain,
//! or a name like `zksync-1`) and calls the cheat at the chain's RPC through the
//! shared [`rpc`](super::rpc) client. anvil-zksync implements the same
//! `evm_*`/`anvil_*` methods as Anvil, so these mirror the `wharfnet evm` verbs
//! one-for-one — including snapshot/revert, which the Starknet/Solana engines
//! have no analogue for.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::Path;

use super::rpc;
use crate::evm::session::validate_address;
use crate::runtime::manifest::{ChainEntry, Manifest};
use crate::runtime::orchestrator::{DEFAULT_STATE_DIR, manifest_path};

/// Read the localnet manifest under `base` and run `f` against every zkSync chain
/// matching `selector`. Errors if the localnet isn't running, nothing matches, or
/// a matched chain isn't a zkSync chain. Like the Solana/Starknet controls, this
/// talks to the published RPC directly rather than shelling into the container.
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
        if chain.kind != "zksync" {
            bail!(
                "chain control under `wharfnet zksync` is only supported on zkSync chains (chain '{}' is an {} chain)",
                chain.name,
                chain.kind
            );
        }
        f(chain)?;
    }
    Ok(())
}

/// Parse a `0x`-prefixed hex quantity (as JSON-RPC returns block numbers and
/// timestamps) into a `u64`.
fn hex_quantity(v: &Value) -> Result<u64> {
    let s = v.as_str().context("expected a hex-string quantity")?;
    let hex = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(hex, 16).with_context(|| format!("parsing hex quantity '{s}'"))
}

/// The chain's current block height.
fn block_number(chain: &ChainEntry) -> Result<u64> {
    hex_quantity(&rpc::call(chain, "eth_blockNumber", json!([]))?)
}

/// The `latest` block's Unix timestamp.
fn block_timestamp(chain: &ChainEntry) -> Result<u64> {
    let block = rpc::call(chain, "eth_getBlockByNumber", json!(["latest", false]))?;
    hex_quantity(
        block
            .get("timestamp")
            .context("latest block had no timestamp")?,
    )
}

pub fn mine(selector: &str, count: u64) -> Result<()> {
    mine_in(Path::new(DEFAULT_STATE_DIR), selector, count)
}

fn mine_in(base: &Path, selector: &str, count: u64) -> Result<()> {
    for_each_target(base, selector, |c| {
        rpc::call(c, "anvil_mine", json!([format!("0x{count:x}")]))?;
        println!(
            "  {}: mined {count} block(s) → block {}",
            c.name,
            block_number(c)?
        );
        Ok(())
    })
}

pub fn increase_time(selector: &str, seconds: u64) -> Result<()> {
    increase_time_in(Path::new(DEFAULT_STATE_DIR), selector, seconds)
}

fn increase_time_in(base: &Path, selector: &str, seconds: u64) -> Result<()> {
    for_each_target(base, selector, |c| {
        // Advance the clock, then mine a block so the new time takes effect.
        rpc::call(c, "evm_increaseTime", json!([seconds]))?;
        rpc::call(c, "evm_mine", json!([]))?;
        println!(
            "  {}: advanced time by {seconds}s → now {}",
            c.name,
            block_timestamp(c)?
        );
        Ok(())
    })
}

pub fn warp(selector: &str, timestamp: u64) -> Result<()> {
    warp_in(Path::new(DEFAULT_STATE_DIR), selector, timestamp)
}

fn warp_in(base: &Path, selector: &str, timestamp: u64) -> Result<()> {
    for_each_target(base, selector, |c| {
        // Pin the next block's timestamp, then mine it.
        rpc::call(c, "evm_setNextBlockTimestamp", json!([timestamp]))?;
        rpc::call(c, "evm_mine", json!([]))?;
        println!("  {}: warped to timestamp {}", c.name, block_timestamp(c)?);
        Ok(())
    })
}

pub fn impersonate(selector: &str, address: &str, stop: bool) -> Result<()> {
    impersonate_in(Path::new(DEFAULT_STATE_DIR), selector, address, stop)
}

fn impersonate_in(base: &Path, selector: &str, address: &str, stop: bool) -> Result<()> {
    validate_address(address)?;
    for_each_target(base, selector, |c| {
        if stop {
            rpc::call(c, "anvil_stopImpersonatingAccount", json!([address]))?;
            println!("  {}: stopped impersonating {address}", c.name);
        } else {
            rpc::call(c, "anvil_impersonateAccount", json!([address]))?;
            println!(
                "  {}: impersonating {address}\n     send as it with any client, unlocked, at {}",
                c.name, c.rpc
            );
        }
        Ok(())
    })
}

pub fn snapshot(selector: &str) -> Result<()> {
    snapshot_in(Path::new(DEFAULT_STATE_DIR), selector)
}

fn snapshot_in(base: &Path, selector: &str) -> Result<()> {
    for_each_target(base, selector, |c| {
        // `evm_snapshot` returns the id as a JSON string (e.g. "0x1").
        let id = rpc::call(c, "evm_snapshot", json!([]))?;
        let id = id.as_str().unwrap_or_default();
        println!(
            "  {}: snapshot {id}  (restore with: wharfnet zksync revert {id} --chain {})",
            c.name, c.name
        );
        Ok(())
    })
}

pub fn revert(selector: &str, id: &str) -> Result<()> {
    revert_in(Path::new(DEFAULT_STATE_DIR), selector, id)
}

fn revert_in(base: &Path, selector: &str, id: &str) -> Result<()> {
    for_each_target(base, selector, |c| {
        let ok = rpc::call(c, "evm_revert", json!([id]))?;
        if ok.as_bool().unwrap_or(false) {
            println!("  {}: reverted to snapshot {id}", c.name);
        } else {
            println!("  {}: snapshot {id} not found (already reverted?)", c.name);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::{Account, Manifest};
    use crate::runtime::orchestrator::manifest_path;
    use tempfile::tempdir;

    fn zksync_chain() -> ChainEntry {
        ChainEntry {
            name: "zksync-1".into(),
            kind: "zksync".into(),
            rpc: "http://127.0.0.1:8011".into(),
            ws: None,
            chain_id: "260".into(),
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

    // These all fail before ever touching anvil-zksync.

    #[test]
    fn commands_error_when_not_running() {
        let dir = tempdir().unwrap();
        assert!(
            mine_in(dir.path(), "zksync", 1)
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
        assert!(
            snapshot_in(dir.path(), "zksync")
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
        assert!(
            revert_in(dir.path(), "zksync", "0x1")
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
    }

    #[test]
    fn errors_when_no_chain_matches() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![zksync_chain()]);
        let err = mine_in(dir.path(), "nope", 1).unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn errors_on_non_zksync_chain() {
        let dir = tempdir().unwrap();
        let mut evm = zksync_chain();
        evm.name = "anvil-1".into();
        evm.kind = "evm".into();
        write_manifest(dir.path(), vec![evm]);
        let err = mine_in(dir.path(), "evm", 1).unwrap_err();
        assert!(
            err.to_string().contains("only supported on zkSync"),
            "{err}"
        );
    }

    #[test]
    fn impersonate_validates_address_first() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![zksync_chain()]);
        let err = impersonate_in(dir.path(), "zksync", "0xnothex", false).unwrap_err();
        assert!(err.to_string().contains("valid EVM address"), "{err}");
    }

    #[test]
    fn hex_quantity_parses_and_rejects() {
        assert_eq!(hex_quantity(&json!("0x1a")).unwrap(), 26);
        assert_eq!(hex_quantity(&json!("0x0")).unwrap(), 0);
        assert!(hex_quantity(&json!(42)).is_err(), "not a string");
        assert!(hex_quantity(&json!("0xzz")).is_err(), "not hex");
    }

    // ---- docker-backed end-to-end run against a live anvil-zksync ----
    //
    // Drives each cheat against a real chain and asserts the observable effect
    // (blocks mined, clock advanced/warped, state rolled back). Self-skips
    // without Docker.

    use crate::harness::{Localnet, docker_available};

    const ZKSYNC_CONTROL_PORT: u16 = 18011;

    #[test]
    fn zksync_controls_drive_a_live_chain() {
        if !docker_available() {
            eprintln!("skipping zksync control e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_zksync("t-zks-control", ZKSYNC_CONTROL_PORT);
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];
        let (base, name) = (net.base(), net.chain());

        // mine: block height advances by exactly the requested count (anvil-zksync
        // mines on demand, so no auto-mining slips extra blocks in).
        let start = block_number(chain).unwrap();
        mine_in(base, name, 5).unwrap();
        assert_eq!(
            block_number(chain).unwrap(),
            start + 5,
            "mine 5 → +5 blocks"
        );

        // snapshot / revert: mark, move past it, then roll back to the mark.
        let id = rpc::call(chain, "evm_snapshot", json!([])).unwrap();
        let id = id.as_str().unwrap().to_string();
        mine_in(base, name, 3).unwrap();
        assert_eq!(block_number(chain).unwrap(), start + 8);
        revert_in(base, name, &id).unwrap();
        assert_eq!(
            block_number(chain).unwrap(),
            start + 5,
            "revert rolls state back to the snapshot"
        );
        // And the snapshot command itself runs cleanly against a live chain.
        snapshot_in(base, name).unwrap();

        // increase-time: mines a block and moves the clock forward by >= delta.
        let t0 = block_timestamp(chain).unwrap();
        increase_time_in(base, name, 3600).unwrap();
        assert!(
            block_timestamp(chain).unwrap() >= t0 + 3600,
            "clock advances by at least the requested seconds"
        );

        // warp: pins the next block's timestamp to an absolute value.
        let target = block_timestamp(chain).unwrap() + 100_000;
        warp_in(base, name, target).unwrap();
        assert_eq!(
            block_timestamp(chain).unwrap(),
            target,
            "warp sets the exact timestamp"
        );

        // impersonate: starting and stopping both succeed against a live chain.
        let whale = "0x000000000000000000000000000000000000dEaD";
        impersonate_in(base, name, whale, false).unwrap();
        impersonate_in(base, name, whale, true).unwrap();
    }
}

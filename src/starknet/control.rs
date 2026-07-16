//! Starknet chain-control commands (exposed under `wharfnet starknet …`): thin
//! wrappers over `starknet-devnet`'s cheat JSON-RPC methods for driving a running
//! localnet — create blocks, travel through time, and impersonate accounts.
//!
//! Each command takes a `--chain` selector (`starknet` to hit every Starknet
//! chain, or a name like `starknet-1`) and calls the devnet cheat at the chain's
//! `/rpc` through the shared [`devnet`] client. These mirror the `wharfnet evm`
//! verbs, with two Starknet-specific notes: devnet has no numbered-snapshot
//! mechanism (only block abort), so there is no `snapshot`/`revert`; and account
//! impersonation is only available in forking mode, so it's refused on a chain
//! that isn't forked.

use anyhow::{Context, Result, bail};
use serde_json::json;
use std::path::Path;

use super::{devnet, faucet};
use crate::runtime::manifest::{ChainEntry, Manifest};
use crate::runtime::orchestrator::{DEFAULT_STATE_DIR, manifest_path};

/// Read the localnet manifest under `base` and run `f` against every Starknet
/// chain matching `selector`. Errors if the localnet isn't running, nothing
/// matches, or a matched chain isn't a Starknet chain. There's no `project`
/// argument — unlike the EVM controls this talks to the published RPC directly
/// rather than shelling into the container.
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
        if chain.kind != "starknet" {
            bail!(
                "chain control under `wharfnet starknet` is only supported on Starknet chains (chain '{}' is an {} chain)",
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
        // devnet creates blocks one at a time (no count param), so loop.
        for _ in 0..count {
            devnet::call(c, "devnet_createBlock", json!({}))?;
        }
        println!(
            "  {}: created {count} block(s) → block {}",
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
        // devnet_increaseTime advances the clock and generates a block itself.
        devnet::call(c, "devnet_increaseTime", json!({ "time": seconds }))?;
        println!(
            "  {}: advanced time by {seconds}s → now {}",
            c.name,
            latest_timestamp(c)?
        );
        Ok(())
    })
}

pub fn warp(selector: &str, timestamp: u64) -> Result<()> {
    warp_in(Path::new(DEFAULT_STATE_DIR), selector, timestamp)
}

fn warp_in(base: &Path, selector: &str, timestamp: u64) -> Result<()> {
    for_each_target(base, selector, |c| {
        // Set an absolute time and generate a block so it takes effect. devnet
        // echoes the applied block timestamp in the response.
        let res = devnet::call(
            c,
            "devnet_setTime",
            json!({ "time": timestamp, "generate_block": true }),
        )?;
        let applied = res
            .get("block_timestamp")
            .and_then(|v| v.as_u64())
            .unwrap_or(timestamp);
        println!("  {}: warped to timestamp {applied}", c.name);
        Ok(())
    })
}

pub fn impersonate(selector: &str, address: &str, stop: bool) -> Result<()> {
    impersonate_in(Path::new(DEFAULT_STATE_DIR), selector, address, stop)
}

fn impersonate_in(base: &Path, selector: &str, address: &str, stop: bool) -> Result<()> {
    faucet::validate_address(address)?;
    for_each_target(base, selector, |c| {
        if stop {
            devnet::call(
                c,
                "devnet_stopImpersonateAccount",
                json!({ "account_address": address }),
            )?;
            println!("  {}: stopped impersonating {address}", c.name);
        } else {
            // devnet only impersonates accounts that live on a forked origin, so
            // the cheat is unavailable on a non-forked chain — fail loudly with a
            // hint rather than surfacing devnet's raw error.
            if c.fork.is_none() {
                bail!(
                    "chain '{}': starknet-devnet only supports account impersonation in forking mode. Fork a live network (set fork_url in wharfnet.toml), then impersonate.",
                    c.name
                );
            }
            devnet::call(
                c,
                "devnet_impersonateAccount",
                json!({ "account_address": address }),
            )?;
            println!(
                "  {}: impersonating {address} — devnet will accept transactions from it without a signature",
                c.name
            );
        }
        Ok(())
    })
}

/// The current block height (`starknet_blockNumber`).
fn block_number(chain: &ChainEntry) -> Result<u64> {
    devnet::call(chain, "starknet_blockNumber", json!([]))?
        .as_u64()
        .context("blockNumber response was not a number")
}

/// The `latest` block's Unix timestamp.
fn latest_timestamp(chain: &ChainEntry) -> Result<u64> {
    devnet::call(chain, "starknet_getBlockWithTxHashes", json!(["latest"]))?
        .get("timestamp")
        .and_then(|v| v.as_u64())
        .context("block response had no timestamp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::{Account, Manifest};
    use crate::runtime::orchestrator::manifest_path;
    use tempfile::tempdir;

    fn starknet_chain() -> ChainEntry {
        ChainEntry {
            name: "starknet-1".into(),
            kind: "starknet".into(),
            rpc: "http://127.0.0.1:5050/rpc".into(),
            ws: None,
            chain_id: "0x534e5f5345504f4c4941".into(),
            accounts: vec![Account {
                address: "0x064b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691"
                    .into(),
                private_key: "0x071d7bb07b9a64f6f78ac4c816aff4da9".into(),
                balance: "1000 ETH & 1000 STRK".into(),
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

    // These all fail before ever touching devnet.

    #[test]
    fn commands_error_when_not_running() {
        let dir = tempdir().unwrap();
        assert!(
            mine_in(dir.path(), "starknet", 1)
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
        assert!(
            warp_in(dir.path(), "starknet", 1)
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
    }

    #[test]
    fn errors_when_no_chain_matches() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![starknet_chain()]);
        let err = mine_in(dir.path(), "nope", 1).unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn errors_on_non_starknet_chain() {
        let dir = tempdir().unwrap();
        let mut evm = starknet_chain();
        evm.name = "anvil-1".into();
        evm.kind = "evm".into();
        write_manifest(dir.path(), vec![evm]);
        let err = mine_in(dir.path(), "evm", 1).unwrap_err();
        assert!(
            err.to_string().contains("only supported on Starknet"),
            "{err}"
        );
    }

    #[test]
    fn impersonate_validates_address_first() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![starknet_chain()]);
        let err = impersonate_in(dir.path(), "starknet", "0xnothex", false).unwrap_err();
        assert!(err.to_string().contains("valid Starknet address"), "{err}");
    }

    #[test]
    fn impersonate_requires_a_forked_chain() {
        let dir = tempdir().unwrap();
        // A plain (non-forked) chain: impersonation is refused with a hint.
        write_manifest(dir.path(), vec![starknet_chain()]);
        let err = impersonate_in(dir.path(), "starknet", "0x123", false).unwrap_err();
        assert!(err.to_string().contains("forking mode"), "{err}");
    }

    // ---- docker-backed end-to-end run against a live starknet-devnet ----
    //
    // The tests above only cover pre-flight checks; this drives each cheat
    // against a real chain and asserts the observable effect (blocks created,
    // clock advanced/warped) plus the forking-mode guard on impersonate.
    // Self-skips without Docker.

    use crate::testkit::{Localnet, docker_available};

    /// A dedicated high port, away from the other Starknet e2e ports.
    const SN_CONTROL_PORT: u16 = 5157;

    #[test]
    fn starknet_controls_drive_a_live_chain() {
        if !docker_available() {
            eprintln!("skipping starknet control e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_starknet("t-sn-control", SN_CONTROL_PORT);
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];
        let (base, name) = (net.base(), net.chain());

        // mine: block height advances by exactly the requested count.
        let start = block_number(chain).unwrap();
        mine_in(base, name, 5).unwrap();
        assert_eq!(
            block_number(chain).unwrap(),
            start + 5,
            "mine 5 → +5 blocks"
        );

        // increase-time: moves the clock forward by at least the delta.
        let t0 = latest_timestamp(chain).unwrap();
        increase_time_in(base, name, 3600).unwrap();
        assert!(
            latest_timestamp(chain).unwrap() >= t0 + 3600,
            "clock advances by at least the requested seconds"
        );

        // warp: pins the next block's timestamp to an absolute value.
        let target = latest_timestamp(chain).unwrap() + 100_000;
        warp_in(base, name, target).unwrap();
        assert_eq!(
            latest_timestamp(chain).unwrap(),
            target,
            "warp sets the exact timestamp"
        );

        // impersonate on a non-forked chain is refused with the forking hint.
        let err = impersonate_in(base, name, "0x123", false).unwrap_err();
        assert!(err.to_string().contains("forking mode"), "{err}");
    }
}

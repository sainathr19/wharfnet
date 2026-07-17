//! Solana chain-control commands (exposed under `wharfnet solana …`): thin
//! wrappers over surfpool's `surfnet_*` cheat JSON-RPC methods for driving a
//! running localnet — advance slots, travel through time, and freeze/resume the
//! clock.
//!
//! Each command takes a `--chain` selector (`solana` to hit every Solana chain,
//! or a name like `solana-1`) and calls the cheat at the chain's RPC through the
//! shared [`rpc`](super::rpc) client. These mirror the `wharfnet evm` / `wharfnet
//! starknet` verbs, with a few surfpool-specific differences:
//!
//! * **`warp` takes a Unix timestamp and is forward-only** — surfpool's time
//!   travel cannot rewind, so warping to a past time is refused.
//! * **There is no `impersonate` and no `snapshot`/`revert`** — surfpool has no
//!   unsigned-transaction impersonation (set account state directly instead), and
//!   no numbered-snapshot mechanism.
//! * **`pause-clock` / `resume-clock`** are surfpool extras with no EVM/Starknet
//!   analogue: surfpool auto-produces slots on a timer, so pausing freezes the
//!   chain for step-by-step control (then `mine` advances it deterministically).

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::Path;

use super::rpc;
use crate::runtime::manifest::{ChainEntry, Manifest};
use crate::runtime::orchestrator::{DEFAULT_STATE_DIR, manifest_path};

/// The Solana Clock sysvar address (fixed and well-known).
const CLOCK_SYSVAR: &str = "SysvarC1ock11111111111111111111111111111111";

/// Read the localnet manifest under `base` and run `f` against every Solana chain
/// matching `selector`. Errors if the localnet isn't running, nothing matches, or
/// a matched chain isn't a Solana chain. Like the Starknet controls, this talks
/// to the published RPC directly rather than shelling into the container.
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
        if chain.kind != "solana" {
            bail!(
                "chain control under `wharfnet solana` is only supported on Solana chains (chain '{}' is an {} chain)",
                chain.name,
                chain.kind
            );
        }
        f(chain)?;
    }
    Ok(())
}

/// The chain's current `(slot, unix_timestamp)` from the Clock sysvar.
///
/// surfpool's `getSlot` reports a lagging *confirmed* slot, but time travel
/// compares against the live tip — so the Clock sysvar (read as `jsonParsed`) is
/// the reliable source for both the slot and the wall-clock time.
fn clock(chain: &ChainEntry) -> Result<(u64, i64)> {
    let res = rpc::call(
        chain,
        "getAccountInfo",
        json!([CLOCK_SYSVAR, { "encoding": "jsonParsed" }]),
    )?;
    let info = res
        .get("value")
        .and_then(|v| v.get("data"))
        .and_then(|d| d.get("parsed"))
        .and_then(|p| p.get("info"))
        .context("clock sysvar response had no parsed info")?;
    let slot = info
        .get("slot")
        .and_then(Value::as_u64)
        .context("clock info had no slot")?;
    let unix = info
        .get("unixTimestamp")
        .and_then(Value::as_i64)
        .context("clock info had no unixTimestamp")?;
    Ok((slot, unix))
}

pub fn mine(selector: &str, count: u64) -> Result<()> {
    mine_in(Path::new(DEFAULT_STATE_DIR), selector, count)
}

fn mine_in(base: &Path, selector: &str, count: u64) -> Result<()> {
    for_each_target(base, selector, |c| {
        // Time travel is absolute, so read the live tip and jump `count` ahead.
        let (slot, _) = clock(c)?;
        rpc::call(
            c,
            "surfnet_timeTravel",
            json!([{ "absoluteSlot": slot + count }]),
        )?;
        let (now, _) = clock(c)?;
        println!("  {}: advanced {count} slot(s) → slot {now}", c.name);
        Ok(())
    })
}

pub fn increase_time(selector: &str, seconds: u64) -> Result<()> {
    increase_time_in(Path::new(DEFAULT_STATE_DIR), selector, seconds)
}

fn increase_time_in(base: &Path, selector: &str, seconds: u64) -> Result<()> {
    for_each_target(base, selector, |c| {
        // surfpool's absoluteTimestamp is in milliseconds; the clock is seconds.
        let (_, unix) = clock(c)?;
        let target_ms = (unix + seconds as i64) * 1000;
        rpc::call(
            c,
            "surfnet_timeTravel",
            json!([{ "absoluteTimestamp": target_ms }]),
        )?;
        let (_, now) = clock(c)?;
        println!("  {}: advanced time by {seconds}s → unix {now}", c.name);
        Ok(())
    })
}

pub fn warp(selector: &str, timestamp: u64) -> Result<()> {
    warp_in(Path::new(DEFAULT_STATE_DIR), selector, timestamp)
}

fn warp_in(base: &Path, selector: &str, timestamp: u64) -> Result<()> {
    for_each_target(base, selector, |c| {
        // Reject a past target up front with a clear message rather than surfacing
        // surfpool's raw "cannot travel to past timestamp" error.
        let (_, unix) = clock(c)?;
        if (timestamp as i64) <= unix {
            bail!(
                "chain '{}': surfpool can only travel forward — target {timestamp} is not after the current time {unix}",
                c.name
            );
        }
        rpc::call(
            c,
            "surfnet_timeTravel",
            json!([{ "absoluteTimestamp": (timestamp as i64) * 1000 }]),
        )?;
        println!("  {}: warped to unix timestamp {timestamp}", c.name);
        Ok(())
    })
}

pub fn pause_clock(selector: &str) -> Result<()> {
    pause_clock_in(Path::new(DEFAULT_STATE_DIR), selector)
}

fn pause_clock_in(base: &Path, selector: &str) -> Result<()> {
    for_each_target(base, selector, |c| {
        rpc::call(c, "surfnet_pauseClock", json!([]))?;
        println!(
            "  {}: clock paused — slot/block production is frozen until `resume-clock`",
            c.name
        );
        Ok(())
    })
}

pub fn resume_clock(selector: &str) -> Result<()> {
    resume_clock_in(Path::new(DEFAULT_STATE_DIR), selector)
}

fn resume_clock_in(base: &Path, selector: &str) -> Result<()> {
    for_each_target(base, selector, |c| {
        rpc::call(c, "surfnet_resumeClock", json!([]))?;
        println!("  {}: clock resumed — slots are advancing again", c.name);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::{Account, Manifest};
    use crate::runtime::orchestrator::manifest_path;
    use tempfile::tempdir;

    fn solana_chain() -> ChainEntry {
        ChainEntry {
            name: "solana-1".into(),
            kind: "solana".into(),
            rpc: "http://127.0.0.1:8899".into(),
            ws: Some("ws://127.0.0.1:8900".into()),
            chain_id: "localnet".into(),
            accounts: vec![Account {
                address: "9akreS78QY4sx2d3aXHdrPCv1rQay1JXoiVWXK6rP9jh".into(),
                private_key: "27qKSFAf".into(),
                balance: "10000 SOL".into(),
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

    // These all fail before ever touching surfpool.

    #[test]
    fn commands_error_when_not_running() {
        let dir = tempdir().unwrap();
        assert!(
            mine_in(dir.path(), "solana", 1)
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
        assert!(
            pause_clock_in(dir.path(), "solana")
                .unwrap_err()
                .to_string()
                .contains("not running")
        );
    }

    #[test]
    fn errors_when_no_chain_matches() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![solana_chain()]);
        let err = mine_in(dir.path(), "nope", 1).unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn errors_on_non_solana_chain() {
        let dir = tempdir().unwrap();
        let mut evm = solana_chain();
        evm.name = "anvil-1".into();
        evm.kind = "evm".into();
        write_manifest(dir.path(), vec![evm]);
        let err = mine_in(dir.path(), "evm", 1).unwrap_err();
        assert!(
            err.to_string().contains("only supported on Solana"),
            "{err}"
        );
    }

    // ---- docker-backed end-to-end run against a live surfpool ----
    //
    // Drives each cheat against a real chain and asserts the observable effect
    // (slots advanced, clock moved/warped, production frozen then resumed) plus
    // the forward-only guard on warp. Self-skips without Docker.

    use crate::harness::{Localnet, docker_available};
    use std::thread::sleep;
    use std::time::Duration;

    /// A dedicated high port, away from the other e2e ports. Solana chains now
    /// also publish RPC + 1 (WS), so these ports are spaced 10 apart to keep
    /// adjacent slots from overlapping.
    const SOL_CONTROL_PORT: u16 = 18960;

    #[test]
    fn solana_controls_drive_a_live_chain() {
        if !docker_available() {
            eprintln!("skipping solana control e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_solana("t-sol-control", SOL_CONTROL_PORT);
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];
        let (base, name) = (net.base(), net.chain());

        // mine: the slot jumps forward by at least the requested count.
        let (start_slot, _) = clock(chain).unwrap();
        mine_in(base, name, 1000).unwrap();
        let (after_mine, _) = clock(chain).unwrap();
        assert!(
            after_mine >= start_slot + 1000,
            "mine 1000 should advance the slot by >= 1000 ({start_slot} -> {after_mine})"
        );

        // increase-time: the clock moves forward by at least the delta.
        let (_, t0) = clock(chain).unwrap();
        increase_time_in(base, name, 100_000).unwrap();
        let (_, t1) = clock(chain).unwrap();
        assert!(
            t1 >= t0 + 100_000,
            "increase-time 100000 should advance the clock by >= 100000s ({t0} -> {t1})"
        );

        // warp: jumps the clock to (at least) an absolute future timestamp.
        let (_, now) = clock(chain).unwrap();
        let target = (now as u64) + 500_000;
        warp_in(base, name, target).unwrap();
        let (_, warped) = clock(chain).unwrap();
        assert!(
            warped >= target as i64,
            "warp should set the clock to >= the target ({target} -> {warped})"
        );

        // warp backwards is refused with the forward-only message.
        let (_, cur) = clock(chain).unwrap();
        let err = warp_in(base, name, (cur as u64).saturating_sub(1000)).unwrap_err();
        assert!(err.to_string().contains("only travel forward"), "{err}");

        // pause-clock: slot production freezes; resume-clock: it advances again.
        pause_clock_in(base, name).unwrap();
        let (p1, _) = clock(chain).unwrap();
        sleep(Duration::from_secs(2));
        let (p2, _) = clock(chain).unwrap();
        assert_eq!(p1, p2, "paused clock must not advance ({p1} -> {p2})");

        resume_clock_in(base, name).unwrap();
        let (r1, _) = clock(chain).unwrap();
        sleep(Duration::from_secs(2));
        let (r2, _) = clock(chain).unwrap();
        assert!(r2 > r1, "resumed clock must advance again ({r1} -> {r2})");
    }
}

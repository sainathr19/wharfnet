//! The zkSync funder: fund an address on a running anvil-zksync chain.
//!
//! Tops up the native coin (ETH) additively via the `anvil_setBalance` cheat,
//! over the shared [`rpc`](super::rpc) client (the image ships no `cast`). Called
//! by the faucet coordinator (see [`crate::faucet`]) for `kind = "zksync"`. There
//! are no bundled test tokens yet, so `--token` is rejected.

use anyhow::{Context, Result, bail};
use serde_json::json;

use super::rpc;
use crate::evm::session::validate_address;
use crate::runtime::amount::to_base_units;
use crate::runtime::manifest::ChainEntry;
use crate::runtime::ui;

/// Decimals of the native coin (ETH), so a decimal `amount` scales to wei.
const NATIVE_DECIMALS: u32 = 18;

/// Fund `address` on a single zkSync `chain` with native ETH. `amount` is a
/// decimal number of whole units unless `raw`, in which case it's exact base
/// units (wei). `token` must be `None` (or "ETH") — no other tokens exist yet.
pub fn fund_chain(
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    token: Option<&str>,
    raw: bool,
) -> Result<()> {
    validate_address(address)?;
    // Only the native coin is available; reject a specific-token request with a
    // clear message rather than silently funding ETH instead.
    if let Some(symbol) = token
        && !symbol.eq_ignore_ascii_case("ETH")
    {
        bail!(
            "no test tokens are deployed on {} — it funds native ETH only (drop --token, or use --token ETH)",
            chain.name
        );
    }
    fund_eth(chain, address, amount, raw)
}

/// Top up native ETH additively via `anvil_setBalance` (read current, add, set)
/// so an existing balance is never clobbered.
fn fund_eth(chain: &ChainEntry, address: &str, amount: &str, raw: bool) -> Result<()> {
    let pb = ui::spinner(format!("{}: funding {amount} ETH…", chain.name));
    let result = (|| -> Result<()> {
        let current = eth_balance_wei(chain, address)?;
        let add = to_base_units(amount, NATIVE_DECIMALS, raw)?;
        let new = current.checked_add(add).context("ETH balance overflow")?;
        rpc::call(
            chain,
            "anvil_setBalance",
            json!([address, format!("0x{new:x}")]),
        )
        .map(|_| ())
    })();

    match result {
        Ok(()) => {
            ui::finish_ok(&pb, format!("{}: +{amount} ETH → {address}", chain.name));
            Ok(())
        }
        Err(e) => {
            ui::finish_err(&pb, format!("{}: ETH funding failed", chain.name));
            Err(e)
        }
    }
}

/// The address's current ETH balance in wei, via `eth_getBalance`.
fn eth_balance_wei(chain: &ChainEntry, address: &str) -> Result<u128> {
    let res = rpc::call(chain, "eth_getBalance", json!([address, "latest"]))?;
    let s = res.as_str().context("eth_getBalance did not return a string")?;
    let hex = s.strip_prefix("0x").unwrap_or(s);
    u128::from_str_radix(hex, 16).with_context(|| format!("parsing balance '{s}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zksync_chain() -> ChainEntry {
        ChainEntry {
            name: "zksync-1".into(),
            kind: "zksync".into(),
            rpc: "http://127.0.0.1:8011".into(),
            ws: None,
            chain_id: "260".into(),
            accounts: vec![],
            tokens: vec![],
            contracts: vec![],
            fork: None,
            explorer: None,
        }
    }

    // Both of these fail before any RPC call, so they need no live chain.

    #[test]
    fn rejects_an_invalid_address() {
        let err = fund_chain(&zksync_chain(), "0xnothex", "100", None, false).unwrap_err();
        assert!(err.to_string().contains("valid EVM address"), "{err}");
    }

    #[test]
    fn rejects_a_non_native_token() {
        let valid = "0x000000000000000000000000000000000000dEaD";
        let err = fund_chain(&zksync_chain(), valid, "100", Some("USDC"), false).unwrap_err();
        assert!(err.to_string().contains("native ETH only"), "{err}");
        // "ETH" is accepted as an explicit native selector — it fails later (no
        // live chain), not on the token check.
        let err = fund_chain(&zksync_chain(), valid, "100", Some("ETH"), false).unwrap_err();
        assert!(!err.to_string().contains("native ETH only"), "{err}");
    }

    // ---- docker-backed end-to-end run against a live anvil-zksync ----

    use crate::harness::{Localnet, docker_available};
    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;

    const ZKSYNC_FAUCET_PORT: u16 = 18021;
    const WEI_PER_ETH: u128 = 1_000_000_000_000_000_000;

    #[test]
    fn faucet_funds_eth_on_a_live_chain() {
        if !docker_available() {
            eprintln!("skipping zksync faucet e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_zksync("t-zks-faucet", ZKSYNC_FAUCET_PORT);
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];
        // A non-dev address, so it starts with zero ETH.
        let recipient = "0x000000000000000000000000000000000000dEaD";

        assert_eq!(eth_balance_wei(chain, recipient).unwrap(), 0);

        // Fund is additive: two top-ups accumulate.
        fund_chain(chain, recipient, "100", None, false).unwrap();
        assert_eq!(eth_balance_wei(chain, recipient).unwrap(), 100 * WEI_PER_ETH);
        fund_chain(chain, recipient, "25", None, false).unwrap();
        assert_eq!(eth_balance_wei(chain, recipient).unwrap(), 125 * WEI_PER_ETH);

        // A --raw amount is taken as exact base units (wei).
        fund_chain(chain, recipient, "1", None, true).unwrap();
        assert_eq!(
            eth_balance_wei(chain, recipient).unwrap(),
            125 * WEI_PER_ETH + 1
        );
    }
}

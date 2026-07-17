//! The UTXO funder: send native coin to an address on a running bitcoind/litecoind
//! regtest chain.
//!
//! The boot wallet (see [`engine`](super::engine)) holds a mature coinbase, so the
//! faucet simply `sendtoaddress` from it and mines one block to confirm the send.
//! UTXO chains carry no test tokens, so only the native coin (BTC/LTC) is funded.
//!
//! Called by the faucet coordinator (see [`crate::faucet`]), which resolves the
//! target chain from the manifest and dispatches here for the UTXO kinds.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use super::rpc::{self, WALLET};
use crate::runtime::amount::to_base_units;
use crate::runtime::manifest::ChainEntry;
use crate::runtime::ui;

/// Satoshis (or litoshis) per coin: both chains use 8 decimals.
const COIN_DECIMALS: u32 = 8;
const SATS_PER_COIN: u128 = 100_000_000;

/// Native symbol for a UTXO `chain`, derived from its kind.
fn symbol(chain: &ChainEntry) -> &'static str {
    if chain.kind == "litecoin" {
        "LTC"
    } else {
        "BTC"
    }
}

/// Fund `address` on a single UTXO `chain` with native coin. `token` may name the
/// native symbol (`BTC`/`LTC`) or be omitted; any other token errors (UTXO chains
/// have none).
pub fn fund_chain(
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    token: Option<&str>,
    raw: bool,
) -> Result<()> {
    validate_address(address)?;
    let native = symbol(chain);
    match token {
        Some(sym) if sym.eq_ignore_ascii_case(native) => fund_native(chain, address, amount, raw),
        Some(sym) => bail!(
            "token '{sym}' is not available on {} — {} chains carry only the native {native}",
            chain.name,
            chain.kind
        ),
        None => fund_native(chain, address, amount, raw),
    }
}

/// Send `amount` native coin to `address` from the boot wallet, then mine one
/// block so the send confirms.
fn fund_native(chain: &ChainEntry, address: &str, amount: &str, raw: bool) -> Result<()> {
    let native = symbol(chain);
    let pb = ui::spinner(format!("{}: sending {amount} {native}…", chain.name));
    let result = (|| -> Result<()> {
        let sats = to_base_units(amount, COIN_DECIMALS, raw)?;
        // bitcoind wants a decimal-coin amount with at most 8 places; build it
        // exactly from the base units rather than via a lossy float.
        let coins = format_coins(sats);
        // `address` is validated to be alphanumeric, so it's safe to splice into
        // the JSON array literal (keeps the amount an exact decimal, not an f64).
        let params: Value = serde_json::from_str(&format!(r#"["{address}",{coins}]"#))
            .context("building sendtoaddress params")?;
        rpc::call(chain, Some(WALLET), "sendtoaddress", params)?;
        // Confirm the send by mining one block to a wallet address.
        let miner = rpc::call(chain, Some(WALLET), "getnewaddress", json!([]))?
            .as_str()
            .context("getnewaddress did not return an address")?
            .to_string();
        rpc::call(chain, Some(WALLET), "generatetoaddress", json!([1, miner]))?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            ui::finish_ok(
                &pb,
                format!("{}: +{amount} {native} → {address}", chain.name),
            );
            Ok(())
        }
        Err(e) => {
            ui::finish_err(&pb, format!("{}: {native} funding failed", chain.name));
            Err(e)
        }
    }
}

/// Render base units as a decimal-coin string with exactly 8 places (e.g.
/// `150_000_000` → `"1.50000000"`), the form bitcoind's `sendtoaddress` expects.
fn format_coins(sats: u128) -> String {
    format!("{}.{:08}", sats / SATS_PER_COIN, sats % SATS_PER_COIN)
}

/// Validate a recipient address well enough to (a) fail fast on junk and (b) make
/// it safe to splice into the JSON request. Bitcoin/Litecoin addresses are base58
/// or bech32 — always ASCII alphanumeric — so anything else is rejected and the
/// node does the real checksum validation on `sendtoaddress`.
fn validate_address(address: &str) -> Result<()> {
    if address.is_empty() || !address.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(anyhow!(
            "'{address}' is not a valid Bitcoin/Litecoin address"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::ChainEntry;

    fn utxo_chain(kind: &str) -> ChainEntry {
        ChainEntry {
            name: format!("{kind}-1"),
            kind: kind.to_string(),
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

    #[test]
    fn format_coins_renders_eight_places() {
        assert_eq!(format_coins(0), "0.00000000");
        assert_eq!(format_coins(150_000_000), "1.50000000");
        assert_eq!(format_coins(1), "0.00000001");
        assert_eq!(format_coins(5_000_000_000), "50.00000000");
    }

    #[test]
    fn symbol_tracks_the_kind() {
        assert_eq!(symbol(&utxo_chain("bitcoin")), "BTC");
        assert_eq!(symbol(&utxo_chain("litecoin")), "LTC");
    }

    #[test]
    fn validate_address_accepts_alnum_and_rejects_junk() {
        assert!(validate_address("bcrt1qzyzhxjntkqvlf4fqsgzdacx6authulk2gdzs74").is_ok());
        assert!(validate_address("rltc1qmh5vvfq7hugnh0355n53zle5tzshl29384zshh").is_ok());
        assert!(validate_address("").is_err());
        // Anything that could break out of the JSON string is rejected.
        assert!(validate_address("addr\",99],[\"x").is_err());
        assert!(validate_address("has space").is_err());
    }

    #[test]
    fn unknown_token_is_rejected_for_utxo() {
        let err =
            fund_chain(&utxo_chain("bitcoin"), "abc123", "1", Some("USDC"), false).unwrap_err();
        assert!(err.to_string().contains("only the native BTC"), "{err}");
    }
}

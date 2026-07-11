//! The EVM funder: fund an address on a running Anvil chain.
//!
//! Tops up the native coin via `anvil_setBalance` and mints the bundled ERC-20
//! test tokens via their public `mint`, all through `cast` inside the chain's
//! container. Called by the faucet coordinator (see [`crate::faucet`]), which
//! resolves the target chain from the manifest and dispatches here for
//! `kind = "evm"`.

use anyhow::{Context, Result};
use std::path::Path;

use super::session::{self as evm, INTERNAL_RPC, Session};
use crate::runtime::amount::to_base_units;
use crate::runtime::manifest::{ChainEntry, Token};
use crate::runtime::ui;

/// Decimals of the native coin (ETH), so a decimal `amount` scales to wei.
const NATIVE_DECIMALS: u32 = 18;

/// Fund `address` on a single EVM `chain`: top up native ETH and mint the
/// bundled tokens (or just `token` when set). Called by the faucet coordinator,
/// which has already resolved this chain from the manifest. `amount` is a decimal
/// number of whole units unless `raw`, in which case it's exact base units.
pub fn fund_chain(
    base: &Path,
    project: &str,
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    token: Option<&str>,
    raw: bool,
) -> Result<()> {
    let session = Session::open(base, project)?;
    evm::validate_address(address)?;

    match token {
        // A single token was requested: fund just that one.
        Some(symbol) => {
            let token = find_token(chain, symbol)?;
            fund_token(&session, chain, address, amount, raw, token)?;
        }
        // Default: native coin plus every bundled token.
        None => {
            fund_eth(&session, chain, address, amount, raw)?;
            for token in &chain.tokens {
                fund_token(&session, chain, address, amount, raw, token)?;
            }
        }
    }
    Ok(())
}

/// Top up native ETH additively via `anvil_setBalance` (read current, add,
/// set) so an existing balance is never clobbered and no dev account is drained.
fn fund_eth(
    session: &Session,
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    raw: bool,
) -> Result<()> {
    let pb = ui::spinner(format!("{}: funding {amount} ETH…", chain.name));
    let result = (|| -> Result<()> {
        let current = eth_balance_wei(session, chain, address)?;
        let add = to_base_units(amount, NATIVE_DECIMALS, raw)?;
        let new = current.checked_add(add).context("ETH balance overflow")?;
        session
            .cast_rpc(chain, "anvil_setBalance", &[address, &format!("0x{new:x}")])
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

/// Mint `amount` whole tokens (scaled by the token's decimals) to `address`.
/// Signed by the first dev account purely to pay gas — `mint` is public, so the
/// signer needs no special role and the recipient needs no key.
fn fund_token(
    session: &Session,
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    raw: bool,
    token: &Token,
) -> Result<()> {
    let pb = ui::spinner(format!(
        "{}: minting {amount} {}…",
        chain.name, token.symbol
    ));
    let result = (|| -> Result<()> {
        let base_units = to_base_units(amount, token.decimals as u32, raw)?;
        let signer = chain
            .accounts
            .first()
            .context("no funded dev account available to sign the mint")?;
        session
            .cast(
                chain,
                &[
                    "send",
                    &token.address,
                    "mint(address,uint256)",
                    address,
                    &base_units.to_string(),
                    "--rpc-url",
                    INTERNAL_RPC,
                    "--private-key",
                    &signer.private_key,
                ],
            )
            .map(|_| ())
    })();

    match result {
        Ok(()) => {
            ui::finish_ok(
                &pb,
                format!("{}: +{amount} {} → {address}", chain.name, token.symbol),
            );
            Ok(())
        }
        Err(e) => {
            ui::finish_err(&pb, format!("{}: {} mint failed", chain.name, token.symbol));
            Err(e)
        }
    }
}

fn eth_balance_wei(session: &Session, chain: &ChainEntry, address: &str) -> Result<u128> {
    let out = session.cast(chain, &["balance", address, "--rpc-url", INTERNAL_RPC])?;
    leading_u128(&out)
}

/// The leading decimal integer from `cast` output, ignoring any trailing
/// `[1e6]`-style annotation newer `cast` versions append.
fn leading_u128(out: &str) -> Result<u128> {
    out.split_whitespace()
        .next()
        .unwrap_or("")
        .parse::<u128>()
        .with_context(|| format!("parsing balance '{}'", out.trim()))
}

fn find_token<'a>(chain: &'a ChainEntry, symbol: &str) -> Result<&'a Token> {
    chain
        .tokens
        .iter()
        .find(|t| t.symbol.eq_ignore_ascii_case(symbol))
        .with_context(|| {
            let known = chain
                .tokens
                .iter()
                .map(|t| t.symbol.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "token '{symbol}' is not deployed on {}. Available: {known}",
                chain.name
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::Account;

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

    // The faucet coordinator owns the selector/dispatch error paths (localnet not
    // running, no chain matches, unsupported kind, invalid address, unknown
    // token) — those are tested in `crate::faucet`. Here we cover the EVM-only
    // helpers plus the live-chain funding path.

    // ---- pure helpers ----

    #[test]
    fn find_token_is_case_insensitive() {
        let chain = evm_chain();
        assert!(find_token(&chain, "usdc").is_ok());
        assert!(find_token(&chain, "WBTC").is_err());
    }

    // ---- docker-backed end-to-end run against a live Anvil chain ----
    //
    // Exercises the real funding path (ETH via `anvil_setBalance`, tokens via an
    // on-chain `mint`) and asserts the balances actually changed on-chain. Only
    // the error paths above run in CI; this covers the happy path locally.

    use crate::testkit::{Localnet, docker_available};

    const WEI_PER_ETH: u128 = 1_000_000_000_000_000_000;

    fn parse_uint(out: &str) -> u128 {
        super::leading_u128(out).expect("cast output is a decimal integer")
    }

    #[test]
    fn leading_u128_ignores_trailing_annotation() {
        assert_eq!(super::leading_u128("1000000").unwrap(), 1_000_000);
        assert_eq!(super::leading_u128("1000000 [1e6]\n").unwrap(), 1_000_000);
        assert!(super::leading_u128("not-a-number").is_err());
    }

    fn eth_wei(session: &Session, chain: &ChainEntry, addr: &str) -> u128 {
        parse_uint(
            &session
                .cast(chain, &["balance", addr, "--rpc-url", INTERNAL_RPC])
                .unwrap(),
        )
    }

    fn token_balance(session: &Session, chain: &ChainEntry, token: &str, addr: &str) -> u128 {
        parse_uint(
            &session
                .cast(
                    chain,
                    &[
                        "call",
                        token,
                        "balanceOf(address)(uint256)",
                        addr,
                        "--rpc-url",
                        INTERNAL_RPC,
                    ],
                )
                .unwrap(),
        )
    }

    #[test]
    fn faucet_funds_eth_and_mints_tokens_on_a_live_chain() {
        if !docker_available() {
            eprintln!("skipping faucet e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot("t-faucet", 18545, 313370, 1);
        // A non-dev address, so it starts with zero ETH and zero tokens.
        let recipient = "0x000000000000000000000000000000000000dEaD";

        let session = Session::open(net.base(), net.project()).unwrap();
        let chains = session.targets(net.chain()).unwrap();
        let chain = chains[0];
        let usdc = chain
            .tokens
            .iter()
            .find(|t| t.symbol == "USDC")
            .expect("USDC is baked into the snapshot")
            .address
            .clone();

        // 1) Fund native ETH + every bundled token in one shot. Success alone
        //    proves all five mints (incl. the weird tokens) land without reverting.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            recipient,
            "100",
            None,
            false,
        )
        .expect("funding native + all tokens should succeed");
        assert_eq!(eth_wei(&session, chain, recipient), 100 * WEI_PER_ETH);
        assert_eq!(
            token_balance(&session, chain, &usdc, recipient),
            100_000_000
        ); // 100 @ 6dp

        // 2) Single-token top-up mints only USDC and leaves ETH untouched.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            recipient,
            "50",
            Some("USDC"),
            false,
        )
        .expect("single-token funding should succeed");
        assert_eq!(
            token_balance(&session, chain, &usdc, recipient),
            150_000_000
        );
        assert_eq!(eth_wei(&session, chain, recipient), 100 * WEI_PER_ETH);

        // 3) A second full fund is additive — ETH tops up, USDC accrues again.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            recipient,
            "25",
            None,
            false,
        )
        .expect("second full funding should succeed");
        assert_eq!(eth_wei(&session, chain, recipient), 125 * WEI_PER_ETH);
        assert_eq!(
            token_balance(&session, chain, &usdc, recipient),
            175_000_000
        );

        // 4) A fractional amount scales by decimals (0.5 USDC @ 6dp = 500_000),
        //    and a --raw amount is taken as exact base units.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            recipient,
            "0.5",
            Some("USDC"),
            false,
        )
        .expect("fractional funding should succeed");
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            recipient,
            "1",
            Some("USDC"),
            true,
        )
        .expect("raw funding should succeed");
        // 175_000_000 + 500_000 (0.5) + 1 (raw base unit) = 175_500_001
        assert_eq!(
            token_balance(&session, chain, &usdc, recipient),
            175_500_001
        );
    }
}

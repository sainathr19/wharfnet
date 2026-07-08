//! The faucet: fund an address on a running localnet.
//!
//! `wharfnet faucet <chain> <address> <amount>` tops up the native coin **and**
//! every bundled test token; `--token <SYMBOL>` restricts it to one token.
//!
//! `<chain>` matches either a chain kind (`evm`) — funding the address on every
//! matching chain — or a specific chain name (`anvil-1`).

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::evm::{self, INTERNAL_RPC, Session};
use crate::manifest::{ChainEntry, Token};
use crate::orchestrator::{DEFAULT_PROJECT, DEFAULT_STATE_DIR};
use crate::ui;

const WEI_PER_ETH: u128 = 1_000_000_000_000_000_000;

pub fn run(chain: &str, address: &str, amount: u64, token: Option<&str>) -> Result<()> {
    run_in(
        Path::new(DEFAULT_STATE_DIR),
        DEFAULT_PROJECT,
        chain,
        address,
        amount,
        token,
    )
}

fn run_in(
    base: &Path,
    project: &str,
    selector: &str,
    address: &str,
    amount: u64,
    token: Option<&str>,
) -> Result<()> {
    let session = Session::open(base, project)?;
    for chain in session.targets(selector)? {
        fund_chain(&session, chain, address, amount, token)?;
    }
    Ok(())
}

fn fund_chain(
    session: &Session,
    chain: &ChainEntry,
    address: &str,
    amount: u64,
    token: Option<&str>,
) -> Result<()> {
    if chain.kind != "evm" {
        bail!(
            "faucet is not yet supported for {} chains (chain '{}')",
            chain.kind,
            chain.name
        );
    }
    evm::validate_address(address)?;

    match token {
        // A single token was requested: fund just that one.
        Some(symbol) => {
            let token = find_token(chain, symbol)?;
            fund_token(session, chain, address, amount, token)?;
        }
        // Default: native coin plus every bundled token.
        None => {
            fund_eth(session, chain, address, amount)?;
            for token in &chain.tokens {
                fund_token(session, chain, address, amount, token)?;
            }
        }
    }
    Ok(())
}

/// Top up native ETH additively via `anvil_setBalance` (read current, add,
/// set) so an existing balance is never clobbered and no dev account is drained.
fn fund_eth(session: &Session, chain: &ChainEntry, address: &str, amount: u64) -> Result<()> {
    let pb = ui::spinner(format!("{}: funding {amount} ETH…", chain.name));
    let result = (|| -> Result<()> {
        let current = eth_balance_wei(session, chain, address)?;
        let add = (amount as u128)
            .checked_mul(WEI_PER_ETH)
            .context("ETH amount too large")?;
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
    amount: u64,
    token: &Token,
) -> Result<()> {
    let pb = ui::spinner(format!(
        "{}: minting {amount} {}…",
        chain.name, token.symbol
    ));
    let result = (|| -> Result<()> {
        let raw = (amount as u128)
            .checked_mul(
                10u128
                    .checked_pow(token.decimals as u32)
                    .context("bad decimals")?,
            )
            .context("token amount too large")?;
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
                    &raw.to_string(),
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
    let trimmed = out.trim();
    trimmed
        .parse::<u128>()
        .with_context(|| format!("parsing balance '{trimmed}'"))
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
    use crate::manifest::{Account, Manifest};
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

    // ---- checks that fail before ever shelling out to docker ----

    #[test]
    fn errors_when_localnet_not_running() {
        let dir = tempdir().unwrap();
        let err = run_in(dir.path(), "p", "evm", VALID_ADDR, 100, None).unwrap_err();
        assert!(err.to_string().contains("not running"), "{err}");
    }

    #[test]
    fn errors_when_no_chain_matches_selector() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(dir.path(), "p", "solana", VALID_ADDR, 100, None).unwrap_err();
        assert!(err.to_string().contains("no chain matching"), "{err}");
    }

    #[test]
    fn errors_on_invalid_address() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(dir.path(), "p", "evm", "0xnothex", 100, None).unwrap_err();
        assert!(err.to_string().contains("valid EVM address"), "{err}");
    }

    #[test]
    fn errors_on_unknown_token() {
        let dir = tempdir().unwrap();
        write_manifest(dir.path(), vec![evm_chain()]);
        let err = run_in(dir.path(), "p", "evm", VALID_ADDR, 100, Some("DAI")).unwrap_err();
        assert!(err.to_string().contains("not deployed"), "{err}");
    }

    #[test]
    fn errors_on_non_evm_chain() {
        let dir = tempdir().unwrap();
        let mut solana = evm_chain();
        solana.name = "solana-1".into();
        solana.kind = "solana".into();
        solana.tokens.clear();
        write_manifest(dir.path(), vec![solana]);
        let err = run_in(dir.path(), "p", "solana", VALID_ADDR, 100, None).unwrap_err();
        assert!(err.to_string().contains("not yet supported"), "{err}");
    }

    // ---- pure helpers ----

    #[test]
    fn find_token_is_case_insensitive() {
        let chain = evm_chain();
        assert!(find_token(&chain, "usdc").is_ok());
        assert!(find_token(&chain, "WBTC").is_err());
    }
}

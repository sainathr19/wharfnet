//! The Solana funder: fund an address on a running `surfpool` chain.
//!
//! Native SOL is topped up with the standard `requestAirdrop` JSON-RPC, which is
//! additive and (on surfpool's in-memory SVM) applies immediately. The baked SPL
//! test tokens are funded with the `surfnet_setTokenAccount` cheat — which *sets*
//! a token account's balance — so to stay additive (matching the EVM/Starknet
//! faucets) the current balance is read first and the top-up added to it. Neither
//! path needs the recipient's key.
//!
//! Called by the faucet coordinator (see [`crate::faucet`]), which resolves the
//! target chain from the manifest and dispatches here for `kind = "solana"`.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use super::rpc;
use crate::runtime::amount::to_base_units;
use crate::runtime::manifest::{ChainEntry, Token};
use crate::runtime::ui;

/// Lamports per SOL: 1 SOL = 10^9 lamports.
const NATIVE_DECIMALS: u32 = 9;

/// The SPL Token program — owner of the baked test tokens' mints and accounts.
const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Fund `address` on a single Solana `chain`: airdrop SOL and top up the bundled
/// SPL tokens (or just `token` when set — the pseudo-symbol `SOL` funds only the
/// native coin). Called by the faucet coordinator, which resolved this chain.
pub fn fund_chain(
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    token: Option<&str>,
    raw: bool,
) -> Result<()> {
    validate_address(address)?;
    match token {
        // `--token SOL` funds only the native coin (SOL isn't in the token list).
        Some(symbol) if symbol.eq_ignore_ascii_case("SOL") => {
            fund_sol(chain, address, amount, raw)?
        }
        // A single SPL token was requested: fund just that one.
        Some(symbol) => {
            let token = find_token(chain, symbol)?;
            fund_token(chain, address, amount, raw, token)?;
        }
        // Default: native SOL plus every bundled SPL token.
        None => {
            fund_sol(chain, address, amount, raw)?;
            for token in &chain.tokens {
                fund_token(chain, address, amount, raw, token)?;
            }
        }
    }
    Ok(())
}

/// Airdrop `amount` SOL to `address` via `requestAirdrop`. Additive — it credits
/// lamports on top of any existing balance — and applies immediately on surfpool.
fn fund_sol(chain: &ChainEntry, address: &str, amount: &str, raw: bool) -> Result<()> {
    let pb = ui::spinner(format!("{}: airdropping {amount} SOL…", chain.name));
    let result = (|| -> Result<()> {
        let lamports = lamports_from(amount, NATIVE_DECIMALS, raw)?;
        rpc::call(chain, "requestAirdrop", json!([address, lamports]))?;
        Ok(())
    })();
    finish(&pb, chain, "SOL", amount, address, result)
}

/// Top up `address` with `amount` of an SPL `token`. `surfnet_setTokenAccount`
/// sets the balance, so read the current balance and add to it, keeping the
/// faucet additive like the EVM/Starknet funders.
fn fund_token(
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    raw: bool,
    token: &Token,
) -> Result<()> {
    let pb = ui::spinner(format!(
        "{}: funding {amount} {}…",
        chain.name, token.symbol
    ));
    let result = (|| -> Result<()> {
        let add = lamports_from(amount, token.decimals as u32, raw)?;
        let current = current_token_balance(chain, address, &token.address)?;
        let total = current
            .checked_add(add)
            .context("resulting token balance overflows u64")?;
        rpc::call(
            chain,
            "surfnet_setTokenAccount",
            json!([address, token.address, { "amount": total }, TOKEN_PROGRAM]),
        )?;
        Ok(())
    })();
    finish(&pb, chain, &token.symbol, amount, address, result)
}

/// Scale a faucet `amount` to base units and narrow it to the `u64` that Solana's
/// RPC and cheatcodes expect.
fn lamports_from(amount: &str, decimals: u32, raw: bool) -> Result<u64> {
    let units = to_base_units(amount, decimals, raw)?;
    u64::try_from(units).map_err(|_| anyhow!("amount '{amount}' is too large for a Solana u64"))
}

/// The recipient's current balance (base units) of the SPL token at `mint`, or 0
/// if it holds no token account for that mint yet.
fn current_token_balance(chain: &ChainEntry, owner: &str, mint: &str) -> Result<u64> {
    let res = rpc::call(
        chain,
        "getTokenAccountsByOwner",
        json!([owner, { "mint": mint }, { "encoding": "jsonParsed" }]),
    )?;
    let amount = res
        .get("value")
        .and_then(Value::as_array)
        .and_then(|accounts| accounts.first())
        .and_then(|a| a.pointer("/account/data/parsed/info/tokenAmount/amount"))
        .and_then(Value::as_str);
    match amount {
        Some(s) => s
            .parse::<u64>()
            .with_context(|| format!("token balance '{s}' is not an integer")),
        None => Ok(0),
    }
}

/// Finish a per-token spinner uniformly across both funding paths.
fn finish(
    pb: &indicatif::ProgressBar,
    chain: &ChainEntry,
    symbol: &str,
    amount: &str,
    address: &str,
    result: Result<()>,
) -> Result<()> {
    match result {
        Ok(()) => {
            ui::finish_ok(
                pb,
                format!("{}: +{amount} {symbol} → {address}", chain.name),
            );
            Ok(())
        }
        Err(e) => {
            ui::finish_err(pb, format!("{}: {symbol} funding failed", chain.name));
            Err(e)
        }
    }
}

/// Validate a Solana address: base58 that decodes to exactly 32 bytes (an ed25519
/// public key).
pub(crate) fn validate_address(address: &str) -> Result<()> {
    match base58_decode(address) {
        Some(bytes) if bytes.len() == 32 => Ok(()),
        _ => bail!(
            "'{address}' is not a valid Solana address (expected a base58 32-byte public key)"
        ),
    }
}

/// Decode base58 (Bitcoin/Solana alphabet) to bytes, or `None` on an invalid
/// character. Dependency-free — the only base58 the funder needs is a length
/// check, so a small decoder beats pulling in a crate.
fn base58_decode(s: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if s.is_empty() {
        return None;
    }
    let mut bytes: Vec<u8> = Vec::new(); // little-endian base-256 accumulator
    for ch in s.bytes() {
        let mut carry = ALPHABET.iter().position(|&c| c == ch)? as u32;
        for b in bytes.iter_mut() {
            carry += (*b as u32) * 58;
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    // Each leading '1' is a leading zero byte.
    let leading_zeros = s.bytes().take_while(|&c| c == b'1').count();
    bytes.extend(std::iter::repeat_n(0u8, leading_zeros));
    bytes.reverse();
    Some(bytes)
}

fn find_token<'a>(chain: &'a ChainEntry, symbol: &str) -> Result<&'a Token> {
    chain
        .tokens
        .iter()
        .find(|t| t.symbol.eq_ignore_ascii_case(symbol))
        .with_context(|| {
            let mut known: Vec<&str> = vec!["SOL"];
            known.extend(chain.tokens.iter().map(|t| t.symbol.as_str()));
            format!(
                "token '{symbol}' is not deployed on {}. Available: {}",
                chain.name,
                known.join(", ")
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::Account;

    fn solana_chain() -> ChainEntry {
        ChainEntry {
            name: "solana-1".into(),
            kind: "solana".into(),
            rpc: "http://127.0.0.1:8899".into(),
            chain_id: "localnet".into(),
            accounts: vec![Account {
                address: "9akreS78QY4sx2d3aXHdrPCv1rQay1JXoiVWXK6rP9jh".into(),
                private_key: "27qKSFAf".into(),
                balance: "10000 SOL".into(),
            }],
            tokens: vec![
                Token {
                    symbol: "USDC".into(),
                    name: "USD Coin".into(),
                    address: "94C6wFGeVr5SahK9owBMBhpFPRtvLuZhQQVRh7NYrEp9".into(),
                    decimals: 6,
                },
                Token {
                    symbol: "WBTC".into(),
                    name: "Wrapped BTC".into(),
                    address: "Fp7Dnb8KKkWWw5RfUPsQBNRrooj75gbNaWoC28AnCn3E".into(),
                    decimals: 8,
                },
            ],
            contracts: vec![],
            fork: None,
            explorer: None,
        }
    }

    #[test]
    fn base58_decode_roundtrips_a_known_pubkey() {
        // dev0's address decodes to its known 32-byte key (first/last bytes).
        let bytes = base58_decode("9akreS78QY4sx2d3aXHdrPCv1rQay1JXoiVWXK6rP9jh").unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 127);
        assert_eq!(bytes[31], 236);
        // The SPL Token program id is also a valid 32-byte key.
        assert_eq!(base58_decode(TOKEN_PROGRAM).unwrap().len(), 32);
    }

    #[test]
    fn validate_address_accepts_pubkeys_and_rejects_junk() {
        assert!(validate_address("9akreS78QY4sx2d3aXHdrPCv1rQay1JXoiVWXK6rP9jh").is_ok());
        assert!(validate_address("").is_err());
        assert!(validate_address("0xabc").is_err()); // '0' isn't in the base58 alphabet
        assert!(validate_address("111").is_err()); // decodes to 3 bytes, not 32
        assert!(validate_address("not valid base58!").is_err());
    }

    #[test]
    fn find_token_is_case_insensitive_and_lists_sol_and_unknowns() {
        let chain = solana_chain();
        assert_eq!(find_token(&chain, "usdc").unwrap().symbol, "USDC");
        assert_eq!(find_token(&chain, "WBTC").unwrap().symbol, "WBTC");
        let err = find_token(&chain, "DAI").unwrap_err();
        assert!(err.to_string().contains("not deployed"), "{err}");
        // The available list mentions native SOL alongside the SPL tokens.
        assert!(err.to_string().contains("SOL"), "{err}");
    }

    // ---- docker-backed end-to-end run against a live surfpool ----
    //
    // Boots a real chain and funds a fresh (non-dev) address, then reads balances
    // straight off the node: SOL via getBalance, the SPL tokens via
    // getTokenAccountsByOwner. Exercises the default fund, a single-token top-up,
    // and a second full fund to prove funding is additive. Self-skips w/o Docker.

    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;
    use crate::testkit::{Localnet, docker_available};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    const SOL_FAUCET_PORT: u16 = 18993;
    /// A fresh, non-dev recipient — starts at zero for SOL and every token.
    const RECIPIENT: &str = "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM";

    fn rpc_post(port: u16, body: &str) -> Value {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        let payload = resp.split("\r\n\r\n").nth(1).unwrap_or("");
        serde_json::from_str(payload).unwrap_or_else(|e| panic!("bad json '{payload}': {e}"))
    }

    fn sol_balance(port: u16) -> u64 {
        let v = rpc_post(
            port,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{RECIPIENT}"]}}"#
            ),
        );
        v["result"]["value"].as_u64().unwrap()
    }

    fn token_balance(port: u16, mint: &str) -> u64 {
        let v = rpc_post(
            port,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"getTokenAccountsByOwner","params":["{RECIPIENT}",{{"mint":"{mint}"}},{{"encoding":"jsonParsed"}}]}}"#
            ),
        );
        v["result"]["value"][0]["account"]["data"]["parsed"]["info"]["tokenAmount"]["amount"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .unwrap()
    }

    #[test]
    fn faucet_funds_sol_and_spl_tokens_additively_on_a_live_chain() {
        if !docker_available() {
            eprintln!("skipping solana faucet e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_solana("t-sol-faucet", SOL_FAUCET_PORT);
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];
        let mint = |sym: &str| {
            chain
                .tokens
                .iter()
                .find(|t| t.symbol == sym)
                .unwrap_or_else(|| panic!("{sym} missing"))
                .address
                .clone()
        };
        let (usdc, wbtc) = (mint("USDC"), mint("WBTC"));

        // 1) Default fund: 5 SOL + 5 of every SPL token to a fresh address.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            "solana",
            RECIPIENT,
            "5",
            None,
            false,
        )
        .expect("default funding should succeed");
        assert_eq!(sol_balance(SOL_FAUCET_PORT), 5_000_000_000); // 5 SOL
        assert_eq!(token_balance(SOL_FAUCET_PORT, &usdc), 5_000_000); // 5 @ 6dp
        assert_eq!(token_balance(SOL_FAUCET_PORT, &wbtc), 500_000_000); // 5 @ 8dp

        // 2) Single-token top-up (via the chain-name selector) funds only USDC,
        //    additively; SOL/WBTC untouched.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            RECIPIENT,
            "3",
            Some("USDC"),
            false,
        )
        .expect("single-token funding should succeed");
        assert_eq!(token_balance(SOL_FAUCET_PORT, &usdc), 8_000_000); // 5 + 3
        assert_eq!(sol_balance(SOL_FAUCET_PORT), 5_000_000_000);
        assert_eq!(token_balance(SOL_FAUCET_PORT, &wbtc), 500_000_000);

        // 3) `--token SOL` tops up only the native coin, additively.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            "solana",
            RECIPIENT,
            "2",
            Some("SOL"),
            false,
        )
        .expect("SOL-only funding should succeed");
        assert_eq!(sol_balance(SOL_FAUCET_PORT), 7_000_000_000); // 5 + 2
        assert_eq!(token_balance(SOL_FAUCET_PORT, &usdc), 8_000_000);

        // 4) A second full fund is additive across SOL and every token.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            "solana",
            RECIPIENT,
            "1",
            None,
            false,
        )
        .expect("second full funding should succeed");
        assert_eq!(sol_balance(SOL_FAUCET_PORT), 8_000_000_000); // 7 + 1
        assert_eq!(token_balance(SOL_FAUCET_PORT, &usdc), 9_000_000); // 8 + 1
        assert_eq!(token_balance(SOL_FAUCET_PORT, &wbtc), 600_000_000); // 5 + 1
    }
}

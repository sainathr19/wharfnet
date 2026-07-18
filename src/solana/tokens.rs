//! Baked SPL test tokens for Solana chains.
//!
//! The EVM and Starknet stacks bake a state snapshot/replay that the node loads
//! at boot. surfpool works differently: the SPL Token program is native (nothing
//! to deploy), and its `surfnet_*` cheatcodes write account state directly. So
//! wharfnet seeds the test tokens at runtime, right after the chain's RPC is live
//! (via the engine's [`post_boot`](crate::runtime::engine::Engine::post_boot)
//! hook): it creates each mint with `surfnet_setAccount` (a hand-built SPL Mint
//! account) and funds every dev account with `surfnet_setTokenAccount`.
//!
//! The mint addresses are deterministic — ed25519 of
//! `sha256("wharfnet-solana-mint-<symbol>")`, generated offline and hardcoded
//! (the same documented-seed scheme as the dev accounts) — so they're identical
//! on every chain and reproducible by anyone. They're advertised in the manifest.

use anyhow::Result;
use serde_json::json;

use super::rpc;
use crate::runtime::manifest::{ChainEntry, Token};

/// The SPL Token program — owner of every classic mint and token account.
const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Rent-exempt lamports for an 82-byte SPL Mint account. surfpool funds the
/// token accounts' own rent itself; this covers just the mint.
const MINT_RENT_LAMPORTS: u64 = 1_461_600;

/// Raw 32-byte public key of dev account 0, used as every test mint's authority
/// (so `spl-token mint` works against these mints too). This is the ed25519 key
/// of `sha256("wharfnet-solana-dev-0")` — see [`super::engine`].
const DEV0_PUBKEY: [u8; 32] = [
    127, 131, 64, 12, 229, 118, 213, 25, 113, 51, 22, 248, 115, 168, 8, 18, 93, 172, 0, 193, 44,
    84, 69, 226, 19, 165, 5, 20, 129, 144, 82, 236,
];

/// A baked SPL test token: a mint at a fixed address, seeded onto the dev accounts.
struct TestToken {
    symbol: &'static str,
    name: &'static str,
    /// Deterministic mint address (base58).
    mint: &'static str,
    decimals: u8,
    /// Amount (in base units) seeded onto each dev account at boot.
    seed_amount: u64,
}

/// The baked tokens: standard, well-behaved SPL mints with real-world decimals,
/// seeded generously onto every dev account.
const TOKENS: &[TestToken] = &[
    TestToken {
        symbol: "USDC",
        name: "USD Coin",
        mint: "94C6wFGeVr5SahK9owBMBhpFPRtvLuZhQQVRh7NYrEp9",
        decimals: 6,
        seed_amount: 1_000_000_000_000, // 1,000,000 USDC
    },
    TestToken {
        symbol: "WBTC",
        name: "Wrapped BTC",
        mint: "Fp7Dnb8KKkWWw5RfUPsQBNRrooj75gbNaWoC28AnCn3E",
        decimals: 8,
        seed_amount: 10_000_000_000, // 100 WBTC
    },
];

/// The tokens advertised in the manifest.
pub(crate) fn manifest_tokens() -> Vec<Token> {
    TOKENS
        .iter()
        .map(|t| Token {
            symbol: t.symbol.to_string(),
            name: t.name.to_string(),
            address: t.mint.to_string(),
            decimals: t.decimals,
        })
        .collect()
}

/// Whether the test tokens are already present on-chain, detected by the first
/// mint's existence. Used to skip re-seeding a resumed persistent session (whose
/// db already restored the mints and the user's balances).
pub(crate) fn already_seeded(chain: &ChainEntry) -> Result<bool> {
    let Some(first) = TOKENS.first() else {
        return Ok(true);
    };
    let res = rpc::call(
        chain,
        "getAccountInfo",
        json!([first.mint, { "encoding": "base64" }]),
    )?;
    // getAccountInfo returns `value: null` when the account doesn't exist.
    Ok(res.get("value").is_some_and(|v| !v.is_null()))
}

/// Seed the test tokens onto a live chain via surfpool cheatcodes: create each
/// mint, then fund every dev account's associated token account. Idempotent —
/// re-running overwrites to the same state.
pub(crate) fn seed(chain: &ChainEntry) -> Result<()> {
    let owners: Vec<&str> = chain.accounts.iter().map(|a| a.address.as_str()).collect();
    for token in TOKENS {
        // 1. Create the mint account (owned by the SPL Token program).
        let data = mint_data_hex(token.decimals);
        rpc::call(
            chain,
            "surfnet_setAccount",
            json!([
                token.mint,
                {
                    "lamports": MINT_RENT_LAMPORTS,
                    "owner": TOKEN_PROGRAM,
                    "data": data,
                    "executable": false,
                }
            ]),
        )?;
        // 2. Fund every dev account's token account with the seed amount.
        for owner in &owners {
            rpc::call(
                chain,
                "surfnet_setTokenAccount",
                json!([owner, token.mint, { "amount": token.seed_amount }, TOKEN_PROGRAM]),
            )?;
        }
    }
    Ok(())
}

/// Build the 82-byte SPL Mint account data (hex) for a mint with dev0 as the mint
/// authority, zero initial supply, no freeze authority, and `decimals`.
///
/// Layout (`spl_token::state::Mint`): mint_authority `COption<Pubkey>` (4-byte
/// tag + 32-byte key), supply `u64`, decimals `u8`, is_initialized `u8`,
/// freeze_authority `COption<Pubkey>`.
fn mint_data_hex(decimals: u8) -> String {
    let mut d = Vec::with_capacity(82);
    d.extend_from_slice(&[1, 0, 0, 0]); // mint_authority = Some
    d.extend_from_slice(&DEV0_PUBKEY);
    d.extend_from_slice(&0u64.to_le_bytes()); // supply = 0
    d.push(decimals);
    d.push(1); // is_initialized = true
    d.extend_from_slice(&[0, 0, 0, 0]); // freeze_authority = None
    d.extend_from_slice(&[0u8; 32]);
    debug_assert_eq!(d.len(), 82);
    hex_encode(&d)
}

/// Lowercase hex, no `0x` prefix — the form surfpool's `surfnet_setAccount`
/// expects for the account `data` field.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_tokens_are_the_standard_pair() {
        let tokens = manifest_tokens();
        let symbols: Vec<&str> = tokens.iter().map(|t| t.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["USDC", "WBTC"]);
        assert_eq!(tokens[0].decimals, 6);
        assert_eq!(
            tokens[0].address,
            "94C6wFGeVr5SahK9owBMBhpFPRtvLuZhQQVRh7NYrEp9"
        );
        assert_eq!(tokens[1].decimals, 8);
    }

    #[test]
    fn mint_data_is_a_valid_82_byte_mint() {
        let hex = mint_data_hex(6);
        // 82 bytes → 164 hex chars.
        assert_eq!(hex.len(), 164);
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(bytes.len(), 82);
        // mint_authority present (COption tag = 1) and equals dev0.
        assert_eq!(&bytes[0..4], &[1, 0, 0, 0]);
        assert_eq!(&bytes[4..36], &DEV0_PUBKEY);
        // supply = 0, decimals = 6, is_initialized = 1.
        assert_eq!(&bytes[36..44], &[0u8; 8]);
        assert_eq!(bytes[44], 6);
        assert_eq!(bytes[45], 1);
        // freeze_authority absent (COption tag = 0).
        assert_eq!(&bytes[46..50], &[0, 0, 0, 0]);
    }

    // ---- docker-backed end-to-end run against a live surfpool ----
    //
    // Boots a real chain (whose post-boot hook seeds the tokens) and asserts each
    // mint exists with the right decimals and each dev account holds the seeded
    // balance. Self-skips without Docker.

    use crate::harness::{Localnet, docker_available};
    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    /// A dedicated high port, away from the other Solana e2e ports (spaced 10
    /// apart, since each chain now also publishes RPC + 1 for WS).
    const SOL_TOKENS_PORT: u16 = 18970;

    fn rpc(port: u16, body: &str) -> serde_json::Value {
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

    #[test]
    fn solana_boots_with_seeded_spl_test_tokens() {
        if !docker_available() {
            eprintln!("skipping solana tokens e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_solana("t-sol-tokens", SOL_TOKENS_PORT);
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];

        // The manifest advertises exactly the standard pair.
        let symbols: Vec<&str> = chain.tokens.iter().map(|t| t.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["USDC", "WBTC"]);

        for token in &chain.tokens {
            // Each mint really exists on-chain with the advertised decimals.
            let info = rpc(
                SOL_TOKENS_PORT,
                &format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["{}",{{"encoding":"jsonParsed"}}]}}"#,
                    token.address
                ),
            );
            let parsed = &info["result"]["value"]["data"]["parsed"];
            assert_eq!(
                parsed["type"], "mint",
                "{} should be an SPL mint: {info}",
                token.symbol
            );
            assert_eq!(
                parsed["info"]["decimals"].as_u64().unwrap() as u8,
                token.decimals,
                "{} decimals mismatch",
                token.symbol
            );

            // Every dev account holds a non-zero seeded balance of it.
            for account in &chain.accounts {
                let bal = rpc(
                    SOL_TOKENS_PORT,
                    &format!(
                        r#"{{"jsonrpc":"2.0","id":1,"method":"getTokenAccountsByOwner","params":["{}",{{"mint":"{}"}},{{"encoding":"jsonParsed"}}]}}"#,
                        account.address, token.address
                    ),
                );
                let amount = bal["result"]["value"][0]["account"]["data"]["parsed"]["info"]
                    ["tokenAmount"]["amount"]
                    .as_str()
                    .unwrap_or_else(|| panic!("no {} balance for {}: {bal}", token.symbol, account.address));
                assert!(
                    amount.parse::<u64>().unwrap() > 0,
                    "{} balance for {} should be seeded, got {amount}",
                    token.symbol,
                    account.address
                );
            }
        }
    }
}

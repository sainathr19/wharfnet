//! The Starknet funder: fund an address on a running `starknet-devnet` chain.
//!
//! The two fee tokens (ETH, STRK) are minted through devnet's `POST /mint` cheat
//! (`unit` `WEI` for ETH, `FRI` for STRK). The baked Cairo test tokens
//! (USDC/WBTC/FEE/REB) each expose a public `mint(recipient, amount)`, so those
//! are funded with a signed invoke submitted through the first predeployed dev
//! account — the signer just pays gas; the recipient needs no key.
//!
//! Called by the faucet coordinator (see [`crate::faucet`]), which resolves the
//! target chain from the manifest and dispatches here for `kind = "starknet"`.
//!
//! The pinned devnet speaks JSON-RPC 0.8.1 while `starknet-rs` speaks 0.9, so the
//! invoke pins **all six** resource bounds. That makes `execute_v3` skip the
//! `estimate_fee` round-trip (whose response shape changed in 0.9) and go straight
//! to `get_nonce` + `add_invoke_transaction`, both of which are unchanged.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use starknet::accounts::{Account, ExecutionEncoding, SingleOwnerAccount};
use starknet::core::types::{BlockId, BlockTag, Call, Felt};
use starknet::core::utils::get_selector_from_name;
use starknet::providers::jsonrpc::HttpTransport;
use starknet::providers::{JsonRpcClient, Provider, Url};
use starknet::signers::{LocalWallet, SigningKey};

use crate::runtime::manifest::{ChainEntry, Token};
use crate::runtime::ui;

// Resource-bound maximums for the mint invoke. devnet's gas prices are 1e9
// fri/gas and the dev account holds 1000 STRK, so these are generous ceilings —
// the tx is charged devnet's actual (far smaller) usage. Pinning all six lets
// `starknet-rs` skip fee estimation entirely (see the module docs); the summed
// cap (~10 STRK) stays well under the signer's balance.
const L1_GAS: u64 = 1_000_000;
const L2_GAS: u64 = 100_000_000;
const L1_DATA_GAS: u64 = 1_000_000;
const GAS_PRICE_FRI: u128 = 100_000_000_000; // 1e11, 100× devnet's 1e9

/// Fund `address` on a single Starknet `chain`: mint the bundled tokens (or just
/// `token` when set). Called by the faucet coordinator, which has already
/// resolved this chain from the manifest.
pub fn fund_chain(
    chain: &ChainEntry,
    address: &str,
    amount: u64,
    token: Option<&str>,
) -> Result<()> {
    validate_address(address)?;
    // One current-thread runtime drives every async invoke on this chain; the
    // /mint cheat uses a plain blocking HTTP call and doesn't touch it.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building the async runtime for Starknet invokes")?;

    match token {
        // A single token was requested: fund just that one.
        Some(symbol) => {
            let token = find_token(chain, symbol)?;
            fund_one(&rt, chain, address, amount, token)?;
        }
        // Default: every bundled token (ETH/STRK plus the Cairo test tokens).
        None => {
            for token in &chain.tokens {
                fund_one(&rt, chain, address, amount, token)?;
            }
        }
    }
    Ok(())
}

/// Route one token to the right mechanism: the fee tokens go through devnet's
/// mint cheat, everything else through a signed `mint` invoke.
fn fund_one(
    rt: &tokio::runtime::Runtime,
    chain: &ChainEntry,
    address: &str,
    amount: u64,
    token: &Token,
) -> Result<()> {
    match token.symbol.as_str() {
        "ETH" => mint_native(chain, address, amount, token, "WEI"),
        "STRK" => mint_native(chain, address, amount, token, "FRI"),
        _ => mint_token(rt, chain, address, amount, token),
    }
}

/// Mint `amount` whole coins of a fee token via devnet's `POST /mint` cheat. The
/// endpoint mints in base units and is additive, so an existing balance is never
/// clobbered — matching the EVM faucet's additive ETH top-up.
fn mint_native(
    chain: &ChainEntry,
    address: &str,
    amount: u64,
    token: &Token,
    unit: &str,
) -> Result<()> {
    let pb = ui::spinner(format!(
        "{}: minting {amount} {}…",
        chain.name, token.symbol
    ));
    let result = (|| -> Result<()> {
        let raw = scaled_amount(amount, token.decimals)?;
        let (host, port) = devnet_endpoint(chain)?;
        let body = format!(r#"{{"address":"{address}","amount":{raw},"unit":"{unit}"}}"#);
        let resp = devnet_post(&host, port, "/mint", &body)?;
        if !resp.contains("new_balance") {
            bail!("devnet /mint did not confirm the deposit: {resp}");
        }
        Ok(())
    })();
    finish(&pb, chain, token.symbol.as_str(), amount, address, result)
}

/// Mint `amount` whole tokens (scaled by the token's decimals) by invoking the
/// baked test token's public `mint(recipient, u256)`, signed by the first dev
/// account purely to pay gas.
fn mint_token(
    rt: &tokio::runtime::Runtime,
    chain: &ChainEntry,
    address: &str,
    amount: u64,
    token: &Token,
) -> Result<()> {
    let pb = ui::spinner(format!(
        "{}: minting {amount} {}…",
        chain.name, token.symbol
    ));
    let result = rt.block_on(mint_token_invoke(chain, address, amount, token));
    finish(&pb, chain, token.symbol.as_str(), amount, address, result)
}

/// The async body of a test-token mint: build a single-owner account for the dev
/// signer and submit the `mint` invoke with fixed resource bounds.
async fn mint_token_invoke(
    chain: &ChainEntry,
    address: &str,
    amount: u64,
    token: &Token,
) -> Result<()> {
    let rpc = Url::parse(&chain.rpc).with_context(|| format!("invalid rpc url '{}'", chain.rpc))?;
    let provider = JsonRpcClient::new(HttpTransport::new(rpc));

    let dev = chain
        .accounts
        .first()
        .context("no funded dev account available to sign the mint")?;
    let key = Felt::from_hex(&dev.private_key).context("dev account private key is not a felt")?;
    let signer = LocalWallet::from(SigningKey::from_secret_scalar(key));
    let sender = Felt::from_hex(&dev.address).context("dev account address is not a felt")?;
    let chain_id = provider
        .chain_id()
        .await
        .context("querying the chain id from devnet")?;

    let mut account =
        SingleOwnerAccount::new(provider, signer, sender, chain_id, ExecutionEncoding::New);
    // Read nonces from the latest block. `starknet-rs` (RPC 0.9) renamed the
    // pending tag, which the RPC-0.8.1 devnet wouldn't accept; `latest` is
    // understood by both, and devnet mines a block per transaction (its default),
    // so back-to-back mints see each other's nonce without racing.
    account.set_block_id(BlockId::Tag(BlockTag::Latest));

    let recipient = Felt::from_hex(address).context("recipient is not a felt")?;
    let to = Felt::from_hex(&token.address)
        .with_context(|| format!("token address '{}' is not a felt", token.address))?;
    let raw = scaled_amount(amount, token.decimals)?;
    let call = Call {
        to,
        selector: get_selector_from_name("mint").expect("`mint` is a valid entrypoint name"),
        // u256 is passed as two felts (low, high); `raw` is a u128 so `high` is 0.
        calldata: vec![recipient, Felt::from(raw), Felt::ZERO],
    };

    account
        .execute_v3(vec![call])
        .l1_gas(L1_GAS)
        .l1_gas_price(GAS_PRICE_FRI)
        .l2_gas(L2_GAS)
        .l2_gas_price(GAS_PRICE_FRI)
        .l1_data_gas(L1_DATA_GAS)
        .l1_data_gas_price(GAS_PRICE_FRI)
        .send()
        .await
        .map(|_| ())
        .with_context(|| format!("submitting the {} mint invoke", token.symbol))
}

/// Finish a per-token spinner uniformly for both funding paths.
fn finish(
    pb: &indicatif::ProgressBar,
    chain: &ChainEntry,
    symbol: &str,
    amount: u64,
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
            ui::finish_err(pb, format!("{}: {symbol} mint failed", chain.name));
            Err(e)
        }
    }
}

/// `amount` whole units scaled to base units by the token's decimals.
fn scaled_amount(amount: u64, decimals: u8) -> Result<u128> {
    (amount as u128)
        .checked_mul(
            10u128
                .checked_pow(decimals as u32)
                .context("token has too many decimals")?,
        )
        .context("token amount too large")
}

/// devnet's cheat endpoints (`/mint`, `/account_balance`) live at the server
/// root, not under `/rpc`, so derive host+port from the manifest's rpc url.
fn devnet_endpoint(chain: &ChainEntry) -> Result<(String, u16)> {
    let url = Url::parse(&chain.rpc).with_context(|| format!("invalid rpc url '{}'", chain.rpc))?;
    let host = url.host_str().unwrap_or("127.0.0.1").to_string();
    let port = url
        .port_or_known_default()
        .context("rpc url has no port to reach devnet on")?;
    Ok((host, port))
}

/// Minimal, dependency-free HTTP `POST` to a devnet cheat endpoint, returning the
/// response body. Mirrors the readiness probe's raw-socket style.
fn devnet_post(host: &str, port: u16, path: &str, json: &str) -> Result<String> {
    let mut stream = TcpStream::connect(format!("{host}:{port}"))
        .with_context(|| format!("connecting to devnet at {host}:{port} — is the localnet up?"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        json.len(),
        json
    );
    stream.write_all(request.as_bytes())?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    let (head, body) = resp
        .split_once("\r\n\r\n")
        .context("devnet returned a malformed HTTP response")?;
    if !head.starts_with("HTTP/1.1 200") {
        bail!(
            "devnet {path} failed: {}",
            head.lines().next().unwrap_or("").trim()
        );
    }
    Ok(body.to_string())
}

/// Validate a Starknet address: `0x` + 1..=64 hex chars (felts are ≤ 252 bits).
/// The exact field-range check is left to `Felt::from_hex` at call time.
fn validate_address(address: &str) -> Result<()> {
    let ok = address.starts_with("0x")
        && (3..=66).contains(&address.len())
        && address[2..].chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        bail!("'{address}' is not a valid Starknet address (expected 0x + up to 64 hex chars)");
    }
    Ok(())
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

    fn starknet_chain() -> ChainEntry {
        ChainEntry {
            name: "starknet-1".into(),
            kind: "starknet".into(),
            rpc: "http://127.0.0.1:5050/rpc".into(),
            chain_id: "0x534e5f5345504f4c4941".into(),
            accounts: vec![Account {
                address: "0x064b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691"
                    .into(),
                private_key: "0x0000000000000000000000000000000071d7bb07b9a64f6f78ac4c816aff4da9"
                    .into(),
                balance: "1000 ETH & 1000 STRK".into(),
            }],
            tokens: vec![
                Token {
                    symbol: "USDC".into(),
                    name: "USD Coin".into(),
                    address: "0x040b582f9ba878be8e78a6ddc665dfdfd55a4deae9ceeb40115abcfa1f8df686"
                        .into(),
                    decimals: 6,
                },
                Token {
                    symbol: "ETH".into(),
                    name: "Ether".into(),
                    address: "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7"
                        .into(),
                    decimals: 18,
                },
            ],
            contracts: vec![],
            fork: None,
            explorer: None,
        }
    }

    #[test]
    fn validate_address_accepts_felts_and_rejects_junk() {
        assert!(validate_address("0x1234").is_ok());
        assert!(
            validate_address("0x064b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691")
                .is_ok()
        );
        assert!(validate_address("0x").is_err()); // no digits
        assert!(validate_address("1234").is_err()); // no 0x
        assert!(validate_address("0xghij").is_err()); // non-hex
        assert!(validate_address(&format!("0x{}", "1".repeat(65))).is_err()); // too long
    }

    #[test]
    fn scaled_amount_scales_by_decimals() {
        assert_eq!(scaled_amount(100, 6).unwrap(), 100_000_000);
        assert_eq!(scaled_amount(1, 18).unwrap(), 1_000_000_000_000_000_000);
        assert_eq!(scaled_amount(0, 8).unwrap(), 0);
    }

    #[test]
    fn devnet_endpoint_strips_the_rpc_path() {
        let (host, port) = devnet_endpoint(&starknet_chain()).unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 5050);
    }

    #[test]
    fn find_token_is_case_insensitive_and_reports_unknowns() {
        let chain = starknet_chain();
        assert_eq!(find_token(&chain, "usdc").unwrap().symbol, "USDC");
        assert_eq!(find_token(&chain, "ETH").unwrap().symbol, "ETH");
        let err = find_token(&chain, "WBTC").unwrap_err();
        assert!(err.to_string().contains("not deployed"), "{err}");
    }

    // ---- docker-backed end-to-end run against a live starknet-devnet ----
    //
    // Boots a real chain and funds a fresh (non-dev) address, then reads the
    // balances straight off the node: ETH/STRK through devnet's balance endpoint
    // (minted via the /mint cheat) and the four Cairo test tokens through
    // `balance_of` (minted via signed invokes). This is the proof the whole
    // signed-invoke path works against the pinned RPC-0.8.1 devnet. Self-skips
    // without Docker.

    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;
    use crate::testkit::{Localnet, docker_available};
    use starknet::core::types::FunctionCall;

    /// A dedicated port, away from the other e2e ports, for parallel runs.
    const SN_FAUCET_PORT: u16 = 5152;
    /// A non-dev recipient — starts at zero for every token.
    const RECIPIENT: &str = "0x00000000000000000000000000000000000000000000000000000000feed1234";
    const ONE_E18: u128 = 1_000_000_000_000_000_000;

    /// Minimal HTTP GET → response body.
    fn http_get_body(port: u16, path: &str) -> String {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
    }

    /// devnet fee-token balance (ETH/STRK) via its `/account_balance` endpoint.
    fn native_balance(port: u16, address: &str, unit: &str) -> u128 {
        let body = http_get_body(
            port,
            &format!("/account_balance?address={address}&unit={unit}"),
        );
        let v: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|e| panic!("parsing '{body}': {e}"));
        v["amount"].as_str().unwrap().parse().unwrap()
    }

    /// ERC-20 `balance_of(holder)` on `token`, read via `starknet_call`.
    fn erc20_balance(rpc: &str, token: &str, holder: &str) -> u128 {
        let provider = JsonRpcClient::new(HttpTransport::new(Url::parse(rpc).unwrap()));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let res = rt
            .block_on(provider.call(
                FunctionCall {
                    contract_address: Felt::from_hex(token).unwrap(),
                    entry_point_selector: get_selector_from_name("balance_of").unwrap(),
                    calldata: vec![Felt::from_hex(holder).unwrap()],
                },
                BlockId::Tag(BlockTag::Latest),
            ))
            .unwrap_or_else(|e| panic!("balance_of call on {token} failed: {e}"));
        // u256 low word; the test amounts all fit in a u128.
        u128::try_from(res[0]).unwrap()
    }

    #[test]
    fn faucet_mints_native_and_test_tokens_on_a_live_chain() {
        if !docker_available() {
            eprintln!("skipping starknet faucet e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_starknet("t-sn-faucet", SN_FAUCET_PORT);
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];
        let addr = |sym: &str| {
            chain
                .tokens
                .iter()
                .find(|t| t.symbol == sym)
                .unwrap_or_else(|| panic!("{sym} missing from manifest"))
                .address
                .clone()
        };
        let rpc = chain.rpc.clone();

        // 1) Default fund: native ETH+STRK via the mint cheat, USDC/WBTC/FEE/REB
        //    via signed invokes — one shot. Success alone proves the invoke path.
        crate::faucet::run_in(net.base(), net.project(), net.chain(), RECIPIENT, 5, None)
            .expect("funding native + all tokens should succeed");
        assert_eq!(
            native_balance(SN_FAUCET_PORT, RECIPIENT, "WEI"),
            5 * ONE_E18
        );
        assert_eq!(
            native_balance(SN_FAUCET_PORT, RECIPIENT, "FRI"),
            5 * ONE_E18
        );
        assert_eq!(erc20_balance(&rpc, &addr("USDC"), RECIPIENT), 5_000_000); // 5 @ 6dp
        assert_eq!(erc20_balance(&rpc, &addr("WBTC"), RECIPIENT), 500_000_000); // 5 @ 8dp
        assert_eq!(erc20_balance(&rpc, &addr("FEE"), RECIPIENT), 5 * ONE_E18); // no fee on mint
        assert_eq!(erc20_balance(&rpc, &addr("REB"), RECIPIENT), 5 * ONE_E18); // factor 1.0

        // 2) Single-token top-up mints only USDC and leaves the rest untouched.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            RECIPIENT,
            3,
            Some("USDC"),
        )
        .expect("single-token funding should succeed");
        assert_eq!(erc20_balance(&rpc, &addr("USDC"), RECIPIENT), 8_000_000); // 5 + 3 @ 6dp
        assert_eq!(
            native_balance(SN_FAUCET_PORT, RECIPIENT, "WEI"),
            5 * ONE_E18
        );

        // 3) A second full fund is additive across every token.
        crate::faucet::run_in(net.base(), net.project(), net.chain(), RECIPIENT, 2, None)
            .expect("second full funding should succeed");
        assert_eq!(
            native_balance(SN_FAUCET_PORT, RECIPIENT, "WEI"),
            7 * ONE_E18
        );
        assert_eq!(erc20_balance(&rpc, &addr("USDC"), RECIPIENT), 10_000_000); // 8 + 2 @ 6dp
        assert_eq!(erc20_balance(&rpc, &addr("WBTC"), RECIPIENT), 700_000_000); // 7 @ 8dp
    }
}

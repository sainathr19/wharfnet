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
//! The client (`starknet-rust`) and the pinned devnet both speak JSON-RPC 0.10.
//! The invoke pins **all six** resource bounds so `execute_v3` skips the
//! `estimate_fee` round-trip entirely and goes straight to `get_nonce` +
//! `add_invoke_transaction` — devnet's gas is nominal, so fixed generous bounds
//! are simpler and more robust than trusting a localnet fee estimate.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use starknet_rust::accounts::{Account, ConnectedAccount, ExecutionEncoding, SingleOwnerAccount};
use starknet_rust::core::types::{BlockId, BlockTag, Call, Felt};
use starknet_rust::core::utils::get_selector_from_name;
use starknet_rust::providers::jsonrpc::HttpTransport;
use starknet_rust::providers::{JsonRpcClient, Provider, Url};
use starknet_rust::signers::{LocalWallet, SigningKey};

use super::devnet;
use crate::runtime::amount::to_base_units;
use crate::runtime::manifest::{ChainEntry, Token};
use crate::runtime::ui;

// Resource-bound maximums for the mint invoke. devnet's gas prices are 1e9
// fri/gas and the dev account holds 1000 STRK, so these are generous ceilings —
// the tx is charged devnet's actual (far smaller) usage. Pinning all six lets
// `execute_v3` skip fee estimation entirely (see the module docs); the summed
// cap (~10 STRK) stays well under the signer's balance.
const L1_GAS: u64 = 1_000_000;
const L2_GAS: u64 = 100_000_000;
const L1_DATA_GAS: u64 = 1_000_000;
const GAS_PRICE_FRI: u128 = 100_000_000_000; // 1e11, 100× devnet's 1e9

/// The signing account for test-token mints — the first predeployed dev account,
/// built once per funding run and reused across every token.
type DevAccount = SingleOwnerAccount<JsonRpcClient<HttpTransport>, LocalWallet>;

/// Fund `address` on a single Starknet `chain`: mint the bundled tokens (or just
/// `token` when set). Called by the faucet coordinator, which has already
/// resolved this chain from the manifest.
pub fn fund_chain(
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    token: Option<&str>,
    raw: bool,
) -> Result<()> {
    validate_address(address)?;
    let tokens: Vec<&Token> = match token {
        // A single token was requested: fund just that one.
        Some(symbol) => vec![find_token(chain, symbol)?],
        // Default: every bundled token (ETH/STRK plus the Cairo test tokens).
        None => chain.tokens.iter().collect(),
    };

    // The runtime and signing account are built once, lazily: only a Cairo-token
    // mint needs them — the ETH/STRK cheat is a plain blocking HTTP call.
    let mut ctx: Option<(tokio::runtime::Runtime, DevAccount)> = None;
    for token in tokens {
        match token.symbol.as_str() {
            "ETH" => mint_native(chain, address, amount, raw, token, "WEI")?,
            "STRK" => mint_native(chain, address, amount, raw, token, "FRI")?,
            _ => {
                if ctx.is_none() {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .context("building the async runtime for Starknet invokes")?;
                    let account = rt.block_on(build_dev_account(chain))?;
                    ctx = Some((rt, account));
                }
                let (rt, account) = ctx.as_ref().expect("ctx was just built");
                let pb = ui::spinner(format!(
                    "{}: minting {amount} {}…",
                    chain.name, token.symbol
                ));
                let result = rt.block_on(mint_with_account(account, address, amount, raw, token));
                finish(&pb, chain, token.symbol.as_str(), amount, address, result)?;
            }
        }
    }
    Ok(())
}

/// Mint `amount` whole coins of a fee token via devnet's `devnet_mint` JSON-RPC
/// cheat. It mints in base units and is additive, so an existing balance is never
/// clobbered — matching the EVM faucet's additive ETH top-up.
fn mint_native(
    chain: &ChainEntry,
    address: &str,
    amount: &str,
    raw: bool,
    token: &Token,
    unit: &str,
) -> Result<()> {
    let pb = ui::spinner(format!(
        "{}: minting {amount} {}…",
        chain.name, token.symbol
    ));
    let result = (|| -> Result<()> {
        let base_units = to_base_units(amount, token.decimals as u32, raw)?;
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"devnet_mint","params":{{"address":"{address}","amount":{base_units},"unit":"{unit}"}}}}"#
        );
        let resp = devnet::post(chain, &body)?;
        if !resp.contains("new_balance") {
            bail!("devnet_mint did not confirm the deposit: {resp}");
        }
        Ok(())
    })();
    finish(&pb, chain, token.symbol.as_str(), amount, address, result)
}

/// Build the dev signing account once per funding run: parse the RPC, load the
/// first predeployed dev account's key, and fetch the chain id. Reused across
/// every test-token mint so those steps aren't repeated per token.
async fn build_dev_account(chain: &ChainEntry) -> Result<DevAccount> {
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
    // Read nonces from the latest block. devnet mines a block per transaction (its
    // default), so back-to-back mints in one funding run see each other's nonce
    // without racing — no need for the pending tag.
    account.set_block_id(BlockId::Tag(BlockTag::Latest));
    Ok(account)
}

/// Mint `amount` whole tokens (scaled by the token's decimals) by invoking the
/// baked test token's public `mint(recipient, u256)` through the shared dev
/// `account`, which just pays gas — `mint` is public, so the recipient needs no key.
async fn mint_with_account(
    account: &DevAccount,
    address: &str,
    amount: &str,
    raw: bool,
    token: &Token,
) -> Result<()> {
    let recipient = Felt::from_hex(address).context("recipient is not a felt")?;
    let to = Felt::from_hex(&token.address)
        .with_context(|| format!("token address '{}' is not a felt", token.address))?;
    let base_units = to_base_units(amount, token.decimals as u32, raw)?;
    let call = Call {
        to,
        selector: get_selector_from_name("mint").expect("`mint` is a valid entrypoint name"),
        // u256 is passed as two felts (low, high); the amount is a u128 so `high` is 0.
        calldata: vec![recipient, Felt::from(base_units), Felt::ZERO],
    };

    let tx_hash = account
        .execute_v3(vec![call])
        .l1_gas(L1_GAS)
        .l1_gas_price(GAS_PRICE_FRI)
        .l2_gas(L2_GAS)
        .l2_gas_price(GAS_PRICE_FRI)
        .l1_data_gas(L1_DATA_GAS)
        .l1_data_gas_price(GAS_PRICE_FRI)
        .send()
        .await
        .with_context(|| format!("submitting the {} mint invoke", token.symbol))?
        .transaction_hash;

    // `send` returns once the tx is accepted, not once it executes — and a
    // reverted tx is still accepted. devnet mines a block per tx, so the receipt
    // lands almost immediately; poll briefly, then confirm the mint didn't revert.
    let provider = account.provider();
    let mut receipt = None;
    for _ in 0..20 {
        if let Ok(r) = provider.get_transaction_receipt(tx_hash).await {
            receipt = Some(r);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let receipt =
        receipt.with_context(|| format!("the {} mint receipt never landed", token.symbol))?;
    if let Some(reason) = receipt.receipt.execution_result().revert_reason() {
        bail!("the {} mint reverted: {reason}", token.symbol);
    }
    Ok(())
}

/// Finish a per-token spinner uniformly for both funding paths.
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
            ui::finish_err(pb, format!("{}: {symbol} mint failed", chain.name));
            Err(e)
        }
    }
}

/// Validate a Starknet address: `0x` + 1..=64 hex chars (felts are ≤ 252 bits).
/// The exact field-range check is left to `Felt::from_hex` at call time. Shared
/// with [chain control](super::control), which pre-flights the same format.
pub(crate) fn validate_address(address: &str) -> Result<()> {
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
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

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
    // balances straight off the node: ETH/STRK through the `devnet_getAccountBalance`
    // cheat (minted via `devnet_mint`) and the four Cairo test tokens through
    // `balance_of` (minted via signed invokes). This is the proof the whole
    // signed-invoke path works against the RPC-0.10 devnet. Self-skips without
    // Docker.

    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;
    use crate::testkit::{Localnet, docker_available};
    use starknet_rust::core::types::FunctionCall;

    /// A dedicated port, away from the other e2e ports, for parallel runs.
    const SN_FAUCET_PORT: u16 = 5152;
    /// A non-dev recipient — starts at zero for every token.
    const RECIPIENT: &str = "0x00000000000000000000000000000000000000000000000000000000feed1234";
    const ONE_E18: u128 = 1_000_000_000_000_000_000;

    /// Minimal JSON-RPC POST to devnet's `/rpc` → response body.
    fn rpc_post(port: u16, body: &str) -> String {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let req = format!(
            "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
    }

    /// devnet fee-token balance (ETH/STRK) via the `devnet_getAccountBalance` cheat.
    fn native_balance(port: u16, address: &str, unit: &str) -> u128 {
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"devnet_getAccountBalance","params":{{"address":"{address}","unit":"{unit}"}}}}"#
        );
        let resp = rpc_post(port, &body);
        let v: serde_json::Value =
            serde_json::from_str(&resp).unwrap_or_else(|e| panic!("parsing '{resp}': {e}"));
        v["result"]["amount"].as_str().unwrap().parse().unwrap()
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
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            RECIPIENT,
            "5",
            None,
            false,
        )
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
            "3",
            Some("USDC"),
            false,
        )
        .expect("single-token funding should succeed");
        assert_eq!(erc20_balance(&rpc, &addr("USDC"), RECIPIENT), 8_000_000); // 5 + 3 @ 6dp
        assert_eq!(
            native_balance(SN_FAUCET_PORT, RECIPIENT, "WEI"),
            5 * ONE_E18
        );

        // 3) A second full fund is additive across every token.
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            RECIPIENT,
            "2",
            None,
            false,
        )
        .expect("second full funding should succeed");
        assert_eq!(
            native_balance(SN_FAUCET_PORT, RECIPIENT, "WEI"),
            7 * ONE_E18
        );
        assert_eq!(erc20_balance(&rpc, &addr("USDC"), RECIPIENT), 10_000_000); // 8 + 2 @ 6dp
        assert_eq!(erc20_balance(&rpc, &addr("WBTC"), RECIPIENT), 700_000_000); // 7 @ 8dp
    }

    /// A dedicated port for the persistence cycle, away from the other e2e ports.
    const SN_PERSIST_PORT: u16 = 5153;

    /// End-to-end persistence: a faucet mint survives `down` → `up --resume`
    /// (devnet re-executes the accumulated session replay), and `up --reset`
    /// discards it. This is the proof `--resume`/`--reset` work on Starknet.
    /// Runs several boot cycles, so it's the heaviest test; self-skips w/o Docker.
    #[test]
    fn faucet_mints_survive_down_and_up_resume() {
        if !docker_available() {
            eprintln!("skipping starknet persistence e2e: docker unavailable");
            return;
        }
        use crate::runtime::orchestrator::{self, UpMode};

        let dir = tempfile::TempDir::new_in(".").expect("temp dir under crate root");
        let base = dir.path();
        let project = "wharfnet-e2e-sn-persist";
        let chain = "sn-persist";
        let config = base.join("wharfnet.toml");
        std::fs::write(
            &config,
            format!(
                "[[chains]]\nname = \"{chain}\"\nkind = \"starknet\"\nport = {SN_PERSIST_PORT}\n"
            ),
        )
        .unwrap();

        // Tear the containers down even if an assertion below panics. Declared
        // after `dir` so it drops first — down_in still sees the compose file.
        struct Teardown<'a>(&'a std::path::Path, &'a str);
        impl Drop for Teardown<'_> {
            fn drop(&mut self) {
                let _ = crate::runtime::orchestrator::down_in(self.0, self.1);
            }
        }
        let _guard = Teardown(base, project);

        let rpc = format!("http://127.0.0.1:{SN_PERSIST_PORT}/rpc");
        let usdc = "0x040b582f9ba878be8e78a6ddc665dfdfd55a4deae9ceeb40115abcfa1f8df686";

        // 1) First `up --resume`: seeds the per-chain session from the baked
        //    tokens, then mint USDC — the invoke lands in the session replay.
        orchestrator::up_in(base, project, UpMode::Resume, false, Some(&config))
            .expect("first up --resume should boot");
        crate::faucet::run_in(base, project, chain, RECIPIENT, "7", Some("USDC"), false)
            .expect("minting USDC should succeed");
        assert_eq!(erc20_balance(&rpc, usdc, RECIPIENT), 7_000_000);

        // 2) Tear down (the bind-mounted session replay survives on the host) and
        //    resume: devnet re-executes the replay, restoring the mint.
        orchestrator::down_in(base, project).expect("down should succeed");
        orchestrator::up_in(base, project, UpMode::Resume, false, Some(&config))
            .expect("second up --resume should boot");
        assert_eq!(
            erc20_balance(&rpc, usdc, RECIPIENT),
            7_000_000,
            "the USDC mint must survive down → up --resume"
        );

        // 3) `up --reset` discards the session and boots fresh — the mint is gone.
        orchestrator::down_in(base, project).expect("down before reset should succeed");
        orchestrator::up_in(base, project, UpMode::Reset, false, Some(&config))
            .expect("up --reset should boot");
        assert_eq!(
            erc20_balance(&rpc, usdc, RECIPIENT),
            0,
            "up --reset must discard the persisted mint"
        );
    }
}

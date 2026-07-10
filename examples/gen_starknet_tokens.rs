//! Deploy the Cairo test tokens onto a running `starknet-devnet` and print their
//! deterministic addresses. Driven by `scripts/gen-starknet-token-state.sh`,
//! which boots a throwaway devnet, runs this to declare/deploy/seed the tokens,
//! then dumps devnet's replay log to `src/resources/state/starknet-tokens.json`.
//!
//! This is the RPC-0.10 replacement for the old starkli-based deploy step: it
//! uses the same `starknet-rust` client the runtime faucet does, so there's no
//! external CLI (starkli/sncast) to keep in sync with the devnet's RPC version.
//!
//! Usage: `cargo run --example gen_starknet_tokens -- <rpc-url> <scarb-target-dir>`
//!
//! Tokens deploy from seed-0 account 0 via the (legacy) UDC with fixed salts and
//! `unique = false`, so the addresses are deterministic given the pinned Cairo
//! toolchain — deployer-independent, hence stable across devnet versions.

use std::sync::Arc;

use anyhow::{Context, Result};
use starknet_rust::accounts::{Account, ExecutionEncoding, SingleOwnerAccount};
use starknet_rust::contract::{ContractFactory, UdcSelector};
use starknet_rust::core::types::contract::{CompiledClass, SierraClass};
use starknet_rust::core::types::{BlockId, BlockTag, Call, Felt};
use starknet_rust::core::utils::{cairo_short_string_to_felt, get_selector_from_name};
use starknet_rust::providers::jsonrpc::HttpTransport;
use starknet_rust::providers::{JsonRpcClient, Provider, Url};
use starknet_rust::signers::{LocalWallet, SigningKey};

/// seed-0 account 0 — the deployer/signer. Must match `StarknetEngine::accounts`.
const DEPLOYER: &str = "0x064b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691";
const DEPLOYER_PK: &str = "0x0000000000000000000000000000000071d7bb07b9a64f6f78ac4c816aff4da9";

/// The three seed-0 dev accounts, each seeded with a starting balance of every token.
const DEV_ACCOUNTS: [&str; 3] = [
    "0x064b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691",
    "0x078662e7352d062084b0010068b99288486c2d8b914f6e2a55ce945f8792c8b1",
    "0x049dfb8ce986e21d354ac93ea65e6a11f639c1934ea253e5ff14ca62eca0f38e",
];

type DevnetAccount = SingleOwnerAccount<JsonRpcClient<HttpTransport>, LocalWallet>;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let rpc = args
        .next()
        .context("usage: gen_starknet_tokens <rpc-url> <scarb-target-dir>")?;
    let target_dir = args
        .next()
        .context("usage: gen_starknet_tokens <rpc-url> <scarb-target-dir>")?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run(&rpc, &target_dir))
}

async fn run(rpc: &str, target_dir: &str) -> Result<()> {
    let provider = JsonRpcClient::new(HttpTransport::new(Url::parse(rpc)?));
    let chain_id = provider.chain_id().await.context("querying chain id")?;
    let signer = LocalWallet::from(SigningKey::from_secret_scalar(Felt::from_hex(DEPLOYER_PK)?));
    let mut account = SingleOwnerAccount::new(
        provider,
        signer,
        Felt::from_hex(DEPLOYER)?,
        chain_id,
        ExecutionEncoding::New,
    );
    // devnet mines a block per tx, so `latest` always reflects the previous tx's
    // nonce — these declares/deploys/mints run strictly sequentially below.
    account.set_block_id(BlockId::Tag(BlockTag::Latest));

    // Declare each Cairo class once, capturing its (Sierra) class hash.
    let test_token = declare(&account, target_dir, "wharfnet_tokens_TestToken").await?;
    let fee_token = declare(&account, target_dir, "wharfnet_tokens_FeeToken").await?;
    let rebasing = declare(&account, target_dir, "wharfnet_tokens_RebasingToken").await?;

    // Deploy the four tokens at fixed salts. TestToken takes (name, symbol,
    // decimals) as short-string felts; FeeToken/RebasingToken take no ctor args.
    let usdc = deploy(
        &account,
        test_token,
        0x1,
        vec![short("USD Coin")?, short("USDC")?, Felt::from(6u8)],
    )
    .await?;
    let wbtc = deploy(
        &account,
        test_token,
        0x2,
        vec![short("Wrapped BTC")?, short("WBTC")?, Felt::from(8u8)],
    )
    .await?;
    let fee = deploy(&account, fee_token, 0x3, vec![]).await?;
    let reb = deploy(&account, rebasing, 0x4, vec![]).await?;

    // Seed every dev account with 1,000,000 whole tokens of each (base units).
    seed(&account, usdc, 1_000_000_000_000).await?; // 1e6 @ 6dp
    seed(&account, wbtc, 100_000_000_000_000).await?; // 1e6 @ 8dp
    seed(&account, fee, 1_000_000_000_000_000_000_000_000).await?; // 1e6 @ 18dp
    seed(&account, reb, 1_000_000_000_000_000_000_000_000).await?; // 1e6 @ 18dp

    println!("USDC {usdc:#064x}");
    println!("WBTC {wbtc:#064x}");
    println!("FEE  {fee:#064x}");
    println!("REB  {reb:#064x}");
    Ok(())
}

/// Declare the class compiled to `<target_dir>/<name>.contract_class.json` (+ the
/// `.compiled_contract_class.json` CASM) and return its class hash.
async fn declare(account: &DevnetAccount, target_dir: &str, name: &str) -> Result<Felt> {
    let sierra_path = format!("{target_dir}/{name}.contract_class.json");
    let casm_path = format!("{target_dir}/{name}.compiled_contract_class.json");
    let sierra: SierraClass = serde_json::from_reader(
        std::fs::File::open(&sierra_path).with_context(|| format!("opening {sierra_path}"))?,
    )
    .with_context(|| format!("parsing {sierra_path}"))?;
    let casm: CompiledClass = serde_json::from_reader(
        std::fs::File::open(&casm_path).with_context(|| format!("opening {casm_path}"))?,
    )
    .with_context(|| format!("parsing {casm_path}"))?;

    let class_hash = sierra.class_hash().context("computing sierra class hash")?;
    let compiled_class_hash = casm.class_hash().context("computing compiled class hash")?;
    account
        .declare_v3(Arc::new(sierra.flatten()?), compiled_class_hash)
        .send()
        .await
        .with_context(|| format!("declaring {name}"))?;
    Ok(class_hash)
}

/// Deploy `class_hash` via the legacy UDC with a fixed `salt` and `unique = false`
/// (deployer-independent → deterministic address) and return that address.
async fn deploy(
    account: &DevnetAccount,
    class_hash: Felt,
    salt: u64,
    constructor_calldata: Vec<Felt>,
) -> Result<Felt> {
    // Legacy UDC (0x041a78…) — deployer-independent addressing that matches the
    // hardcoded token addresses; devnet predeploys it alongside the newer one.
    let factory = ContractFactory::new_with_udc(class_hash, account, UdcSelector::Legacy);
    let deployment = factory.deploy_v3(constructor_calldata, Felt::from(salt), false);
    let address = deployment.deployed_address();
    deployment
        .send()
        .await
        .with_context(|| format!("deploying class {class_hash:#x} at salt {salt:#x}"))?;
    Ok(address)
}

/// Mint 1,000,000-worth (in base units) of `token` to each dev account via its
/// public `mint(recipient, u256)`.
async fn seed(account: &DevnetAccount, token: Felt, amount: u128) -> Result<()> {
    let selector = get_selector_from_name("mint").expect("`mint` is a valid entrypoint");
    for holder in DEV_ACCOUNTS {
        account
            .execute_v3(vec![Call {
                to: token,
                selector,
                calldata: vec![Felt::from_hex(holder)?, Felt::from(amount), Felt::ZERO],
            }])
            .send()
            .await
            .with_context(|| format!("minting {token:#x} to {holder}"))?;
    }
    Ok(())
}

/// Encode a ≤31-char ASCII string as a Cairo short-string felt (`felt252`).
fn short(s: &str) -> Result<Felt> {
    cairo_short_string_to_felt(s).with_context(|| format!("encoding short string '{s}'"))
}

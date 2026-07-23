//! The bitcoind/litecoind-backed UTXO engine: one [`Engine`] impl for both
//! Bitcoin and Litecoin.
//!
//! Litecoin is a Bitcoin fork with an identical JSON-RPC, so a single engine
//! parameterized by a [`Coin`] serves both — only the image, ports, and native
//! symbol differ. Each chain runs its daemon in **regtest**: a standalone network
//! where blocks are produced on demand (`generatetoaddress`) rather than on a
//! timer. At boot the engine creates a wallet and mines 101 blocks to it, so one
//! coinbase matures (50 coins) and the faucet has a funded source to spend from —
//! the UTXO analogue of Anvil's pre-funded dev accounts.
//!
//! The compose service YAML lives in `src/resources/docker/services/utxo.yml` and
//! is embedded at compile time — edit that template, not a Rust string.

use std::sync::OnceLock;

use anyhow::{Context, Result};
use serde_json::json;

use super::rpc::{self, RPC_PASS, RPC_USER, WALLET};
use crate::runtime::engine::{Engine, ExplorerTarget, HealthProbe, StateMode};
use crate::runtime::manifest::{Account, ChainEntry};

/// Static parameters that distinguish the two otherwise-identical daemons.
#[derive(Clone, Copy)]
pub struct Coin {
    /// Chain kind / manifest label: `"bitcoin"` or `"litecoin"`.
    pub kind: &'static str,
    /// Pinned daemon image (reproducible boots, like the other engines' images).
    pub image: &'static str,
    /// The RPC port inside the container (also the published host default). Set
    /// to each coin's conventional regtest port for familiarity.
    pub rpc_port: u16,
    /// Native coin symbol, for manifest balances and the faucet's default token.
    pub symbol: &'static str,
    /// The env var the image entrypoint reads to place the datadir. Both images
    /// append their own `-datadir=$..._DATA` after our flags, so a `-datadir`
    /// command arg is silently overridden — we must set this var to `/data`
    /// instead. Differs per coin (`BITCOIN_DATA` vs `LITECOIN_DATA`).
    pub datadir_env: &'static str,
}

/// Bitcoin Core in regtest. `bitcoin/bitcoin` is the official image; `:29` speaks
/// Bitcoin Core 29 and the standard JSON-RPC.
pub const BITCOIN: Coin = Coin {
    kind: "bitcoin",
    image: "bitcoin/bitcoin:29",
    rpc_port: 18443,
    symbol: "BTC",
    datadir_env: "BITCOIN_DATA",
};

/// Litecoin Core in regtest. `:0.21` is Litecoin Core v0.21.2.2 — a Bitcoin-0.21
/// fork, so the RPC surface wharfnet uses is identical to Bitcoin's.
pub const LITECOIN: Coin = Coin {
    kind: "litecoin",
    image: "uphold/litecoin-core:0.21",
    rpc_port: 19443,
    symbol: "LTC",
    datadir_env: "LITECOIN_DATA",
};

/// Blocks mined at boot. Coinbase maturity in regtest is 100, so 101 blocks leaves
/// exactly one mature coinbase (50 coins) spendable by the faucet.
const BOOT_BLOCKS: u64 = 101;

/// Compose service template shared by both coins.
const UTXO_SERVICE_TEMPLATE: &str = include_str!("../resources/docker/services/utxo.yml");

/// A bitcoind/litecoind regtest chain.
pub struct UtxoEngine {
    coin: Coin,
    name: String,
    host_port: u16,
    /// The wallet address funded at boot, recorded by `post_boot` for the
    /// manifest. `OnceLock` because the `Engine` trait hands out `&self` — the
    /// address isn't known until the node is live and mined.
    funded: OnceLock<Account>,
}

impl UtxoEngine {
    pub fn new(coin: Coin, name: &str, host_port: u16) -> Self {
        UtxoEngine {
            coin,
            name: name.to_string(),
            host_port,
            funded: OnceLock::new(),
        }
    }

    /// The manifest RPC url, with the dev credentials embedded so a tool reading
    /// the manifest can connect straight away (bitcoind RPC always needs auth).
    fn rpc_url(&self) -> String {
        format!("http://{RPC_USER}:{RPC_PASS}@127.0.0.1:{}", self.host_port)
    }

    /// The per-chain datadir, relative to the state dir. Mounted into the
    /// container (as `/data`) in persistent mode so the chain survives restarts;
    /// this is the path `--reset` wipes and `--resume` preserves.
    fn datadir_rel(&self) -> String {
        format!("state/utxo-{}", self.name)
    }
}

impl Engine for UtxoEngine {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn host_port(&self) -> u16 {
        self.host_port
    }

    fn compose_service(&self, mode: StateMode) -> String {
        // Persistent: bind-mount a per-chain datadir and place the daemon's data
        // there so state survives `down` → `up --resume`. The datadir must be set
        // via the image's env var (`$..._DATA`), not a `-datadir` arg: both
        // entrypoints append their own `-datadir=$..._DATA` after our flags, which
        // would override the arg and send data to the image's anonymous volume
        // (wiped by `down -v`). Ephemeral: no volume, datadir stays in the
        // container, so every boot is fresh.
        let (environment, volumes) = match mode {
            StateMode::Persistent => (
                format!(
                    "\n    environment:\n      - \"{}=/data\"",
                    self.coin.datadir_env
                ),
                format!("\n    volumes:\n      - \"./{}:/data\"", self.datadir_rel()),
            ),
            StateMode::Ephemeral => (String::new(), String::new()),
        };
        UTXO_SERVICE_TEMPLATE
            .replace("{{NAME}}", &self.name)
            .replace("{{IMAGE}}", self.coin.image)
            .replace("{{PORT}}", &self.coin.rpc_port.to_string())
            .replace("{{HOST_PORT}}", &self.host_port.to_string())
            .replace("{{ENVIRONMENT}}", &environment)
            .replace("{{VOLUMES}}", &volumes)
    }

    fn manifest_entry(&self) -> ChainEntry {
        ChainEntry {
            name: self.name.clone(),
            kind: self.coin.kind.to_string(),
            rpc: self.rpc_url(),
            ws: None,
            // regtest has no numeric chain id; the network name is the identifier.
            chain_id: "regtest".to_string(),
            // Populated by post_boot once the boot wallet is mined; empty before
            // that (e.g. in unit tests that skip the docker boot).
            accounts: self.funded.get().cloned().into_iter().collect(),
            // UTXO chains carry no baked test tokens or infra contracts.
            tokens: Vec::new(),
            contracts: Vec::new(),
            fork: None,
            explorer: None,
        }
    }

    fn health_probe(&self) -> HealthProbe {
        // bitcoind/litecoind answer `getblockchaininfo` only with credentials, so
        // the probe carries the fixed dev Basic-auth header.
        HealthProbe::JsonRpcAuth {
            method: "getblockchaininfo",
            authorization: rpc::RPC_AUTH_HEADER,
        }
    }

    fn explorer_target(&self) -> Option<ExplorerTarget> {
        // Both coins get a btc-rpc-explorer-family explorer: btc-rpc-explorer for
        // Bitcoin, its ltc-rpc-explorer fork for Litecoin (the orchestrator picks
        // the image and env-var flavor from `coin`). Unlike Otterscan, it makes
        // its RPC calls server-side, so it reaches the daemon over the docker
        // network via the chain's service name + internal RPC port.
        Some(ExplorerTarget::UtxoRpc {
            coin: self.coin.kind,
            chain_name: self.name.clone(),
            rpc_service: self.name.clone(),
            rpc_port: self.coin.rpc_port,
            rpc_user: RPC_USER.to_string(),
            rpc_pass: RPC_PASS.to_string(),
        })
    }

    fn session_paths(&self) -> Vec<String> {
        // The whole datadir is the resumable session; `--reset` wipes it and
        // `--resume` keeps it. Only materializes on the host in persistent mode.
        vec![self.datadir_rel()]
    }

    fn post_boot(&self) -> Result<()> {
        // Once the RPC is live, ensure the wharfnet wallet exists and the chain
        // holds a matured coinbase, so the faucet has a funded source (like
        // Anvil's pre-funded accounts). Idempotent: a resumed datadir already has
        // the wallet and blocks, so we load-not-create and mine only the shortfall.
        let chain = self.manifest_entry();
        // createwallet fails if it already exists (resumed datadir); fall back to
        // loadwallet so the wallet is available either way.
        if rpc::call(&chain, None, "createwallet", json!([WALLET])).is_err() {
            rpc::call(&chain, None, "loadwallet", json!([WALLET]))
                .context("boot wallet exists but could not be loaded")?;
        }
        let address = rpc::call(&chain, Some(WALLET), "getnewaddress", json!([]))?
            .as_str()
            .context("getnewaddress did not return an address")?
            .to_string();
        let height = rpc::call(&chain, None, "getblockcount", json!([]))?
            .as_u64()
            .unwrap_or(0);
        if height < BOOT_BLOCKS {
            rpc::call(
                &chain,
                Some(WALLET),
                "generatetoaddress",
                json!([BOOT_BLOCKS - height, address]),
            )?;
        }
        let balance = rpc::call(&chain, Some(WALLET), "getbalance", json!([]))?
            .as_f64()
            .unwrap_or(0.0);
        // Trailing ".0" trimmed: 50.0 → "50". Regtest maturity yields whole coins.
        let balance = format!("{balance} {}", self.coin.symbol);
        let _ = self.funded.set(Account {
            address,
            // The key lives in the node wallet; regtest descriptor wallets don't
            // export it, so record how to spend rather than a raw secret.
            private_key: format!("(spendable via node wallet '{WALLET}')"),
            balance,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_coin_fields() {
        let e = UtxoEngine::new(BITCOIN, "bitcoin-1", 18443);
        assert_eq!(e.name(), "bitcoin-1");
        assert_eq!(e.host_port(), 18443);
        assert_eq!(e.coin.symbol, "BTC");
    }

    #[test]
    fn compose_service_substitutes_every_placeholder() {
        for (coin, image) in [
            (BITCOIN, "bitcoin/bitcoin:29"),
            (LITECOIN, "uphold/litecoin-core:0.21"),
        ] {
            let yaml = UtxoEngine::new(coin, "utxo-1", 40000).compose_service(StateMode::Ephemeral);
            assert!(yaml.contains("utxo-1:"));
            assert!(yaml.contains(image));
            assert!(yaml.contains("\"-regtest\""));
            assert!(yaml.contains(&format!("\"-rpcport={}\"", coin.rpc_port)));
            assert!(yaml.contains(&format!("\"40000:{}\"", coin.rpc_port)));
            assert!(!yaml.contains("{{"), "no placeholder should remain: {yaml}");
        }
    }

    #[test]
    fn persistent_compose_mounts_a_per_chain_datadir() {
        // Datadir is placed via the image's env var (the entrypoint appends its
        // own `-datadir`, which would override a flag), and the host dir is
        // bind-mounted. Per coin: BITCOIN_DATA vs LITECOIN_DATA.
        for (coin, env) in [(BITCOIN, "BITCOIN_DATA"), (LITECOIN, "LITECOIN_DATA")] {
            let engine = UtxoEngine::new(coin, "utxo-1", 40000);
            let yaml = engine.compose_service(StateMode::Persistent);
            assert!(yaml.contains(&format!("\"{env}=/data\"")), "{yaml}");
            assert!(yaml.contains("./state/utxo-utxo-1:/data"));
            // The datadir is placed via the env var, not an overridable command
            // flag (a `-datadir=` arg would be the broken form).
            assert!(!yaml.contains("\"-datadir="), "{yaml}");
            // That path is what --reset wipes / --resume preserves.
            assert_eq!(
                engine.session_paths(),
                vec!["state/utxo-utxo-1".to_string()]
            );

            // Ephemeral keeps the datadir in-container: no volume, no env var.
            let eph = engine.compose_service(StateMode::Ephemeral);
            assert!(!eph.contains(env));
            assert!(!eph.contains("volumes:"));
        }
    }

    #[test]
    fn manifest_entry_describes_a_regtest_chain() {
        let entry = UtxoEngine::new(LITECOIN, "litecoin-1", 19443).manifest_entry();
        assert_eq!(entry.kind, "litecoin");
        assert_eq!(entry.chain_id, "regtest");
        // Credentials are embedded so a manifest reader can connect immediately.
        assert_eq!(entry.rpc, "http://wharfnet:wharfnet@127.0.0.1:19443");
        assert!(entry.ws.is_none());
        // No baked tokens/contracts, and no funded account until post_boot runs.
        assert!(entry.tokens.is_empty());
        assert!(entry.contracts.is_empty());
        assert!(entry.accounts.is_empty());
        assert!(entry.fork.is_none());
    }

    #[test]
    fn both_coins_expose_a_utxo_rpc_explorer_target() {
        // Both coins get a btc-rpc-explorer-family explorer wired to their
        // in-network RPC (service name + internal port); `coin` selects the image.
        for (coin, name, port) in [
            (BITCOIN, "bitcoin-1", 18443),
            (LITECOIN, "litecoin-1", 19443),
        ] {
            match UtxoEngine::new(coin, name, port).explorer_target() {
                Some(ExplorerTarget::UtxoRpc {
                    coin: coin_kind,
                    chain_name,
                    rpc_service,
                    rpc_port,
                    ..
                }) => {
                    assert_eq!(coin_kind, coin.kind);
                    assert_eq!(chain_name, name);
                    assert_eq!(rpc_service, name);
                    assert_eq!(rpc_port, port);
                }
                other => panic!("{name} should expose a UtxoRpc explorer, got {other:?}"),
            }
        }
    }

    #[test]
    fn health_probe_is_authed_getblockchaininfo() {
        let probe = UtxoEngine::new(BITCOIN, "bitcoin-1", 18443).health_probe();
        assert!(matches!(
            probe,
            HealthProbe::JsonRpcAuth {
                method: "getblockchaininfo",
                ..
            }
        ));
    }

    // ---- docker-backed end-to-end runs against live bitcoind/litecoind ----
    //
    // Boot a real regtest chain and check the facts the engine promises: the
    // authed getblockchaininfo probe passes (so `up` waited on the right thing),
    // the boot wallet is funded with a mature coinbase, and the faucet credits a
    // fresh recipient. Bitcoin and Litecoin share the engine, so one coin per
    // test exercises the whole path. Both self-skip without Docker.

    use crate::harness::{Localnet, docker_available};
    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;

    /// Query the recipient wallet's confirmed balance (coins) via the node RPC,
    /// using the dev credentials embedded in the chain's rpc url.
    fn wallet_balance(rpc_url: &str, wallet: &str) -> f64 {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let authority = rpc_url
            .strip_prefix("http://")
            .unwrap_or(rpc_url)
            .split('/')
            .next()
            .unwrap();
        let hostport = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
        let mut stream = TcpStream::connect(hostport).unwrap();
        let body = r#"{"jsonrpc":"1.0","id":1,"method":"getbalance","params":[]}"#;
        let req = format!(
            "POST /wallet/{wallet} HTTP/1.1\r\nHost: h\r\nAuthorization: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            rpc::RPC_AUTH_HEADER,
            body.len(),
            body
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        let payload = resp.rsplit("\r\n\r\n").next().unwrap_or("");
        let v: serde_json::Value = serde_json::from_str(payload.trim()).unwrap();
        v["result"].as_f64().unwrap()
    }

    /// Create a fresh recipient wallet on the node and return one of its addresses,
    /// so the faucet funds a wallet distinct from the boot wallet.
    fn fresh_recipient(rpc_url: &str, wallet: &str) -> String {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let authority = rpc_url
            .strip_prefix("http://")
            .unwrap_or(rpc_url)
            .split('/')
            .next()
            .unwrap();
        let hostport = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
        let call = |path: &str, body: &str| -> String {
            let mut stream = TcpStream::connect(hostport).unwrap();
            let req = format!(
                "POST {path} HTTP/1.1\r\nHost: h\r\nAuthorization: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                rpc::RPC_AUTH_HEADER,
                body.len(),
                body
            );
            stream.write_all(req.as_bytes()).unwrap();
            let mut resp = String::new();
            stream.read_to_string(&mut resp).unwrap();
            resp.rsplit("\r\n\r\n")
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        };
        call(
            "/",
            &format!(r#"{{"jsonrpc":"1.0","id":1,"method":"createwallet","params":["{wallet}"]}}"#),
        );
        let resp = call(
            &format!("/wallet/{wallet}"),
            r#"{"jsonrpc":"1.0","id":1,"method":"getnewaddress","params":[]}"#,
        );
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        v["result"].as_str().unwrap().to_string()
    }

    /// Shared body: boot `net`, assert the boot wallet holds its mature coinbase,
    /// then faucet a fresh recipient and assert the credited balance.
    fn assert_boots_funded_and_faucets(net: &Localnet, expect_symbol: &str) {
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];

        // The boot wallet is funded (one mature 50-coin coinbase) and advertised.
        assert_eq!(
            chain.accounts.len(),
            1,
            "boot should record one funded account"
        );
        assert!(
            chain.accounts[0].balance.starts_with("50 "),
            "boot wallet should hold the mature coinbase: {}",
            chain.accounts[0].balance
        );
        assert!(chain.accounts[0].balance.ends_with(expect_symbol));

        // Faucet 2.5 coins to a fresh recipient wallet; it should confirm to 2.5.
        let recipient = fresh_recipient(&chain.rpc, "recv");
        crate::faucet::run_in(
            net.base(),
            net.project(),
            net.chain(),
            &recipient,
            "2.5",
            None,
            false,
        )
        .expect("faucet should fund the recipient");
        let bal = wallet_balance(&chain.rpc, "recv");
        assert!(
            (bal - 2.5).abs() < 1e-8,
            "recipient should hold 2.5 after the faucet send, got {bal}"
        );
    }

    /// The chain's current block height, via `getblockcount` (no wallet needed).
    /// A clean persistence signal: it's baked at boot (101) and grows as the
    /// faucet mines, so it survives `--resume` and resets on `--reset`.
    fn block_count(rpc_url: &str) -> u64 {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let authority = rpc_url
            .strip_prefix("http://")
            .unwrap_or(rpc_url)
            .split('/')
            .next()
            .unwrap();
        let hostport = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
        let mut stream = TcpStream::connect(hostport).unwrap();
        let body = r#"{"jsonrpc":"1.0","id":1,"method":"getblockcount","params":[]}"#;
        let req = format!(
            "POST / HTTP/1.1\r\nHost: h\r\nAuthorization: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            rpc::RPC_AUTH_HEADER,
            body.len(),
            body
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        let payload = resp.rsplit("\r\n\r\n").next().unwrap_or("");
        let v: serde_json::Value = serde_json::from_str(payload.trim()).unwrap();
        v["result"].as_u64().unwrap()
    }

    /// A dedicated high port, clear of the other e2e ports (Solana at 189xx).
    const BTC_E2E_PORT: u16 = 28443;
    const LTC_E2E_PORT: u16 = 29443;
    const BTC_PERSIST_PORT: u16 = 28444;
    const LTC_PERSIST_PORT: u16 = 29444;
    const BTC_UI_E2E_PORT: u16 = 28445;
    const LTC_UI_E2E_PORT: u16 = 29445;

    /// GET `path` on `127.0.0.1:port` and return the HTTP status code (0 on a
    /// connection/parse failure, so a not-yet-listening explorer just reads as
    /// "not ready" for the poll loop).
    fn http_status(port: u16, path: &str) -> u16 {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            return 0;
        };
        let req = format!("GET {path} HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n");
        if stream.write_all(req.as_bytes()).is_err() {
            return 0;
        }
        let mut resp = String::new();
        if stream.read_to_string(&mut resp).is_err() {
            return 0;
        }
        // Status line: "HTTP/1.1 200 OK".
        resp.split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0)
    }

    #[test]
    fn bitcoin_regtest_boots_funded_and_faucets() {
        if !docker_available() {
            eprintln!("skipping bitcoin e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_bitcoin("t-bitcoin", BTC_E2E_PORT);
        assert_boots_funded_and_faucets(&net, "BTC");
    }

    #[test]
    fn litecoin_regtest_boots_funded_and_faucets() {
        if !docker_available() {
            eprintln!("skipping litecoin e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_litecoin("t-litecoin", LTC_E2E_PORT);
        assert_boots_funded_and_faucets(&net, "LTC");
    }

    /// With explorers on, each UTXO chain boots its btc-rpc-explorer-family
    /// container (btc-rpc-explorer for Bitcoin, the ltc-rpc-explorer fork for
    /// Litecoin), which connects to the daemon over the docker network and serves
    /// its UI — advertised in the manifest. Both are booted in one localnet so
    /// their explorers take consecutive host ports (5100/5101) rather than racing
    /// on the same one. The explorer connects a few seconds after the chain is
    /// ready (a separate container doing server-side RPC), so poll rather than
    /// assume an immediate 200; Litecoin's image is amd64-only, so under emulation
    /// it needs a wider window. Self-skips without Docker.
    #[test]
    fn utxo_chains_serve_their_explorers() {
        if !docker_available() {
            eprintln!("skipping utxo explorer e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_utxo_ui(BTC_UI_E2E_PORT, LTC_UI_E2E_PORT);
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();

        // bitcoin-1 → 5100, litecoin-1 → 5101 (assigned in config/engine order).
        for (name, port, wait_secs) in [("bitcoin-1", 5100u16, 30u32), ("litecoin-1", 5101, 90)] {
            let mut last = 0;
            for _ in 0..wait_secs {
                last = http_status(port, "/");
                if last == 200 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            assert_eq!(last, 200, "{name} explorer should serve its UI");
            // A block page renders regtest data (chain wired through correctly).
            assert_eq!(
                http_status(port, "/block-height/0"),
                200,
                "{name} block page should render"
            );
            // ...and the manifest advertises exactly that URL.
            let chain = manifest.chains.iter().find(|c| c.name == name).unwrap();
            assert_eq!(
                chain.explorer.as_deref(),
                Some(format!("http://127.0.0.1:{port}").as_str())
            );
        }
    }

    /// The datadir is bind-mounted in persistent mode, so chain state must
    /// survive `down` → `up --resume` and be wiped by `up --reset`. Drives the
    /// orchestrator directly (the `boot_*` harness only does fresh boots) and
    /// uses block height as the persistence signal. Run per coin because the two
    /// images place their datadir via different env vars — the exact per-image
    /// difference this guards against. Self-skips without Docker.
    fn assert_state_survives_resume_and_resets_on_reset(kind: &str, port: u16, project: &str) {
        use crate::runtime::orchestrator::{self, UpMode};

        let dir = tempfile::TempDir::new_in(".").expect("temp dir under crate root");
        let base = dir.path();
        let chain = "persist-1";
        let config = base.join("wharfnet.toml");
        std::fs::write(
            &config,
            format!("[[chains]]\nname = \"{chain}\"\nkind = \"{kind}\"\nport = {port}\n"),
        )
        .unwrap();
        let rpc_url = format!("http://{RPC_USER}:{RPC_PASS}@127.0.0.1:{port}");

        // Tear the containers down even if an assertion panics. Declared after
        // `dir` so it drops first — down_in still sees the compose file.
        struct Teardown<'a>(&'a std::path::Path, &'a str);
        impl Drop for Teardown<'_> {
            fn drop(&mut self) {
                let _ = crate::runtime::orchestrator::down_in(self.0, self.1);
            }
        }
        let _guard = Teardown(base, project);

        // 1) First `up --resume`: fresh datadir, boot mines 101. Fund a fresh
        //    recipient — the faucet mines a block, so the height climbs past 101.
        orchestrator::up_in(base, project, UpMode::Resume, false, Some(&config))
            .expect("first up --resume should boot");
        let boot_height = block_count(&rpc_url);
        assert_eq!(boot_height, BOOT_BLOCKS, "boot should mine {BOOT_BLOCKS}");
        let recipient = fresh_recipient(&rpc_url, "recv");
        crate::faucet::run_in(base, project, chain, &recipient, "2.5", None, false)
            .expect("faucet should fund the recipient");
        let funded_height = block_count(&rpc_url);
        assert!(funded_height > boot_height, "faucet should mine a block");

        // 2) Tear down (the bind-mounted datadir survives on the host) and resume:
        //    the chain restores, so the height is exactly where we left it.
        orchestrator::down_in(base, project).expect("down should succeed");
        orchestrator::up_in(base, project, UpMode::Resume, false, Some(&config))
            .expect("second up --resume should boot");
        assert_eq!(
            block_count(&rpc_url),
            funded_height,
            "chain height must survive down → up --resume"
        );

        // 3) `up --reset` wipes the datadir and boots fresh — height is back to
        //    the baked coinbase count, with the faucet's block gone.
        orchestrator::down_in(base, project).expect("down before reset should succeed");
        orchestrator::up_in(base, project, UpMode::Reset, false, Some(&config))
            .expect("up --reset should boot");
        assert_eq!(
            block_count(&rpc_url),
            BOOT_BLOCKS,
            "up --reset must discard the persisted chain"
        );
    }

    #[test]
    fn bitcoin_state_survives_resume_and_resets_on_reset() {
        if !docker_available() {
            eprintln!("skipping bitcoin persistence e2e: docker unavailable");
            return;
        }
        assert_state_survives_resume_and_resets_on_reset(
            "bitcoin",
            BTC_PERSIST_PORT,
            "wharfnet-e2e-btc-persist",
        );
    }

    #[test]
    fn litecoin_state_survives_resume_and_resets_on_reset() {
        if !docker_available() {
            eprintln!("skipping litecoin persistence e2e: docker unavailable");
            return;
        }
        assert_state_survives_resume_and_resets_on_reset(
            "litecoin",
            LTC_PERSIST_PORT,
            "wharfnet-e2e-ltc-persist",
        );
    }
}

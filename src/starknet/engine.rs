//! The `starknet-devnet`-backed engine: the [`Engine`] impl for a local Starknet
//! chain.
//!
//! Unlike the EVM engine there is no baked state snapshot to stage — devnet
//! predeploys its accounts and the ETH/STRK fee tokens itself, deterministically
//! from `--seed`. The compose service YAML lives in
//! `src/resources/docker/services/` and is embedded at compile time.

use crate::runtime::engine::{Engine, HealthProbe, StateMode};
use crate::runtime::manifest::{Account, ChainEntry, Contract, Token};

/// Pinned devnet image. Pinned (like the Anvil and Otterscan images) so the
/// predeployed accounts and token addresses below stay reproducible.
const DEVNET_IMAGE: &str = "shardlabs/starknet-devnet-rs:0.4.3";

/// Port devnet listens on inside its container (its default).
const DEVNET_INTERNAL_PORT: u16 = 5050;

/// Fixed RNG seed, so the predeployed accounts are identical on every boot —
/// the Starknet analogue of Anvil's fixed dev mnemonic.
const DEVNET_SEED: u16 = 0;

/// Number of predeployed accounts (matches the three EVM dev accounts).
const DEVNET_ACCOUNTS: u16 = 3;

/// Resolved felt for devnet's default chain id (`TESTNET` → `SN_SEPOLIA`), as
/// returned by `starknet_chainId`. A felt, so it's carried as a string.
const SN_SEPOLIA: &str = "0x534e5f5345504f4c4941";

/// Compose service template for a `starknet-devnet` chain.
const STARKNET_SERVICE_TEMPLATE: &str = include_str!("../resources/docker/services/starknet.yml");

/// A `starknet-devnet`-backed Starknet chain.
pub struct StarknetEngine {
    name: String,
    image: String,
    host_port: u16,
}

impl StarknetEngine {
    pub fn devnet(name: &str, host_port: u16) -> Self {
        StarknetEngine {
            name: name.to_string(),
            image: DEVNET_IMAGE.to_string(),
            host_port,
        }
    }

    /// The deterministic predeployed accounts for `--seed 0`, captured from the
    /// pinned image. Addresses are the 0x + 64-hex canonical form; the docker
    /// e2e test asserts these still match devnet's `/predeployed_accounts`.
    /// Each is funded with 1000 ETH and 1000 STRK (10^21 wei/fri).
    fn accounts() -> Vec<Account> {
        [
            (
                "0x064b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691",
                "0x0000000000000000000000000000000071d7bb07b9a64f6f78ac4c816aff4da9",
            ),
            (
                "0x078662e7352d062084b0010068b99288486c2d8b914f6e2a55ce945f8792c8b1",
                "0x000000000000000000000000000000000e1406455b7d66b1690803be066cbe5e",
            ),
            (
                "0x049dfb8ce986e21d354ac93ea65e6a11f639c1934ea253e5ff14ca62eca0f38e",
                "0x00000000000000000000000000000000a20a02f0ac53692d144b20cb371a60d7",
            ),
        ]
        .into_iter()
        .map(|(address, private_key)| Account {
            address: address.to_string(),
            private_key: private_key.to_string(),
            balance: "1000 ETH & 1000 STRK".to_string(),
        })
        .collect()
    }

    /// The predeployed fee tokens devnet funds accounts with. Both are 18-decimal
    /// and live at fixed addresses (identical to the public Starknet networks).
    fn tokens() -> Vec<Token> {
        vec![
            Token {
                symbol: "ETH".to_string(),
                name: "Ether".to_string(),
                address: "0x049d36570d4e46f48e99674bd3fcc84644ddd6b96f7c741b1562b82f9e004dc7"
                    .to_string(),
                decimals: 18,
            },
            Token {
                symbol: "STRK".to_string(),
                name: "Starknet Token".to_string(),
                address: "0x04718f5a0fc34cc1af16a1cdee98ffb20c31f5cd61d6ab07201858f4287c938d"
                    .to_string(),
                decimals: 18,
            },
        ]
    }

    /// Canonical infra devnet predeploys at a fixed address: the Universal
    /// Deployer Contract, used for deterministic (CREATE2-style) deploys.
    fn contracts() -> Vec<Contract> {
        vec![Contract {
            name: "UDC".to_string(),
            address: "0x041a78e741e5af2fec34b695679bc6891742439f7afb8484ecd7766661ad02bf"
                .to_string(),
        }]
    }
}

impl Engine for StarknetEngine {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn host_port(&self) -> u16 {
        self.host_port
    }

    fn compose_service(&self, _mode: StateMode) -> String {
        // Persistence (dump/load) is not wired up yet, so both state modes render
        // the same service and {{STATE_ARGS}} collapses to nothing.
        STARKNET_SERVICE_TEMPLATE
            .replace("{{NAME}}", &self.name)
            .replace("{{IMAGE}}", &self.image)
            .replace("{{PORT}}", &DEVNET_INTERNAL_PORT.to_string())
            .replace("{{HOST_PORT}}", &self.host_port.to_string())
            .replace("{{SEED}}", &DEVNET_SEED.to_string())
            .replace("{{ACCOUNTS}}", &DEVNET_ACCOUNTS.to_string())
            .replace("{{STATE_ARGS}}", "")
    }

    fn manifest_entry(&self) -> ChainEntry {
        ChainEntry {
            name: self.name.clone(),
            kind: "starknet".to_string(),
            // devnet serves JSON-RPC at /rpc (and /).
            rpc: format!("http://127.0.0.1:{}/rpc", self.host_port),
            chain_id: SN_SEPOLIA.to_string(),
            accounts: Self::accounts(),
            tokens: Self::tokens(),
            contracts: Self::contracts(),
            fork: None,
            explorer: None,
        }
    }

    fn health_probe(&self) -> HealthProbe {
        // devnet exposes a plain HTTP liveness endpoint rather than eth_-style RPC.
        HealthProbe::HttpGet { path: "/is_alive" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devnet_constructor_sets_fields() {
        let e = StarknetEngine::devnet("starknet-1", 5050);
        assert_eq!(e.name(), "starknet-1");
        assert_eq!(e.host_port(), 5050);
        assert_eq!(e.image, DEVNET_IMAGE);
    }

    #[test]
    fn compose_service_substitutes_every_placeholder() {
        for mode in [StateMode::Ephemeral, StateMode::Persistent] {
            let yaml = StarknetEngine::devnet("starknet-1", 5055).compose_service(mode);
            assert!(yaml.contains("starknet-1:"));
            assert!(yaml.contains("shardlabs/starknet-devnet-rs:0.4.3"));
            assert!(yaml.contains("\"--seed\", \"0\""));
            assert!(yaml.contains("\"--accounts\", \"3\""));
            // Published host port maps to the container's 5050.
            assert!(yaml.contains("\"5055:5050\""));
            assert!(!yaml.contains("{{"), "no placeholder should remain: {yaml}");
            assert!(!yaml.contains("}}"), "no placeholder should remain: {yaml}");
        }
    }

    #[test]
    fn manifest_entry_describes_the_chain() {
        let entry = StarknetEngine::devnet("starknet-1", 5050).manifest_entry();
        assert_eq!(entry.kind, "starknet");
        assert_eq!(entry.rpc, "http://127.0.0.1:5050/rpc");
        assert_eq!(entry.chain_id, "0x534e5f5345504f4c4941");
        assert_eq!(entry.accounts.len(), 3);
        assert_eq!(
            entry.accounts[0].address,
            "0x064b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691"
        );
    }

    #[test]
    fn manifest_entry_lists_the_fee_tokens() {
        let entry = StarknetEngine::devnet("starknet-1", 5050).manifest_entry();
        let symbols: Vec<&str> = entry.tokens.iter().map(|t| t.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["ETH", "STRK"]);
        assert!(entry.tokens.iter().all(|t| t.decimals == 18));
        assert_eq!(entry.contracts[0].name, "UDC");
    }

    #[test]
    fn health_probe_is_the_is_alive_endpoint() {
        let probe = StarknetEngine::devnet("starknet-1", 5050).health_probe();
        assert!(matches!(probe, HealthProbe::HttpGet { path: "/is_alive" }));
    }

    #[test]
    fn stages_no_files_and_has_no_explorer() {
        let e = StarknetEngine::devnet("starknet-1", 5050);
        assert!(e.staged_files(StateMode::Ephemeral).is_empty());
        assert!(e.explorer_target().is_none());
    }

    // ---- docker-backed end-to-end run against a live starknet-devnet ----
    //
    // Boots a real devnet and checks the two facts the engine hardcodes: that
    // the liveness probe passes (so `up` really waited on the right endpoint),
    // and that the baked seed-0 accounts still match what devnet predeploys.
    // Self-skips without Docker.

    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;
    use crate::testkit::{Localnet, docker_available};
    use std::collections::HashSet;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    /// A dedicated high port, away from the EVM e2e ports, so this can run in
    /// parallel with the other docker tests.
    const SN_E2E_PORT: u16 = 5150;

    /// Minimal HTTP GET → `(status_code, body)`.
    fn http_get(port: u16, path: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
        stream.write_all(req.as_bytes()).unwrap();
        let mut resp = String::new();
        let _ = stream.read_to_string(&mut resp);
        let status = resp
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let body = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    /// Normalize a felt for comparison: drop the `0x` and any leading zeros so
    /// devnet's minimal form matches the manifest's zero-padded form.
    fn norm(felt: &str) -> String {
        let h = felt.trim_start_matches("0x").trim_start_matches('0');
        if h.is_empty() {
            "0".into()
        } else {
            h.to_lowercase()
        }
    }

    #[test]
    fn starknet_devnet_boots_and_predeploys_the_baked_accounts() {
        if !docker_available() {
            eprintln!("skipping starknet e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_starknet("t-starknet", SN_E2E_PORT);

        // Liveness: `up` only returns once /is_alive passed, but assert it
        // directly too — this is the endpoint the engine's health probe targets.
        let (status, body) = http_get(SN_E2E_PORT, "/is_alive");
        assert_eq!(status, 200, "GET /is_alive should be 200");
        assert!(body.contains("Alive"), "unexpected /is_alive body: {body}");

        // The manifest advertises the baked seed-0 accounts...
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];
        assert_eq!(chain.kind, "starknet");
        let baked: HashSet<String> = chain.accounts.iter().map(|a| norm(&a.address)).collect();

        // ...which must still match what devnet actually predeploys.
        let (_, accounts_json) = http_get(SN_E2E_PORT, "/predeployed_accounts");
        let live: Vec<serde_json::Value> = serde_json::from_str(&accounts_json)
            .unwrap_or_else(|e| panic!("parsing /predeployed_accounts '{accounts_json}': {e}"));
        let live: HashSet<String> = live
            .iter()
            .map(|a| norm(a["address"].as_str().unwrap()))
            .collect();
        assert_eq!(baked, live, "baked seed-0 accounts drifted from the image");
    }
}

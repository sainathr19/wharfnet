//! The `surfpool`-backed Solana engine: the [`Engine`] impl for a local Solana
//! chain.
//!
//! surfpool runs an in-memory SVM ("surfnet") that boots in about a second and
//! serves the standard Solana JSON-RPC. wharfnet funds a fixed set of
//! deterministic dev accounts at boot with `--airdrop` — the Solana analogue of
//! Anvil's fixed dev mnemonic. The compose service YAML lives in
//! `src/resources/docker/services/` and is embedded into the binary at compile
//! time — edit that template, not a Rust string.

use crate::runtime::engine::{Engine, HealthProbe, StateMode};
use crate::runtime::manifest::{Account, ChainEntry};

/// Pinned surfpool image. Pinned (like the Anvil and devnet images) so the boot
/// behaviour and predeployed accounts stay reproducible. This tag speaks
/// solana-core 4.0.0 and the standard Solana JSON-RPC.
const SURFPOOL_IMAGE: &str = "surfpool/surfpool:1.4.0";

/// Port surfpool serves the Solana JSON-RPC on inside its container (its default).
const SURFPOOL_INTERNAL_PORT: u16 = 8899;

/// Lamports airdropped to each dev account at boot: 10,000 SOL (1 SOL = 10^9
/// lamports), matching surfpool's own default and the spirit of Anvil's
/// 10,000-ETH dev accounts. surfpool also tops each account up to the
/// rent-exempt minimum, so the on-chain balance is marginally higher.
const AIRDROP_LAMPORTS: u64 = 10_000_000_000_000;

/// Manifest chain identifier. Solana has no numeric chain id, and a surfnet's
/// genesis hash — while deterministic for a fixed set of boot airdrops — shifts
/// when the airdrop set changes, so it's unsuitable as a stable identifier. A
/// local chain is simply "localnet".
const SOLANA_CHAIN_ID: &str = "localnet";

/// Compose service template for a `surfpool` Solana chain.
const SOLANA_SERVICE_TEMPLATE: &str = include_str!("../resources/docker/services/solana.yml");

/// A `surfpool`-backed Solana chain.
pub struct SolanaEngine {
    name: String,
    image: String,
    host_port: u16,
}

impl SolanaEngine {
    pub fn surfpool(name: &str, host_port: u16) -> Self {
        SolanaEngine {
            name: name.to_string(),
            image: SURFPOOL_IMAGE.to_string(),
            host_port,
        }
    }

    /// Deterministic dev accounts, funded at boot. Each keypair is derived from a
    /// documented seed — `sha256("wharfnet-solana-dev-<i>")` fed to ed25519 — so
    /// anyone can regenerate it (the Solana analogue of Anvil's fixed test
    /// mnemonic). These are well-known throwaway keys: safe to publish, never use
    /// with real funds. `private_key` is the base58 64-byte secret that Solana
    /// wallets and the CLI import (`[seed || pubkey]`).
    fn accounts() -> Vec<Account> {
        [
            (
                "9akreS78QY4sx2d3aXHdrPCv1rQay1JXoiVWXK6rP9jh",
                "27qKSFAfeFQso21YSNus2TSv62soiH2XY2YK3MB8h51swgf8F3PE2g2rngyxMBj9Bon4UVm5zMG7dJEE2EpnahF9",
            ),
            (
                "9qyyzGXkchnjv79kFfLDMC9Rvs3rjNEMHshJZoksCbiL",
                "58WTqY1kJCAFyPmngoutvQMAxTQwzEF2GDVMUSzMRemF8wW2fRcsDg8BYCM3SrpPqQUJEbuej54Mh1afxWFBpHak",
            ),
            (
                "5fTNM2ctTnUsgjZUrRLdzMc8zsZW46YCRhjGuwGiuwTh",
                "rsriwMH6yA8nzezbeMYZGH3FuucqCyMvjT3u93ePRKxT31iyvK4PLwbScg3wXczHUT4xfsBxdh9G9DZxXtDBcK5",
            ),
        ]
        .into_iter()
        .map(|(address, private_key)| Account {
            address: address.to_string(),
            private_key: private_key.to_string(),
            balance: "10000 SOL".to_string(),
        })
        .collect()
    }

    /// The `--airdrop <pubkey>` args that fund each dev account at boot, followed
    /// by the shared `--airdrop-amount`. Emitted as leading-comma JSON tokens so
    /// they splice into the compose command array.
    fn airdrop_args() -> String {
        let mut args = String::new();
        for account in Self::accounts() {
            args.push_str(&format!(r#", "--airdrop", "{}""#, account.address));
        }
        args.push_str(&format!(r#", "--airdrop-amount", "{AIRDROP_LAMPORTS}""#));
        args
    }
}

impl Engine for SolanaEngine {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn host_port(&self) -> u16 {
        self.host_port
    }

    fn compose_service(&self, _mode: StateMode) -> String {
        // A standalone local chain runs `--offline` (no mainnet datasource).
        // Forking (`--rpc-url`) and persistence land in later work, so their arg
        // slots are empty for now, and the Studio explorer stays off (`--no-studio`)
        // until the explorer work — mirroring how the Starknet UI arrived later.
        SOLANA_SERVICE_TEMPLATE
            .replace("{{NAME}}", &self.name)
            .replace("{{IMAGE}}", &self.image)
            .replace("{{PORT}}", &SURFPOOL_INTERNAL_PORT.to_string())
            .replace("{{HOST_PORT}}", &self.host_port.to_string())
            .replace("{{DATASOURCE_ARGS}}", r#", "--offline""#)
            .replace("{{AIRDROP_ARGS}}", &Self::airdrop_args())
            .replace("{{STATE_ARGS}}", "")
            .replace("{{STUDIO_ARGS}}", r#", "--no-studio""#)
    }

    fn manifest_entry(&self) -> ChainEntry {
        ChainEntry {
            name: self.name.clone(),
            kind: "solana".to_string(),
            rpc: format!("http://127.0.0.1:{}", self.host_port),
            chain_id: SOLANA_CHAIN_ID.to_string(),
            accounts: Self::accounts(),
            // SPL test tokens, forking, and the Studio explorer arrive in later
            // work; a freshly-booted local chain advertises none of them yet.
            tokens: Vec::new(),
            contracts: Vec::new(),
            fork: None,
            explorer: None,
        }
    }

    fn health_probe(&self) -> HealthProbe {
        // surfpool answers the standard Solana JSON-RPC; `getHealth` → "ok" is the
        // cheapest readiness ping.
        HealthProbe::JsonRpc {
            method: "getHealth",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surfpool_constructor_sets_fields() {
        let e = SolanaEngine::surfpool("solana-1", 8899);
        assert_eq!(e.name(), "solana-1");
        assert_eq!(e.host_port(), 8899);
        assert_eq!(e.image, SURFPOOL_IMAGE);
    }

    #[test]
    fn compose_service_substitutes_every_placeholder() {
        for mode in [StateMode::Ephemeral, StateMode::Persistent] {
            let yaml = SolanaEngine::surfpool("solana-1", 18899).compose_service(mode);
            assert!(yaml.contains("solana-1:"));
            assert!(yaml.contains("surfpool/surfpool:1.4.0"));
            // Headless, standalone, explorer off.
            assert!(yaml.contains("\"start\", \"--no-tui\""));
            assert!(yaml.contains("\"--offline\""));
            assert!(yaml.contains("\"--no-studio\""));
            // Published host port maps to the container's 8899.
            assert!(yaml.contains("\"18899:8899\""));
            assert!(!yaml.contains("{{"), "no placeholder should remain: {yaml}");
            assert!(!yaml.contains("}}"), "no placeholder should remain: {yaml}");
        }
    }

    #[test]
    fn compose_service_airdrops_every_dev_account() {
        let yaml = SolanaEngine::surfpool("solana-1", 8899).compose_service(StateMode::Ephemeral);
        for account in SolanaEngine::accounts() {
            assert!(
                yaml.contains(&format!("\"--airdrop\", \"{}\"", account.address)),
                "dev account {} should be airdropped at boot",
                account.address
            );
        }
        // The shared airdrop amount is passed once.
        assert!(yaml.contains("\"--airdrop-amount\", \"10000000000000\""));
    }

    #[test]
    fn manifest_entry_describes_the_chain() {
        let entry = SolanaEngine::surfpool("solana-1", 8899).manifest_entry();
        assert_eq!(entry.kind, "solana");
        assert_eq!(entry.rpc, "http://127.0.0.1:8899");
        assert_eq!(entry.chain_id, "localnet");
        assert_eq!(entry.accounts.len(), 3);
        assert_eq!(
            entry.accounts[0].address,
            "9akreS78QY4sx2d3aXHdrPCv1rQay1JXoiVWXK6rP9jh"
        );
        // No baked tokens/contracts/fork/explorer on a fresh local chain yet.
        assert!(entry.tokens.is_empty());
        assert!(entry.contracts.is_empty());
        assert!(entry.fork.is_none());
        assert!(entry.explorer.is_none());
    }

    #[test]
    fn dev_accounts_are_deterministic_and_published_with_secrets() {
        let accounts = SolanaEngine::accounts();
        assert_eq!(accounts.len(), 3);
        let addresses: Vec<&str> = accounts.iter().map(|a| a.address.as_str()).collect();
        assert_eq!(
            addresses,
            vec![
                "9akreS78QY4sx2d3aXHdrPCv1rQay1JXoiVWXK6rP9jh",
                "9qyyzGXkchnjv79kFfLDMC9Rvs3rjNEMHshJZoksCbiL",
                "5fTNM2ctTnUsgjZUrRLdzMc8zsZW46YCRhjGuwGiuwTh",
            ]
        );
        // Each account carries the base58 secret so tooling can sign as it.
        assert!(accounts.iter().all(|a| !a.private_key.is_empty()));
    }

    #[test]
    fn health_probe_is_get_health() {
        let probe = SolanaEngine::surfpool("solana-1", 8899).health_probe();
        assert!(matches!(
            probe,
            HealthProbe::JsonRpc {
                method: "getHealth"
            }
        ));
        // surfpool serves no separate explorer container.
        assert!(
            SolanaEngine::surfpool("solana-1", 8899)
                .explorer_target()
                .is_none()
        );
    }

    // ---- docker-backed end-to-end run against a live surfpool ----
    //
    // Boots a real surfpool chain and checks the two facts the engine relies on:
    // that the getHealth probe passes (so `up` waited on the right endpoint), and
    // that every baked dev account is funded by the boot airdrop. Self-skips
    // without Docker.

    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;
    use crate::testkit::{Localnet, docker_available};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    /// A dedicated high port, away from the other e2e ports, so this can run in
    /// parallel with the EVM and Starknet docker tests.
    const SOL_E2E_PORT: u16 = 18990;

    /// Minimal JSON-RPC POST to surfpool's `/` endpoint → raw response body.
    fn rpc(port: u16, body: &str) -> String {
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
        resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
    }

    #[test]
    fn solana_surfpool_boots_with_funded_dev_accounts() {
        if !docker_available() {
            eprintln!("skipping solana e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_solana("t-solana", SOL_E2E_PORT);

        // Liveness: `up` only returns once getHealth passed, but assert it
        // directly too — this is the method the engine's health probe targets.
        let health = rpc(
            SOL_E2E_PORT,
            r#"{"jsonrpc":"2.0","id":1,"method":"getHealth","params":[]}"#,
        );
        assert!(
            health.contains("\"result\":\"ok\""),
            "getHealth should report ok: {health}"
        );

        // The manifest advertises the baked dev accounts...
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        let chain = &manifest.chains[0];
        assert_eq!(chain.kind, "solana");
        assert_eq!(chain.accounts.len(), 3);

        // ...and each must be funded on-chain by the boot airdrop. getBalance
        // returns the lamport balance under result.value.
        for account in &chain.accounts {
            let body = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{}"]}}"#,
                account.address
            );
            let resp = rpc(SOL_E2E_PORT, &body);
            let parsed: serde_json::Value = serde_json::from_str(&resp)
                .unwrap_or_else(|e| panic!("parsing getBalance '{resp}': {e}"));
            let lamports = parsed["result"]["value"].as_u64().unwrap_or_else(|| {
                panic!("no balance for {} in {resp}", account.address);
            });
            assert!(
                lamports >= AIRDROP_LAMPORTS,
                "dev account {} should hold at least the airdrop ({AIRDROP_LAMPORTS}); got {lamports}",
                account.address
            );
        }
    }
}

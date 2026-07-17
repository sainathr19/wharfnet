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

/// Port surfpool serves the Studio explorer on inside its container (its
/// default). Unlike the Starknet web UI — which devnet serves at `/ui` on the
/// RPC port — surfpool's Studio is a separate service on its own port, so
/// wharfnet publishes it as a second host mapping when the explorer is on.
const SURFPOOL_STUDIO_INTERNAL_PORT: u16 = 18488;

/// Offset added to a chain's published RPC host port to derive its Studio host
/// port, so `solana-1` on 8899 serves Studio on 18899. Deterministic and — since
/// RPC host ports are already unique across chains — collision-free between
/// Solana chains; [`extra_host_ports`](Engine::extra_host_ports) surfaces it to
/// the orchestrator's cross-chain port check for the rare clash with another
/// chain's RPC port.
const STUDIO_HOST_PORT_OFFSET: u16 = 10_000;

/// Port surfpool serves the WebSocket RPC on inside its container (its default).
/// Subscriptions (`slotSubscribe`, `logsSubscribe`) and `confirmTransaction`
/// ride this endpoint, so it must be published for host-side clients to work.
const SURFPOOL_WS_INTERNAL_PORT: u16 = 8900;

/// Offset added to a chain's published RPC host port to derive its WebSocket
/// host port (`solana-1` on 8899 → WS on 8900). This mirrors Solana's own
/// convention — clients like `@solana/web3.js` derive the WS URL as one past the
/// RPC port — so a host client that only knows the RPC URL finds the WS endpoint
/// automatically. Always published (WS is core RPC, not gated by `--bare`), and
/// surfaced through [`extra_host_ports`](Engine::extra_host_ports) for the
/// collision check.
const WS_HOST_PORT_OFFSET: u16 = 1;

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

/// Fork settings for a Solana chain: mirror a live network's state from `url` (a
/// Solana JSON-RPC provider) via surfpool's copy-on-read fork. Unlike the EVM and
/// Starknet forks, there is **no block/slot pin** — surfpool has no fork-at-slot
/// flag, so a fork always tracks the datasource's current slot (config rejects
/// `fork_block` on Solana chains).
struct Fork {
    url: String,
}

impl Fork {
    /// A redacted description safe to record and print — the RPC key is dropped.
    fn describe(&self) -> String {
        crate::runtime::fork::describe(&self.url, None)
    }
}

/// A `surfpool`-backed Solana chain.
pub struct SolanaEngine {
    name: String,
    image: String,
    host_port: u16,
    studio: bool,
    fork: Option<Fork>,
}

impl SolanaEngine {
    pub fn surfpool(name: &str, host_port: u16) -> Self {
        SolanaEngine {
            name: name.to_string(),
            image: SURFPOOL_IMAGE.to_string(),
            host_port,
            studio: false,
            fork: None,
        }
    }

    /// Enable surfpool's Studio explorer for this chain. On by default under
    /// `up`; disabled by `up --bare`. Studio is a separate in-container service
    /// (not served on the RPC port like the Starknet UI), so enabling it
    /// publishes a second host port — [`studio_host_port`](Self::studio_host_port)
    /// — rather than reusing the RPC one.
    pub fn studio(mut self, enabled: bool) -> Self {
        self.studio = enabled;
        self
    }

    /// Host port this chain's Studio explorer is published on: the RPC host port
    /// plus a fixed offset (`solana-1` on 8899 → 18899). `saturating_add` keeps a
    /// near-max RPC port from panicking; any resulting clash surfaces through the
    /// orchestrator's port check, not a docker bind error.
    fn studio_host_port(&self) -> u16 {
        self.host_port.saturating_add(STUDIO_HOST_PORT_OFFSET)
    }

    /// URL of surfpool's Studio explorer for this chain, when enabled. The
    /// browser reaches it on the published Studio host port (served at the root).
    fn studio_url(&self) -> Option<String> {
        self.studio
            .then(|| format!("http://127.0.0.1:{}", self.studio_host_port()))
    }

    /// Host port this chain's WebSocket RPC is published on: the RPC host port
    /// plus one (`solana-1` on 8899 → 8900), matching Solana's client convention.
    /// `saturating_add` guards a near-max RPC port; any clash surfaces through the
    /// orchestrator's port check.
    fn ws_host_port(&self) -> u16 {
        self.host_port.saturating_add(WS_HOST_PORT_OFFSET)
    }

    /// WebSocket RPC URL for this chain, on its published WS host port. Always
    /// served (WS is core RPC), so — unlike the explorer — this is unconditional.
    fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.ws_host_port())
    }

    /// Fork this chain from a live Solana RPC via surfpool's `--rpc-url`
    /// (copy-on-read). A forked chain mirrors real network state, so — like the
    /// EVM and Starknet chains — it does not seed the baked SPL test tokens and
    /// advertises none. surfpool can't pin the fork to a slot, so there's no
    /// block argument (config rejects `fork_block` for Solana).
    pub fn fork(mut self, url: String) -> Self {
        self.fork = Some(Fork { url });
        self
    }

    /// Whether this chain forks a live network.
    fn is_forked(&self) -> bool {
        self.fork.is_some()
    }

    /// Path (relative to the state dir) of this chain's persistent surfnet
    /// database. Named `session-…` so the orchestrator's reset and resume
    /// detection — which key off that prefix — treat it like any other saved
    /// session (it matches `.sqlite` there, alongside the `.json` sessions).
    fn session_rel_path(&self) -> String {
        format!("state/session-{}.sqlite", self.name)
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

    fn extra_host_ports(&self) -> Vec<u16> {
        // The WebSocket RPC is always published on its own host port; the Studio
        // explorer adds another when enabled. Surface both so the orchestrator's
        // collision check covers them.
        let mut ports = vec![self.ws_host_port()];
        if self.studio {
            ports.push(self.studio_host_port());
        }
        ports
    }

    fn compose_service(&self, mode: StateMode) -> String {
        // The datasource is either a standalone local chain (`--offline`) or a
        // copy-on-read fork of a live network (`--rpc-url`). Dev accounts are
        // airdropped either way, so a forked chain still has funded signers
        // layered over the live state.
        let datasource_args = match &self.fork {
            Some(fork) => format!(r#", "--rpc-url", "{}""#, fork.url),
            None => r#", "--offline""#.to_string(),
        };
        // Studio explorer: off by default (`--no-studio`). When on, pin its
        // in-container port (Studio defaults on) and publish it as a second host
        // mapping so a browser on the host can reach it — surfpool serves Studio
        // on its own port, not on the RPC port like the Starknet `--ui`.
        let (studio_args, studio_port_mapping) = if self.studio {
            (
                format!(r#", "--studio-port", "{SURFPOOL_STUDIO_INTERNAL_PORT}""#),
                format!(
                    "\n      - \"{}:{}\"",
                    self.studio_host_port(),
                    SURFPOOL_STUDIO_INTERNAL_PORT
                ),
            )
        } else {
            (r#", "--no-studio""#.to_string(), String::new())
        };
        // Persistence: `--db` + a stable `--surfnet-id` write surfnet state to a
        // per-chain SQLite db under the bind-mounted state dir, restored on the
        // next boot. Ephemeral runs in memory (no db), so it's disposable. This
        // layers over a fork too — local writes persist above the live state.
        let state_args = match mode {
            StateMode::Ephemeral => String::new(),
            StateMode::Persistent => format!(
                r#", "--db", "/{}", "--surfnet-id", "{}""#,
                self.session_rel_path(),
                self.name
            ),
        };
        SOLANA_SERVICE_TEMPLATE
            .replace("{{NAME}}", &self.name)
            .replace("{{IMAGE}}", &self.image)
            .replace("{{PORT}}", &SURFPOOL_INTERNAL_PORT.to_string())
            .replace("{{HOST_PORT}}", &self.host_port.to_string())
            .replace("{{WS_PORT}}", &SURFPOOL_WS_INTERNAL_PORT.to_string())
            .replace("{{WS_HOST_PORT}}", &self.ws_host_port().to_string())
            .replace("{{DATASOURCE_ARGS}}", &datasource_args)
            .replace("{{AIRDROP_ARGS}}", &Self::airdrop_args())
            .replace("{{STATE_ARGS}}", &state_args)
            .replace("{{STUDIO_ARGS}}", &studio_args)
            .replace("{{STUDIO_PORT_MAPPING}}", &studio_port_mapping)
    }

    fn manifest_entry(&self) -> ChainEntry {
        // A forked chain mirrors live state; wharfnet seeds nothing onto it, so it
        // advertises no baked tokens — only the fork. The predeployed dev accounts
        // still apply (surfpool airdrops them over the fork).
        let forked = self.is_forked();
        ChainEntry {
            name: self.name.clone(),
            kind: "solana".to_string(),
            rpc: format!("http://127.0.0.1:{}", self.host_port),
            // surfpool serves WS on its own port; advertise it (RPC port + 1) so
            // subscription clients don't have to guess. Always present.
            ws: Some(self.ws_url()),
            chain_id: SOLANA_CHAIN_ID.to_string(),
            accounts: Self::accounts(),
            // The SPL test tokens are seeded post-boot via cheatcodes (see
            // `post_boot`); advertise their deterministic mint addresses here.
            tokens: if forked {
                Vec::new()
            } else {
                crate::solana::tokens::manifest_tokens()
            },
            contracts: Vec::new(),
            fork: self.fork.as_ref().map(Fork::describe),
            // surfpool's Studio explorer, when enabled — served on its own
            // published host port, unlike the Starknet UI on the RPC port.
            explorer: self.studio_url(),
        }
    }

    fn health_probe(&self) -> HealthProbe {
        // surfpool answers the standard Solana JSON-RPC; `getHealth` → "ok" is the
        // cheapest readiness ping.
        HealthProbe::JsonRpc {
            method: "getHealth",
        }
    }

    fn post_boot(&self) -> anyhow::Result<()> {
        // A forked chain mirrors live state, so it seeds no baked tokens (its
        // manifest advertises none). Otherwise surfpool has no program to deploy
        // for SPL tokens, so wharfnet seeds the baked test tokens (create mints,
        // fund the dev accounts) via cheatcodes once the RPC is live, rather than
        // loading a state file at boot.
        if self.is_forked() {
            return Ok(());
        }
        // A resumed persistent session restored the tokens (and the user's
        // balances) from its db, so only seed when they're absent — re-seeding
        // would reset the balances to the baked amounts. Ephemeral and first
        // persistent boots start empty, so they seed as usual.
        let chain = self.manifest_entry();
        if crate::solana::tokens::already_seeded(&chain)? {
            return Ok(());
        }
        crate::solana::tokens::seed(&chain)
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
            // Published host port maps to the container's 8899, and the WS port
            // (RPC host port + 1 → 18900) maps to the container's 8900.
            assert!(yaml.contains("\"18899:8899\""));
            assert!(yaml.contains("\"--ws-port\", \"8900\""));
            assert!(yaml.contains("\"18900:8900\""));
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
    fn persistent_compose_adds_a_per_chain_surfnet_db() {
        let yaml = SolanaEngine::surfpool("solana-1", 8899).compose_service(StateMode::Persistent);
        assert!(yaml.contains("\"--db\", \"/state/session-solana-1.sqlite\""));
        assert!(yaml.contains("\"--surfnet-id\", \"solana-1\""));
    }

    #[test]
    fn ephemeral_compose_runs_in_memory_without_a_db() {
        let yaml = SolanaEngine::surfpool("solana-1", 8899).compose_service(StateMode::Ephemeral);
        assert!(!yaml.contains("--db"), "ephemeral must not persist: {yaml}");
        assert!(!yaml.contains("--surfnet-id"));
    }

    #[test]
    fn manifest_entry_describes_the_chain() {
        let entry = SolanaEngine::surfpool("solana-1", 8899).manifest_entry();
        assert_eq!(entry.kind, "solana");
        assert_eq!(entry.rpc, "http://127.0.0.1:8899");
        // surfpool serves WS on the RPC port + 1.
        assert_eq!(entry.ws.as_deref(), Some("ws://127.0.0.1:8900"));
        assert_eq!(entry.chain_id, "localnet");
        assert_eq!(entry.accounts.len(), 3);
        assert_eq!(
            entry.accounts[0].address,
            "9akreS78QY4sx2d3aXHdrPCv1rQay1JXoiVWXK6rP9jh"
        );
        // The baked SPL test tokens are advertised (seeded post-boot); no infra
        // contracts or fork on a fresh local chain, and the explorer is off unless
        // enabled (default constructor leaves Studio off).
        let symbols: Vec<&str> = entry.tokens.iter().map(|t| t.symbol.as_str()).collect();
        assert_eq!(symbols, vec!["USDC", "WBTC"]);
        assert!(entry.contracts.is_empty());
        assert!(entry.fork.is_none());
        assert!(entry.explorer.is_none());
    }

    #[test]
    fn studio_enabled_publishes_the_port_and_advertises_the_explorer() {
        let e = SolanaEngine::surfpool("solana-1", 8899).studio(true);
        // The flag rides on surfpool's command (both state modes), and the
        // container's Studio port is published on the host at RPC port + 10000.
        for mode in [StateMode::Ephemeral, StateMode::Persistent] {
            let yaml = e.compose_service(mode);
            assert!(
                yaml.contains("\"--studio-port\", \"18488\""),
                "studio on → surfpool serves its explorer: {yaml}"
            );
            // Match the quoted command token, not a bare substring — the template
            // comment mentions `--no-studio` even when the flag itself is absent.
            assert!(
                !yaml.contains("\"--no-studio\""),
                "studio on must not also disable it: {yaml}"
            );
            assert!(
                yaml.contains("\"18899:18488\""),
                "the Studio port must be published on the host: {yaml}"
            );
        }
        // The manifest advertises the published Studio host port (served at root)…
        assert_eq!(
            e.manifest_entry().explorer.as_deref(),
            Some("http://127.0.0.1:18899")
        );
        // …the collision check sees that extra port (alongside the always-on WS
        // port on RPC + 1)…
        assert_eq!(e.extra_host_ports(), vec![8900, 18899]);
        // …and Studio is in-process, so no Otterscan pairing is requested.
        assert!(e.explorer_target().is_none());
    }

    #[test]
    fn studio_disabled_by_default_omits_the_port_and_the_explorer() {
        let e = SolanaEngine::surfpool("solana-1", 8899);
        let yaml = e.compose_service(StateMode::Ephemeral);
        assert!(yaml.contains("\"--no-studio\""));
        // Only the RPC port is published; no second Studio mapping.
        assert!(
            !yaml.contains("18899"),
            "studio off must not publish a Studio port: {yaml}"
        );
        assert!(e.manifest_entry().explorer.is_none());
        // The WS port is always published, so it's the sole extra port here.
        assert_eq!(e.extra_host_ports(), vec![8900]);
    }

    #[test]
    fn ws_rpc_is_always_served_published_and_advertised() {
        // WS is core RPC, not gated by the explorer flag — present with Studio on
        // or off, in either state mode, on the RPC host port + 1.
        for studio in [false, true] {
            let e = SolanaEngine::surfpool("solana-1", 8899).studio(studio);
            for mode in [StateMode::Ephemeral, StateMode::Persistent] {
                let yaml = e.compose_service(mode);
                assert!(
                    yaml.contains("\"--ws-port\", \"8900\""),
                    "surfpool must serve WS: {yaml}"
                );
                assert!(
                    yaml.contains("\"8900:8900\""),
                    "the WS port must be published on the host (RPC 8899 → 8900): {yaml}"
                );
            }
            // Advertised in the manifest and surfaced to the collision check.
            assert_eq!(
                e.manifest_entry().ws.as_deref(),
                Some("ws://127.0.0.1:8900")
            );
            assert!(e.extra_host_ports().contains(&8900));
        }
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

    #[test]
    fn forked_compose_uses_rpc_url_instead_of_offline() {
        let yaml = SolanaEngine::surfpool("sol-fork", 8899)
            .fork("https://rpc.example/key".to_string())
            .compose_service(StateMode::Ephemeral);
        assert!(yaml.contains("\"--rpc-url\", \"https://rpc.example/key\""));
        // Check the quoted command token, not a bare substring — the template
        // comment mentions `--offline` even when the flag itself is absent.
        assert!(
            !yaml.contains("\"--offline\""),
            "a forked chain must not run offline: {yaml}"
        );
        // Dev accounts are still airdropped over the fork.
        assert!(yaml.contains("\"--airdrop\""));
        assert!(!yaml.contains("{{"), "no placeholder should remain: {yaml}");
    }

    #[test]
    fn forked_manifest_redacts_the_url_and_drops_baked_tokens() {
        let entry = SolanaEngine::surfpool("sol-fork", 8899)
            .fork("https://api.example.com/rpc/SECRET".to_string())
            .manifest_entry();
        // The RPC key is never recorded; a Solana fork is never block-pinned, so
        // it's always described as tracking the datasource's live state.
        assert_eq!(
            entry.fork.as_deref(),
            Some("https://api.example.com @ latest")
        );
        // A fork mirrors live state, so it advertises none of the baked tokens —
        // but the predeployed dev accounts still apply over the fork.
        assert!(entry.tokens.is_empty());
        assert_eq!(entry.accounts.len(), 3);
    }

    // ---- docker-backed end-to-end run against a live surfpool ----
    //
    // Boots a real surfpool chain and checks the two facts the engine relies on:
    // that the getHealth probe passes (so `up` waited on the right endpoint), and
    // that every baked dev account is funded by the boot airdrop. Self-skips
    // without Docker.

    use crate::harness::{Localnet, docker_available};
    use crate::runtime::manifest::Manifest;
    use crate::runtime::orchestrator::manifest_path;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    /// A dedicated high port, away from the other e2e ports, so this can run in
    /// parallel with the EVM and Starknet docker tests. Solana e2e ports are
    /// spaced 10 apart: each chain now occupies its RPC port *and* RPC + 1 (WS),
    /// so adjacent slots must not overlap.
    const SOL_E2E_PORT: u16 = 18900;

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

    /// Minimal HTTP GET → `(status_code, body)`, for hitting the Studio page.
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

    /// A dedicated high port for the Studio-explorer e2e, distinct from the other
    /// Solana e2e ports so it can run in parallel. Its Studio lands on +10000.
    const SOL_UI_E2E_PORT: u16 = 18910;

    #[test]
    fn solana_surfpool_serves_the_studio_explorer() {
        if !docker_available() {
            eprintln!("skipping solana studio e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_solana_ui("t-solana-ui", SOL_UI_E2E_PORT);

        // With the explorer on, surfpool serves Studio on its own published host
        // port (RPC port + 10000), at the root — a separate service, not the RPC
        // port like the Starknet UI.
        let studio_port = SOL_UI_E2E_PORT + 10000;
        let (status, body) = http_get(studio_port, "/");
        assert_eq!(
            status, 200,
            "GET / on the Studio port should serve the page"
        );
        assert!(
            body.to_lowercase().contains("<!doctype html"),
            "Studio should return an HTML page, got: {}",
            &body[..body.len().min(200)]
        );

        // ...and the manifest advertises exactly that URL, so tooling can find it.
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        assert_eq!(
            manifest.chains[0].explorer.as_deref(),
            Some(format!("http://127.0.0.1:{studio_port}").as_str())
        );
    }

    /// A dedicated high port for the WebSocket e2e, distinct from the other
    /// Solana e2e ports so it can run in parallel. Its WS lands on +1 (18921).
    const SOL_WS_E2E_PORT: u16 = 18920;

    /// Open a WebSocket to `port`, send one JSON-RPC `method` (no params), and
    /// return `(handshake_status_line, first_text_frame_payload)`. A minimal
    /// hand-rolled client (RFC 6455): a fixed `Sec-WebSocket-Key` and a single
    /// masked text frame are enough — we don't validate the server's accept hash,
    /// only that it upgrades and answers the subscription.
    fn ws_call(port: u16, method: &str) -> (String, String) {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        // Upgrade handshake. The key is the canonical RFC 6455 example value —
        // any base64 of 16 bytes works, and we don't check the accept response.
        let req = format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).unwrap();

        // Read until the header terminator, keeping any bytes of the first data
        // frame that arrive in the same segment.
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        let header_end = loop {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0, "connection closed during WS handshake");
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let status_line = String::from_utf8_lossy(&buf[..buf.len().min(64)])
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        // Send the request as a masked client text frame (payload < 126 bytes).
        let payload = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}"}}"#).into_bytes();
        let mask = [0x12u8, 0x34, 0x56, 0x78];
        let mut frame = vec![0x81, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        stream.write_all(&frame).unwrap();

        // Read the server's reply frame (unmasked) — reuse any leftover bytes
        // past the handshake, then top up from the socket.
        let mut data = buf[header_end..].to_vec();
        while data.len() < 2 {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0, "connection closed before WS reply");
            data.extend_from_slice(&tmp[..n]);
        }
        let len = (data[1] & 0x7f) as usize;
        while data.len() < 2 + len {
            let n = stream.read(&mut tmp).unwrap();
            assert!(n > 0, "connection closed mid WS frame");
            data.extend_from_slice(&tmp[..n]);
        }
        let reply = String::from_utf8_lossy(&data[2..2 + len]).to_string();
        (status_line, reply)
    }

    #[test]
    fn solana_surfpool_serves_the_websocket_rpc() {
        if !docker_available() {
            eprintln!("skipping solana ws e2e: docker unavailable");
            return;
        }
        let net = Localnet::boot_solana("t-solana-ws", SOL_WS_E2E_PORT);

        // surfpool publishes WS on the RPC host port + 1. A real client upgrade +
        // subscription proves the port is published and subscriptions work from
        // the host — the whole point of exposing 8900.
        let ws_port = SOL_WS_E2E_PORT + 1;
        let (status, reply) = ws_call(ws_port, "slotSubscribe");
        assert!(
            status.contains("101"),
            "WS upgrade should return 101 Switching Protocols, got: {status}"
        );
        // slotSubscribe returns a numeric subscription id under `result`.
        assert!(
            reply.contains("\"result\""),
            "slotSubscribe should return a subscription id, got: {reply}"
        );

        // The manifest advertises the WS endpoint at that port.
        let manifest = Manifest::read(&manifest_path(net.base())).unwrap();
        assert_eq!(
            manifest.chains[0].ws.as_deref(),
            Some(format!("ws://127.0.0.1:{ws_port}").as_str())
        );
    }

    /// Dedicated ports for the forking e2e: an origin chain and the chain that
    /// forks it. Distinct from the other Solana e2e ports so they run in parallel.
    const SOL_FORK_ORIGIN_PORT: u16 = 18930;
    const SOL_FORK_CHILD_PORT: u16 = 18940;

    #[test]
    fn solana_chain_can_fork_a_live_network() {
        if !docker_available() {
            eprintln!("skipping solana fork e2e: docker unavailable");
            return;
        }
        // Origin: a full wharfnet Solana chain, so it already has the baked USDC
        // seeded and the dev accounts funded — concrete state the fork should see.
        // Bound to `_origin` so it stays up (and tears down on drop).
        let _origin = Localnet::boot_solana("t-sol-fork-origin", SOL_FORK_ORIGIN_PORT);
        let origin_manifest = Manifest::read(&manifest_path(_origin.base())).unwrap();
        let usdc = origin_manifest.chains[0]
            .tokens
            .iter()
            .find(|t| t.symbol == "USDC")
            .unwrap()
            .address
            .clone();
        let dev0 = origin_manifest.chains[0].accounts[0].address.clone();

        // Child: a wharfnet Solana chain forking the origin's RPC. Inside the
        // container the origin is reachable on the host via host.docker.internal.
        let child = Localnet::boot_solana_fork(
            "t-sol-fork-child",
            SOL_FORK_CHILD_PORT,
            &format!("http://host.docker.internal:{SOL_FORK_ORIGIN_PORT}"),
        );

        // The fork mirrors the origin's baked USDC mint (copy-on-read)...
        let mint_info = rpc(
            SOL_FORK_CHILD_PORT,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["{usdc}",{{"encoding":"jsonParsed"}}]}}"#
            ),
        );
        let parsed: serde_json::Value = serde_json::from_str(&mint_info).unwrap();
        assert_eq!(
            parsed["result"]["value"]["data"]["parsed"]["type"], "mint",
            "fork should see the origin's USDC mint: {mint_info}"
        );

        // ...and the origin's funded dev account.
        let bal = rpc(
            SOL_FORK_CHILD_PORT,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{dev0}"]}}"#),
        );
        let parsed: serde_json::Value = serde_json::from_str(&bal).unwrap();
        assert!(
            parsed["result"]["value"].as_u64().unwrap() >= AIRDROP_LAMPORTS,
            "fork should mirror the origin's funded dev account: {bal}"
        );

        // The manifest records the fork (redacted) and, since a fork mirrors live
        // state, advertises none of wharfnet's baked tokens.
        let manifest = Manifest::read(&manifest_path(child.base())).unwrap();
        let chain = &manifest.chains[0];
        assert_eq!(
            chain.fork.as_deref(),
            Some(format!("http://host.docker.internal:{SOL_FORK_ORIGIN_PORT} @ latest").as_str())
        );
        assert!(chain.tokens.is_empty(), "a fork advertises no baked tokens");
    }

    /// A dedicated port for the persistence cycle, away from the other e2e ports.
    const SOL_PERSIST_PORT: u16 = 18950;

    /// End-to-end persistence: a faucet top-up survives `down` → `up --resume`
    /// (surfpool restores its SQLite surfnet db), and `up --reset` discards it.
    /// This is the proof `--resume`/`--reset` work on Solana. Runs several boot
    /// cycles, so it's the heaviest Solana test; self-skips without Docker.
    #[test]
    fn solana_faucet_funding_survives_down_and_up_resume() {
        if !docker_available() {
            eprintln!("skipping solana persistence e2e: docker unavailable");
            return;
        }
        use crate::runtime::orchestrator::{self, UpMode};

        let dir = tempfile::TempDir::new_in(".").expect("temp dir under crate root");
        let base = dir.path();
        let project = "wharfnet-e2e-sol-persist";
        let chain = "sol-persist";
        let config = base.join("wharfnet.toml");
        std::fs::write(
            &config,
            format!(
                "[[chains]]\nname = \"{chain}\"\nkind = \"solana\"\nport = {SOL_PERSIST_PORT}\n"
            ),
        )
        .unwrap();

        // Tear the containers down even if an assertion panics. Declared after
        // `dir` so it drops first — down_in still sees the compose file.
        struct Teardown<'a>(&'a std::path::Path, &'a str);
        impl Drop for Teardown<'_> {
            fn drop(&mut self) {
                let _ = crate::runtime::orchestrator::down_in(self.0, self.1);
            }
        }
        let _guard = Teardown(base, project);

        // A fresh, non-dev recipient — its funding comes only from the faucet, so
        // it's purely runtime state the surfnet db must persist (the dev accounts
        // are re-airdropped at boot, so they're not a clean persistence signal).
        let recipient = "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM";
        let usdc = "94C6wFGeVr5SahK9owBMBhpFPRtvLuZhQQVRh7NYrEp9";
        let usdc_balance = || -> u64 {
            let body = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"getTokenAccountsByOwner","params":["{recipient}",{{"mint":"{usdc}"}},{{"encoding":"jsonParsed"}}]}}"#
            );
            let resp = rpc(SOL_PERSIST_PORT, &body);
            let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
            v["result"]["value"][0]["account"]["data"]["parsed"]["info"]["tokenAmount"]["amount"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap()
        };

        // 1) First `up --resume`: fresh db, tokens seeded; fund the recipient.
        orchestrator::up_in(base, project, UpMode::Resume, false, Some(&config))
            .expect("first up --resume should boot");
        crate::faucet::run_in(base, project, chain, recipient, "7", Some("USDC"), false)
            .expect("funding USDC should succeed");
        assert_eq!(usdc_balance(), 7_000_000);

        // 2) Tear down (the bind-mounted db survives on the host) and resume:
        //    surfpool restores the surfnet, so the funding is back.
        orchestrator::down_in(base, project).expect("down should succeed");
        orchestrator::up_in(base, project, UpMode::Resume, false, Some(&config))
            .expect("second up --resume should boot");
        assert_eq!(
            usdc_balance(),
            7_000_000,
            "the USDC funding must survive down → up --resume"
        );

        // 3) `up --reset` discards the db and boots fresh — the funding is gone
        //    (the recipient was never airdropped, only faucet-funded).
        orchestrator::down_in(base, project).expect("down before reset should succeed");
        orchestrator::up_in(base, project, UpMode::Reset, false, Some(&config))
            .expect("up --reset should boot");
        assert_eq!(
            usdc_balance(),
            0,
            "up --reset must discard the persisted funding"
        );
    }
}

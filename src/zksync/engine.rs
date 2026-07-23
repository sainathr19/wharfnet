//! The anvil-zksync-backed zkSync engine: the [`Engine`] impl for a local zkSync
//! chain.
//!
//! Compose service YAML lives in `src/resources/docker/services/` and is
//! embedded into the binary at compile time — edit that template, not Rust
//! strings.

use crate::runtime::engine::{Engine, HealthProbe, StateMode};
use crate::runtime::manifest::{Account, ChainEntry};

/// Internal port the node listens on inside its container, irrespective of the
/// published host port. anvil-zksync's own default is 8011; the control/faucet
/// clients reach the node on the published host port, so this only needs to
/// match the `--port` baked into the compose command.
pub(crate) const ZKSYNC_INTERNAL_PORT: u16 = 8011;

/// anvil-zksync's default chain id, used when a `wharfnet.toml` chain omits one.
pub(crate) const ZKSYNC_DEFAULT_CHAIN_ID: u64 = 260;

/// How often the node flushes its state to disk in persistent mode (seconds). A
/// small interval keeps the on-disk session close to live so little is lost if
/// the process is killed rather than shut down gracefully.
const ZKSYNC_STATE_INTERVAL_SECS: u16 = 5;

/// Pinned anvil-zksync image. Ubuntu-based, entrypoint `anvil-zksync`, serving on
/// 8011 — see `resources/docker/services/zksync.yml`.
const ZKSYNC_IMAGE: &str = "ghcr.io/matter-labs/anvil-zksync:v0.6.11";

/// Compose service template for an anvil-zksync-backed zkSync chain.
const ZKSYNC_SERVICE_TEMPLATE: &str = include_str!("../resources/docker/services/zksync.yml");

/// Fork settings for a zkSync chain: mirror a live network's state from `url`,
/// optionally pinned to a block height.
struct Fork {
    url: String,
    block: Option<u64>,
}

impl Fork {
    /// A redacted description safe to record and print — the RPC key is dropped.
    fn describe(&self) -> String {
        crate::runtime::fork::describe(&self.url, self.block)
    }
}

/// An anvil-zksync-backed zkSync chain.
pub struct ZkSyncEngine {
    name: String,
    image: String,
    host_port: u16,
    chain_id: u64,
    fork: Option<Fork>,
}

impl ZkSyncEngine {
    pub fn new(name: &str, host_port: u16, chain_id: u64) -> Self {
        ZkSyncEngine {
            name: name.to_string(),
            image: ZKSYNC_IMAGE.to_string(),
            host_port,
            chain_id,
            fork: None,
        }
    }

    /// Fork this chain from a live zkSync RPC, optionally pinned to a block.
    pub fn fork(mut self, url: String, block: Option<u64>) -> Self {
        self.fork = Some(Fork { url, block });
        self
    }

    /// Path (relative to the state dir) of this chain's persistent session
    /// snapshot. Named `session-<chain>.json` so the orchestrator's session
    /// tracking (resume/reset) picks it up like the Anvil/devnet snapshots.
    fn session_rel_path(&self) -> String {
        format!("state/session-{}.json", self.name)
    }

    /// Funded dev accounts. anvil-zksync funds the standard Anvil test-mnemonic
    /// accounts by default (mnemonic "test test test test test test test test
    /// test test test junk"), so these are identical to the EVM chains' accounts
    /// — well-known throwaway keys, safe to publish, never for real funds.
    fn accounts() -> Vec<Account> {
        vec![
            Account {
                address: "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".to_string(),
                private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
                    .to_string(),
                balance: "10000 ETH".to_string(),
            },
            Account {
                address: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_string(),
                private_key: "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
                    .to_string(),
                balance: "10000 ETH".to_string(),
            },
            Account {
                address: "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC".to_string(),
                private_key: "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
                    .to_string(),
                balance: "10000 ETH".to_string(),
            },
        ]
    }
}

impl Engine for ZkSyncEngine {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn host_port(&self) -> u16 {
        self.host_port
    }

    fn compose_service(&self, mode: StateMode) -> String {
        // Ephemeral: start fresh in-memory every boot (no baked token snapshot
        // yet). Persistent: `--state` loads the session snapshot if present and
        // dumps back to it on exit and every interval, so a crash-restart resumes.
        // Each optional arg group is leading-comma so it can collapse to nothing.
        let state_args = match mode {
            StateMode::Ephemeral => String::new(),
            StateMode::Persistent => format!(
                r#", "--state", "/{session}", "--state-interval", "{interval}""#,
                session = self.session_rel_path(),
                interval = ZKSYNC_STATE_INTERVAL_SECS,
            ),
        };
        // Global options (--port/--chain-id/--state) precede the subcommand; a
        // plain chain boots with `run`, a fork with `fork --fork-url …`.
        let (subcommand, fork_args) = match &self.fork {
            None => ("run", String::new()),
            Some(fork) => {
                let mut a = format!(r#", "--fork-url", "{}""#, fork.url);
                if let Some(block) = fork.block {
                    a.push_str(&format!(r#", "--fork-block-number", "{block}""#));
                }
                ("fork", a)
            }
        };
        ZKSYNC_SERVICE_TEMPLATE
            .replace("{{NAME}}", &self.name)
            .replace("{{IMAGE}}", &self.image)
            .replace("{{PORT}}", &ZKSYNC_INTERNAL_PORT.to_string())
            .replace("{{CHAIN_ID}}", &self.chain_id.to_string())
            .replace("{{HOST_PORT}}", &self.host_port.to_string())
            .replace("{{STATE_ARGS}}", &state_args)
            .replace("{{SUBCOMMAND}}", subcommand)
            .replace("{{FORK_ARGS}}", &fork_args)
    }

    fn manifest_entry(&self) -> ChainEntry {
        ChainEntry {
            name: self.name.clone(),
            kind: "zksync".to_string(),
            rpc: format!("http://127.0.0.1:{}", self.host_port),
            // anvil-zksync serves WS on the same port as HTTP; nothing distinct
            // to advertise.
            ws: None,
            chain_id: self.chain_id.to_string(),
            accounts: Self::accounts(),
            // No bundled test tokens or infra contracts yet — the chain boots with
            // just the funded native-coin accounts above.
            tokens: Vec::new(),
            contracts: Vec::new(),
            fork: self.fork.as_ref().map(Fork::describe),
            explorer: None,
        }
    }

    fn health_probe(&self) -> HealthProbe {
        // anvil-zksync answers the eth JSON-RPC namespace; `eth_chainId` is the
        // cheapest readiness ping, same as Anvil.
        HealthProbe::JsonRpc {
            method: "eth_chainId",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_sets_fields() {
        let e = ZkSyncEngine::new("zksync-1", 8011, 260);
        assert_eq!(e.name(), "zksync-1");
        assert_eq!(e.host_port(), 8011);
        assert_eq!(e.image, ZKSYNC_IMAGE);
        assert_eq!(e.chain_id, 260);
        assert!(e.fork.is_none());
    }

    #[test]
    fn compose_service_substitutes_every_placeholder() {
        for mode in [StateMode::Ephemeral, StateMode::Persistent] {
            let yaml = ZkSyncEngine::new("zksync-2", 8012, 300).compose_service(mode);
            assert!(yaml.contains("zksync-2:"));
            assert!(yaml.contains(ZKSYNC_IMAGE));
            assert!(yaml.contains("\"--chain-id\", \"300\""));
            assert!(yaml.contains("\"8012:8011\""));
            // Global options precede the subcommand token.
            assert!(yaml.contains("\"run\""));
            assert!(!yaml.contains("{{"), "no placeholder should remain: {yaml}");
            assert!(!yaml.contains("}}"), "no placeholder should remain: {yaml}");
        }
    }

    #[test]
    fn ephemeral_compose_boots_run_without_state() {
        let yaml = ZkSyncEngine::new("zksync-1", 8011, 260).compose_service(StateMode::Ephemeral);
        assert!(yaml.contains("\"run\""));
        // Match the quoted arg token, not the word in the template comment.
        assert!(
            !yaml.contains("\"--state\""),
            "ephemeral must not persist state: {yaml}"
        );
        // The whole state dir is mounted so persistent runs can write dumps.
        assert!(yaml.contains("./state:/state"));
    }

    #[test]
    fn persistent_compose_loads_and_dumps_a_session_snapshot() {
        let yaml = ZkSyncEngine::new("zksync-1", 8011, 260).compose_service(StateMode::Persistent);
        assert!(yaml.contains("\"--state\", \"/state/session-zksync-1.json\""));
        assert!(yaml.contains("\"--state-interval\", \"5\""));
        assert!(yaml.contains("\"run\""));
    }

    #[test]
    fn forked_compose_uses_the_fork_subcommand_and_pins_the_block() {
        let yaml = ZkSyncEngine::new("zk-fork", 8011, 260)
            .fork("https://rpc.example/key".to_string(), Some(12345))
            .compose_service(StateMode::Ephemeral);
        assert!(yaml.contains("\"fork\""));
        assert!(yaml.contains("\"--fork-url\", \"https://rpc.example/key\""));
        assert!(yaml.contains("\"--fork-block-number\", \"12345\""));
        assert!(
            !yaml.contains("\"run\""),
            "a forked chain boots via `fork`, not `run`: {yaml}"
        );
        assert!(!yaml.contains("{{"), "no placeholder should remain: {yaml}");
    }

    #[test]
    fn forked_compose_without_a_block_omits_the_pin() {
        let yaml = ZkSyncEngine::new("zk-fork", 8011, 260)
            .fork("https://rpc.example/key".to_string(), None)
            .compose_service(StateMode::Persistent);
        assert!(yaml.contains("\"--fork-url\", \"https://rpc.example/key\""));
        assert!(!yaml.contains("--fork-block-number"), "no block was pinned");
        // Persistence still layers on top of the fork.
        assert!(yaml.contains("\"--state\", \"/state/session-zk-fork.json\""));
    }

    #[test]
    fn manifest_entry_describes_the_chain() {
        let entry = ZkSyncEngine::new("zksync-1", 8011, 260).manifest_entry();
        assert_eq!(entry.kind, "zksync");
        assert_eq!(entry.rpc, "http://127.0.0.1:8011");
        assert_eq!(entry.chain_id, "260");
        assert!(entry.ws.is_none());
        assert!(entry.fork.is_none());
        assert!(entry.explorer.is_none());
        // Native-only for now: funded accounts, no baked tokens or contracts.
        assert!(entry.tokens.is_empty());
        assert!(entry.contracts.is_empty());
    }

    #[test]
    fn manifest_entry_lists_the_standard_dev_accounts() {
        let entry = ZkSyncEngine::new("zksync-1", 8011, 260).manifest_entry();
        assert_eq!(entry.accounts.len(), 3);
        // Account 0 is the standard Anvil test-mnemonic account.
        assert_eq!(
            entry.accounts[0].address,
            "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
        );
        assert_eq!(
            entry.accounts[0].private_key,
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
        );
        assert_eq!(entry.accounts[0].balance, "10000 ETH");
    }

    #[test]
    fn forked_manifest_entry_redacts_the_url_but_keeps_dev_accounts() {
        let entry = ZkSyncEngine::new("zk-fork", 8011, 260)
            .fork("https://era.example.com/v2/SECRET".to_string(), Some(99))
            .manifest_entry();
        // The RPC key is never recorded.
        assert_eq!(
            entry.fork.as_deref(),
            Some("https://era.example.com @ block 99")
        );
        // anvil-zksync funds the dev accounts over the fork, so they still apply.
        assert_eq!(entry.accounts.len(), 3);
    }

    #[test]
    fn health_probe_is_eth_chain_id() {
        let probe = ZkSyncEngine::new("zksync-1", 8011, 260).health_probe();
        assert!(matches!(
            probe,
            HealthProbe::JsonRpc {
                method: "eth_chainId"
            }
        ));
    }

    #[test]
    fn no_explorer_target_yet() {
        // Otterscan speaks EVM bytecode, not EraVM, so no bundled explorer.
        assert!(
            ZkSyncEngine::new("zksync-1", 8011, 260)
                .explorer_target()
                .is_none()
        );
    }
}

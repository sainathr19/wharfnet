//! Chain engines. Each engine knows how to (a) describe itself as a
//! docker-compose service and (b) describe how to reach it in the manifest.
//!
//! wharfnet does not implement chains — it wraps best-in-class engines:
//! Anvil for EVM (here), with Solana (solana-test-validator/Surfpool) and
//! Starknet (starknet-devnet-rs) landing in M2 as additional `Engine` impls.
//!
//! Compose service YAML lives in `src/resources/docker/services/` and is
//! embedded into the binary at compile time — edit those templates, not Rust
//! strings.

use crate::manifest::{Account, ChainEntry};

/// Internal port the engine listens on inside its container.
const ANVIL_INTERNAL_PORT: u16 = 8545;

/// Compose service template for an Anvil-backed EVM chain.
const ANVIL_SERVICE_TEMPLATE: &str = include_str!("resources/docker/services/anvil.yml");

pub trait Engine {
    /// Service / container name, e.g. "anvil-1".
    fn name(&self) -> String;
    /// The host port the RPC is published on (used for health checks).
    fn host_port(&self) -> u16;
    /// A docker-compose service fragment, indented two spaces under `services:`.
    fn compose_service(&self) -> String;
    /// How to reach this chain, for the manifest.
    fn manifest_entry(&self) -> ChainEntry;
}

/// An Anvil-backed EVM chain.
pub struct EvmEngine {
    name: String,
    image: String,
    host_port: u16,
    chain_id: u64,
}

impl EvmEngine {
    pub fn anvil(name: &str, host_port: u16, chain_id: u64) -> Self {
        EvmEngine {
            name: name.to_string(),
            image: "ghcr.io/foundry-rs/foundry:stable".to_string(),
            host_port,
            chain_id,
        }
    }

    /// Standard deterministic Anvil dev accounts (mnemonic:
    /// "test test test test test test test test test test test junk").
    /// These are well-known throwaway keys — safe to publish, never use with
    /// real funds.
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

impl Engine for EvmEngine {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn host_port(&self) -> u16 {
        self.host_port
    }

    fn compose_service(&self) -> String {
        ANVIL_SERVICE_TEMPLATE
            .replace("{{NAME}}", &self.name)
            .replace("{{IMAGE}}", &self.image)
            .replace("{{PORT}}", &ANVIL_INTERNAL_PORT.to_string())
            .replace("{{CHAIN_ID}}", &self.chain_id.to_string())
            .replace("{{HOST_PORT}}", &self.host_port.to_string())
    }

    fn manifest_entry(&self) -> ChainEntry {
        ChainEntry {
            name: self.name.clone(),
            kind: "evm".to_string(),
            rpc: format!("http://127.0.0.1:{}", self.host_port),
            chain_id: self.chain_id,
            accounts: Self::accounts(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anvil_constructor_sets_fields() {
        let e = EvmEngine::anvil("anvil-1", 8545, 31337);
        assert_eq!(e.name(), "anvil-1");
        assert_eq!(e.host_port(), 8545);
        assert_eq!(e.image, "ghcr.io/foundry-rs/foundry:stable");
        assert_eq!(e.chain_id, 31337);
    }

    #[test]
    fn compose_service_substitutes_every_placeholder() {
        let yaml = EvmEngine::anvil("anvil-2", 8546, 31338).compose_service();
        assert!(yaml.contains("anvil-2:"));
        assert!(yaml.contains("ghcr.io/foundry-rs/foundry:stable"));
        assert!(yaml.contains("\"--chain-id\", \"31338\""));
        assert!(yaml.contains("\"8546:8545\""));
        assert!(!yaml.contains("{{"), "no placeholder should remain: {yaml}");
        assert!(!yaml.contains("}}"), "no placeholder should remain: {yaml}");
    }

    #[test]
    fn manifest_entry_describes_the_chain() {
        let entry = EvmEngine::anvil("anvil-1", 8545, 31337).manifest_entry();
        assert_eq!(entry.kind, "evm");
        assert_eq!(entry.rpc, "http://127.0.0.1:8545");
        assert_eq!(entry.chain_id, 31337);
        assert_eq!(entry.accounts.len(), 3);
        assert_eq!(
            entry.accounts[0].address,
            "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
        );
    }
}

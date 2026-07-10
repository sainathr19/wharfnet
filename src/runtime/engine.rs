//! The chain-engine abstraction. An [`Engine`] knows how to (a) describe itself
//! as a docker-compose service and (b) describe how to reach it in the manifest;
//! the orchestrator drives a set of them without caring which chain kind each is.
//!
//! wharfnet does not implement chains — it wraps best-in-class engines: Anvil for
//! EVM (see [`crate::evm::engine`]), with Solana (solana-test-validator/Surfpool)
//! and Starknet (starknet-devnet-rs) landing as further `Engine` impls.

use super::manifest::ChainEntry;

/// Whether a chain boots fresh every time or resumes (and keeps saving) its
/// runtime state across restarts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StateMode {
    /// Load the baked snapshot and never dump — deterministic, disposable.
    /// Every boot starts from the same pre-deployed tokens and seeded accounts.
    Ephemeral,
    /// Load from (and continuously dump to) a per-chain session snapshot, so
    /// balances, transactions, and deployments survive `down` → `up --resume`.
    Persistent,
}

/// How to point a bundled block explorer at a chain. The browser talks to the
/// chain directly, so it needs the chain's *published host* RPC port.
pub struct ExplorerTarget {
    pub chain_name: String,
    pub rpc_host_port: u16,
}

/// How the orchestrator decides a chain's RPC is ready. Different engines speak
/// different protocols — an EVM node answers JSON-RPC, Starknet-devnet exposes a
/// plain HTTP liveness endpoint — so each engine describes its own probe.
pub enum HealthProbe {
    /// POST a JSON-RPC request calling `method` and consider the chain ready once
    /// the response carries a `result` (used by EVM chains, e.g. `eth_chainId`).
    JsonRpc { method: &'static str },
    /// GET `path` and consider the chain ready on an HTTP `200` (used by
    /// Starknet-devnet's `/is_alive`).
    HttpGet { path: &'static str },
}

/// A file an engine needs written under the state dir before its container
/// boots (e.g. a chain snapshot mounted into the container).
pub struct StagedFile {
    /// Path relative to the state dir; matches the compose volume source.
    pub rel_path: String,
    pub contents: &'static str,
    /// When true, write only if the destination does not already exist. Used to
    /// seed a session snapshot without ever clobbering saved state.
    pub if_absent: bool,
}

pub trait Engine {
    /// Service / container name, e.g. "anvil-1".
    fn name(&self) -> String;
    /// The host port the RPC is published on (used for health checks).
    fn host_port(&self) -> u16;
    /// A docker-compose service fragment, indented two spaces under `services:`.
    fn compose_service(&self, mode: StateMode) -> String;
    /// How to reach this chain, for the manifest.
    fn manifest_entry(&self) -> ChainEntry;
    /// How to check this chain's RPC is up, once its container is running.
    fn health_probe(&self) -> HealthProbe;
    /// Files to write under the state dir before boot. Defaults to none.
    fn staged_files(&self, _mode: StateMode) -> Vec<StagedFile> {
        Vec::new()
    }
    /// How to pair this chain with an Otterscan explorer, if it supports one.
    /// Defaults to `None` (no explorer).
    fn explorer_target(&self) -> Option<ExplorerTarget> {
        None
    }
}

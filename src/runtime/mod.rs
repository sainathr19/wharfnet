//! The chain-agnostic framework: the localnet lifecycle (orchestration) plus the
//! supporting plumbing every chain kind shares — config, the endpoints manifest,
//! the Docker wrapper, and terminal UI. Nothing in here is EVM-specific; the
//! per-kind pieces live under `crate::evm` (and future `crate::solana`, …).

pub mod amount;
pub mod config;
pub mod docker;
pub mod engine;
pub mod fork;
pub mod manifest;
pub mod orchestrator;
pub mod ui;

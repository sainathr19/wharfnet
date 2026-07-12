//! Solana chain support: the [`engine`] implements the [`Engine`] trait for a
//! `surfpool` chain (boot, endpoints, readiness).
//!
//! surfpool runs an in-memory SVM ("surfnet") that boots in about a second and
//! speaks the standard Solana JSON-RPC, plus `surfnet_*` cheatcodes. The faucet,
//! forking, persistence, and chain-control pieces land in later work, mirroring
//! the EVM and Starknet stacks.
//!
//! [`Engine`]: crate::runtime::engine::Engine

pub mod engine;

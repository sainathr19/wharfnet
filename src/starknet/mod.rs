//! Starknet chain support: the [`engine`] implements the [`Engine`] trait for a
//! `starknet-devnet` chain (boot, endpoints, readiness). Faucet and chain
//! control land in later steps.
//!
//! [`Engine`]: crate::runtime::engine::Engine

pub mod engine;

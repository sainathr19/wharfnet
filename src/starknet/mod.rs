//! Starknet chain support: the [`engine`] implements the [`Engine`] trait for a
//! `starknet-devnet` chain (boot, endpoints, readiness), and the [`faucet`] funds
//! addresses on a running chain (ETH/STRK via devnet's mint cheat, the Cairo test
//! tokens via signed invokes). Chain control lands in a later step.
//!
//! [`Engine`]: crate::runtime::engine::Engine

pub mod engine;
pub mod faucet;

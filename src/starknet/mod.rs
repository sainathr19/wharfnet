//! Starknet chain support: the [`engine`] implements the [`Engine`] trait for a
//! `starknet-devnet` chain (boot, endpoints, readiness), the [`faucet`] funds
//! addresses on a running chain (ETH/STRK via devnet's mint cheat, the Cairo test
//! tokens via signed invokes), and [`control`] drives a running chain through
//! devnet's cheat JSON-RPC (create blocks, advance time, impersonate). Both the
//! faucet and control talk to devnet through the shared [`devnet`] client.
//!
//! [`Engine`]: crate::runtime::engine::Engine

pub mod control;
pub mod devnet;
pub mod engine;
pub mod faucet;

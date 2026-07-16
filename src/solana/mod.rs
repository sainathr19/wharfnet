//! Solana chain support: the [`engine`] implements the [`Engine`] trait for a
//! `surfpool` chain (boot, endpoints, readiness), and [`control`] drives a
//! running chain through surfpool's `surfnet_*` cheat JSON-RPC (advance slots,
//! travel through time, freeze/resume the clock) via the shared [`rpc`] client.
//!
//! surfpool runs an in-memory SVM ("surfnet") that boots in about a second and
//! speaks the standard Solana JSON-RPC, plus `surfnet_*` cheatcodes. The faucet,
//! forking, and persistence pieces land in later work, mirroring the EVM and
//! Starknet stacks.
//!
//! [`Engine`]: crate::runtime::engine::Engine

pub mod control;
pub mod engine;
pub mod faucet;
pub mod rpc;
pub mod tokens;

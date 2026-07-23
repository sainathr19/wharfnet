//! The zkSync chain kind: an [`anvil-zksync`](engine)-backed [`Engine`] plus the
//! helpers and commands that drive a running zkSync chain.
//!
//! [`anvil-zksync`](https://github.com/matter-labs/anvil-zksync) is Matter Labs'
//! in-memory zkSync node — the EraVM analogue of Anvil. It speaks an
//! Anvil-compatible JSON-RPC (the `evm_*`/`anvil_*` cheat methods), but its image
//! ships only the node binary — no `cast` — so [control](control) and the
//! [faucet](faucet) drive it over JSON-RPC through the shared [`rpc`] client
//! rather than by exec-ing a CLI inside the container (the EVM approach).
//!
//! [`Engine`]: crate::runtime::engine::Engine

pub mod control;
pub mod engine;
pub mod faucet;
pub mod rpc;

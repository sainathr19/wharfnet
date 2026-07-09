//! The EVM chain kind: an Anvil-backed [`engine`](engine) plus the helpers and
//! commands that drive a running EVM chain — [`session`](session) (cast / address
//! validation), the [`faucet`](faucet), and the [`control`](control) cheat-RPC
//! verbs exposed under `wharfnet evm …`.

pub mod control;
pub mod engine;
pub mod faucet;
pub mod session;

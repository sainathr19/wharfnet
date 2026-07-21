//! Bitcoin & Litecoin support: the [`engine`] implements the [`Engine`] trait for
//! a bitcoind/litecoind regtest chain (boot, endpoints, readiness, and a wallet
//! funded at boot), and [`control`] drives a running chain (mine blocks on demand)
//! via the shared [`rpc`] client. [`faucet`] sends native coin to an address.
//!
//! Litecoin is a Bitcoin fork with an identical JSON-RPC, so one engine — the
//! [`UtxoEngine`](engine::UtxoEngine), parameterized by a [`Coin`](engine::Coin) —
//! serves both kinds. UTXO chains run ephemerally (in-container datadir); the
//! faucet/control talk to the published RPC directly with fixed dev credentials.
//!
//! [`Engine`]: crate::runtime::engine::Engine

pub mod control;
pub mod engine;
pub mod faucet;
pub mod rpc;

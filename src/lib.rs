//! wharfnet — one-command localnet for EVM, Solana & Starknet.
//!
//! This crate is both the `wharfnet` CLI and a library. The library exposes a
//! small **test-utils** API ([`testkit`]) so integration tests can connect to a
//! running localnet and read its endpoints, funded accounts, and pre-deployed
//! token addresses from the manifest — instead of hard-coding RPC URLs and
//! addresses.
//!
//! ```no_run
//! use wharfnet::testkit::Localnet;
//!
//! # fn main() -> anyhow::Result<()> {
//! // Reads `.wharfnet/wharfnet.json`, written by `wharfnet up`.
//! let net = Localnet::connect()?;
//! let sol = net.solana();
//! let rpc = sol.rpc_url();          // e.g. "http://127.0.0.1:8899"
//! let usdc = sol.token("USDC");     // mint address + decimals
//! let dev0 = sol.account(0);        // a funded dev signer
//! # let _ = (rpc, usdc, dev0);
//! # Ok(())
//! # }
//! ```

mod evm;
mod faucet;
mod runtime;
mod solana;
mod starknet;
mod utxo;

// Internal docker-backed harness for the crate's own end-to-end tests.
#[cfg(test)]
mod harness;

pub mod abi;
pub mod cli;
pub mod testkit;

// The manifest data model, re-exported for downstream tests that want the raw
// types behind the [`testkit`] handles.
pub use runtime::manifest::{Account, ChainEntry, Contract, Manifest, Token};

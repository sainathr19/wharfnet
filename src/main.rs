//! wharfnet — one-command localnet for EVM, Solana & Starknet.
//!
//! The binary is a thin shim over the library's [`wharfnet::cli`]; all logic
//! lives in the crate so it can also be used as a test-utils dependency (see
//! [`wharfnet::testkit`]).

fn main() {
    wharfnet::cli::main();
}

//! Test utilities for driving a running wharfnet localnet from integration
//! tests.
//!
//! [`Localnet::connect`] reads the endpoints manifest that `wharfnet up` writes
//! to `.wharfnet/wharfnet.json`, giving typed access to each chain's RPC/WS
//! URLs, funded dev accounts (with keys), and pre-deployed token addresses — so
//! tests never hard-code any of them.
//!
//! ```no_run
//! use wharfnet::testkit::Localnet;
//!
//! # fn main() -> anyhow::Result<()> {
//! let net = Localnet::connect()?;
//! let sol = net.solana();
//! let client_url = sol.rpc_url();       // point your Solana client here
//! let usdc = sol.token("USDC");         // { address, decimals, .. }
//! let signer = sol.account(0);          // funded dev account + private key
//! # let _ = (client_url, usdc, signer);
//! # Ok(())
//! # }
//! ```

use std::path::Path;

use anyhow::{Context, Result};

use crate::runtime::manifest::{Account, ChainEntry, Manifest, Token};
use crate::runtime::orchestrator::{DEFAULT_STATE_DIR, manifest_path};

/// A handle to a running localnet, read from the manifest `wharfnet up` writes.
///
/// Construct one with [`Localnet::connect`] (current directory) or
/// [`Localnet::connect_from`] (an explicit `.wharfnet` directory).
#[derive(Debug)]
pub struct Localnet {
    manifest: Manifest,
}

impl Localnet {
    /// Connect to the localnet started by `wharfnet up` in the current working
    /// directory. Returns a clear error if no manifest is found (i.e. nothing
    /// is running).
    pub fn connect() -> Result<Self> {
        Self::connect_from(Path::new(DEFAULT_STATE_DIR))
    }

    /// Connect using a specific `.wharfnet` state directory — useful when the
    /// test process runs from a different working directory than `wharfnet up`.
    pub fn connect_from(state_dir: &Path) -> Result<Self> {
        let path = manifest_path(state_dir);
        let manifest = Manifest::read(&path).with_context(|| {
            format!(
                "no running localnet at {} — start one with `wharfnet up`",
                path.display()
            )
        })?;
        Ok(Self { manifest })
    }

    /// Every chain in the manifest.
    pub fn chains(&self) -> impl Iterator<Item = Chain<'_>> {
        self.manifest.chains.iter().map(Chain::new)
    }

    /// A chain by exact name (e.g. `"anvil-1"`, `"solana-1"`).
    pub fn chain(&self, name: &str) -> Result<Chain<'_>> {
        self.manifest
            .chains
            .iter()
            .find(|c| c.name == name)
            .map(Chain::new)
            .with_context(|| format!("no chain named '{name}' in the running localnet"))
    }

    /// The first chain of a kind (`"evm"`, `"solana"`, `"starknet"`), or an
    /// error if none is running.
    pub fn of_kind(&self, kind: &str) -> Result<Chain<'_>> {
        self.manifest
            .chains
            .iter()
            .find(|c| c.kind == kind)
            .map(Chain::new)
            .with_context(|| format!("no {kind} chain in the running localnet"))
    }

    /// The first EVM chain. Panics if none is running — convenient in tests
    /// where a missing chain is a setup error, not a case to handle.
    pub fn evm(&self) -> Chain<'_> {
        self.expect_kind("evm")
    }

    /// The first Solana chain. Panics if none is running.
    pub fn solana(&self) -> Chain<'_> {
        self.expect_kind("solana")
    }

    /// The first Starknet chain. Panics if none is running.
    pub fn starknet(&self) -> Chain<'_> {
        self.expect_kind("starknet")
    }

    fn expect_kind(&self, kind: &str) -> Chain<'_> {
        self.of_kind(kind).unwrap_or_else(|e| panic!("{e}"))
    }
}

/// A single chain's endpoints, funded accounts, and pre-deployed tokens.
///
/// A lightweight borrowed view over one manifest entry — cheap to copy.
#[derive(Clone, Copy)]
pub struct Chain<'a> {
    entry: &'a ChainEntry,
}

impl<'a> Chain<'a> {
    fn new(entry: &'a ChainEntry) -> Self {
        Self { entry }
    }

    /// The chain's name (e.g. `"anvil-1"`).
    pub fn name(&self) -> &str {
        &self.entry.name
    }

    /// The chain kind (`"evm"`, `"solana"`, `"starknet"`).
    pub fn kind(&self) -> &str {
        &self.entry.kind
    }

    /// The HTTP JSON-RPC URL — point your client (viem, solana-client,
    /// starknet.js, …) here.
    pub fn rpc_url(&self) -> &str {
        &self.entry.rpc
    }

    /// The WebSocket RPC URL, when the chain serves one on a port distinct from
    /// its HTTP RPC (Solana). EVM/Starknet share the HTTP port, so this is
    /// `None`.
    pub fn ws_url(&self) -> Option<&str> {
        self.entry.ws.as_deref()
    }

    /// The chain id as a string — decimal for EVM (`"31337"`), a felt for
    /// Starknet, or `"localnet"` for Solana.
    pub fn chain_id(&self) -> &str {
        &self.entry.chain_id
    }

    /// The bundled block explorer URL, when one was booted (skipped by
    /// `wharfnet up --bare`).
    pub fn explorer(&self) -> Option<&str> {
        self.entry.explorer.as_deref()
    }

    /// The funded dev accounts (address, private key, and starting balance).
    pub fn accounts(&self) -> &[Account] {
        &self.entry.accounts
    }

    /// The i-th funded dev account. Panics if out of range.
    pub fn account(&self, i: usize) -> &Account {
        self.entry.accounts.get(i).unwrap_or_else(|| {
            panic!(
                "chain '{}' has {} dev accounts; no account #{i}",
                self.entry.name,
                self.entry.accounts.len()
            )
        })
    }

    /// The pre-deployed test tokens at their fixed addresses.
    pub fn tokens(&self) -> &[Token] {
        &self.entry.tokens
    }

    /// A token by symbol (e.g. `"USDC"`). Panics if this chain doesn't have it.
    pub fn token(&self, symbol: &str) -> &Token {
        self.try_token(symbol)
            .unwrap_or_else(|| panic!("chain '{}' has no token '{symbol}'", self.entry.name))
    }

    /// A token by symbol, or `None` if this chain doesn't have it.
    pub fn try_token(&self, symbol: &str) -> Option<&Token> {
        self.entry.tokens.iter().find(|t| t.symbol == symbol)
    }

    /// The underlying manifest entry, for fields not surfaced by the helpers
    /// above (e.g. canonical `contracts` or a redacted `fork` source).
    pub fn entry(&self) -> &ChainEntry {
        self.entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_sample(dir: &Path) {
        let m = Manifest::new(vec![ChainEntry {
            name: "solana-1".into(),
            kind: "solana".into(),
            rpc: "http://127.0.0.1:8899".into(),
            ws: Some("ws://127.0.0.1:8900".into()),
            chain_id: "localnet".into(),
            accounts: vec![Account {
                address: "Dev0".into(),
                private_key: "sk0".into(),
                balance: "10000 SOL".into(),
            }],
            tokens: vec![Token {
                symbol: "USDC".into(),
                name: "USD Coin".into(),
                address: "Mint1111".into(),
                decimals: 6,
            }],
            contracts: vec![],
            fork: None,
            explorer: Some("http://127.0.0.1:18899".into()),
        }]);
        m.write(&manifest_path(dir)).unwrap();
    }

    #[test]
    fn connect_reads_endpoints_accounts_and_tokens() {
        let dir = tempdir().unwrap();
        write_sample(dir.path());

        let net = Localnet::connect_from(dir.path()).unwrap();
        let sol = net.solana();
        assert_eq!(sol.name(), "solana-1");
        assert_eq!(sol.rpc_url(), "http://127.0.0.1:8899");
        assert_eq!(sol.ws_url(), Some("ws://127.0.0.1:8900"));
        assert_eq!(sol.chain_id(), "localnet");
        assert_eq!(sol.explorer(), Some("http://127.0.0.1:18899"));
        assert_eq!(sol.account(0).address, "Dev0");
        assert_eq!(sol.account(0).private_key, "sk0");
        assert_eq!(sol.token("USDC").decimals, 6);
        assert!(sol.try_token("WBTC").is_none());
    }

    #[test]
    fn connect_without_a_manifest_errors_with_a_hint() {
        let dir = tempdir().unwrap();
        let err = Localnet::connect_from(dir.path()).unwrap_err();
        assert!(err.to_string().contains("wharfnet up"), "{err}");
    }

    #[test]
    fn selecting_a_missing_chain_or_kind_reports_clearly() {
        let dir = tempdir().unwrap();
        write_sample(dir.path());
        let net = Localnet::connect_from(dir.path()).unwrap();

        assert!(net.of_kind("evm").is_err());
        assert!(net.chain("anvil-1").is_err());
        assert_eq!(net.chain("solana-1").unwrap().kind(), "solana");
        assert_eq!(net.chains().count(), 1);
    }
}

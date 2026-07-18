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
///
/// Accessors return references tied to the manifest (`'a`), not to the handle,
/// so the values they hand back outlive the temporary `Chain`.
#[derive(Debug, Clone, Copy)]
pub struct Chain<'a> {
    entry: &'a ChainEntry,
}

impl<'a> Chain<'a> {
    fn new(entry: &'a ChainEntry) -> Self {
        Self { entry }
    }

    /// The chain's name (e.g. `"anvil-1"`).
    pub fn name(&self) -> &'a str {
        &self.entry.name
    }

    /// The chain kind (`"evm"`, `"solana"`, `"starknet"`).
    pub fn kind(&self) -> &'a str {
        &self.entry.kind
    }

    /// The HTTP JSON-RPC URL — point your client (viem, solana-client,
    /// starknet.js, …) here.
    pub fn rpc_url(&self) -> &'a str {
        &self.entry.rpc
    }

    /// The WebSocket RPC URL, when the chain serves one on a port distinct from
    /// its HTTP RPC (Solana). EVM/Starknet share the HTTP port, so this is
    /// `None`.
    pub fn ws_url(&self) -> Option<&'a str> {
        self.entry.ws.as_deref()
    }

    /// The chain id as a string — decimal for EVM (`"31337"`), a felt for
    /// Starknet, or `"localnet"` for Solana.
    pub fn chain_id(&self) -> &'a str {
        &self.entry.chain_id
    }

    /// The bundled block explorer URL, when one was booted (skipped by
    /// `wharfnet up --bare`).
    pub fn explorer(&self) -> Option<&'a str> {
        self.entry.explorer.as_deref()
    }

    /// The funded dev accounts (address, private key, and starting balance).
    pub fn accounts(&self) -> &'a [Account] {
        &self.entry.accounts
    }

    /// The i-th funded dev account. Panics if out of range.
    pub fn account(&self, i: usize) -> &'a Account {
        self.entry.accounts.get(i).unwrap_or_else(|| {
            panic!(
                "chain '{}' has {} dev accounts; no account #{i}",
                self.entry.name,
                self.entry.accounts.len()
            )
        })
    }

    /// The pre-deployed test tokens at their fixed addresses.
    pub fn tokens(&self) -> &'a [Token] {
        &self.entry.tokens
    }

    /// A token by symbol (e.g. `"USDC"`). Panics if this chain doesn't have it.
    pub fn token(&self, symbol: &str) -> &'a Token {
        self.try_token(symbol)
            .unwrap_or_else(|| panic!("chain '{}' has no token '{symbol}'", self.entry.name))
    }

    /// A token by symbol, or `None` if this chain doesn't have it.
    pub fn try_token(&self, symbol: &str) -> Option<&'a Token> {
        self.entry.tokens.iter().find(|t| t.symbol == symbol)
    }

    /// The contract ABI (as JSON) for a bundled test token — feed it straight to
    /// viem/ethers/alloy (EVM) or starknet.js/starknet-rust (Starknet).
    ///
    /// `None` when the interface isn't shipped: Solana SPL tokens (use the
    /// standard SPL Token program) and the Starknet `ETH`/`STRK` fee tokens
    /// (provided by devnet). See [`crate::abi`] for the raw constants.
    pub fn token_abi(&self, symbol: &str) -> Option<&'static str> {
        crate::abi::token_abi(self.kind(), symbol)
    }

    /// The underlying manifest entry, for fields not surfaced by the helpers
    /// above (e.g. canonical `contracts` or a redacted `fork` source).
    pub fn entry(&self) -> &'a ChainEntry {
        self.entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::Contract;
    use tempfile::tempdir;

    fn acct(addr: &str) -> Account {
        Account {
            address: addr.into(),
            private_key: format!("sk-{addr}"),
            balance: "1000".into(),
        }
    }

    fn token(symbol: &str, decimals: u8) -> Token {
        Token {
            symbol: symbol.into(),
            name: format!("{symbol} token"),
            address: format!("addr-{symbol}"),
            decimals,
        }
    }

    /// A single-chain Solana manifest (with a distinct WS + explorer).
    fn write_sample(dir: &Path) {
        Manifest::new(vec![ChainEntry {
            name: "solana-1".into(),
            kind: "solana".into(),
            rpc: "http://127.0.0.1:8899".into(),
            ws: Some("ws://127.0.0.1:8900".into()),
            chain_id: "localnet".into(),
            accounts: vec![acct("Dev0"), acct("Dev1")],
            tokens: vec![token("USDC", 6)],
            contracts: vec![],
            fork: None,
            explorer: Some("http://127.0.0.1:18899".into()),
        }])
        .write(&manifest_path(dir))
        .unwrap();
    }

    /// A three-chain manifest exercising every kind, ws-present/absent, and
    /// explorer-present/absent.
    fn write_multichain(dir: &Path) {
        Manifest::new(vec![
            ChainEntry {
                name: "anvil-1".into(),
                kind: "evm".into(),
                rpc: "http://127.0.0.1:8545".into(),
                ws: None,
                chain_id: "31337".into(),
                accounts: vec![acct("0xf39F"), acct("0x7099")],
                tokens: vec![token("USDC", 6), token("WBTC", 8), token("NRT", 6)],
                contracts: vec![Contract {
                    name: "Multicall3".into(),
                    address: "0xcA11".into(),
                }],
                fork: None,
                explorer: Some("http://127.0.0.1:5100".into()),
            },
            ChainEntry {
                name: "solana-1".into(),
                kind: "solana".into(),
                rpc: "http://127.0.0.1:8899".into(),
                ws: Some("ws://127.0.0.1:8900".into()),
                chain_id: "localnet".into(),
                accounts: vec![acct("Dev0")],
                tokens: vec![token("USDC", 6), token("WBTC", 8)],
                contracts: vec![],
                fork: None,
                explorer: None,
            },
            ChainEntry {
                name: "starknet-1".into(),
                kind: "starknet".into(),
                rpc: "http://127.0.0.1:5050/rpc".into(),
                ws: None,
                chain_id: "0x534e5f5345504f4c4941".into(),
                accounts: vec![acct("0x064b")],
                tokens: vec![token("REB", 18), token("ETH", 18)],
                contracts: vec![],
                fork: Some("https://sepolia".into()),
                explorer: Some("http://127.0.0.1:5050/ui".into()),
            },
        ])
        .write(&manifest_path(dir))
        .unwrap();
    }

    #[test]
    fn connect_reads_endpoints_accounts_and_tokens() {
        let dir = tempdir().unwrap();
        write_sample(dir.path());

        let net = Localnet::connect_from(dir.path()).unwrap();
        let sol = net.solana();
        assert_eq!(sol.name(), "solana-1");
        assert_eq!(sol.kind(), "solana");
        assert_eq!(sol.rpc_url(), "http://127.0.0.1:8899");
        assert_eq!(sol.ws_url(), Some("ws://127.0.0.1:8900"));
        assert_eq!(sol.chain_id(), "localnet");
        assert_eq!(sol.explorer(), Some("http://127.0.0.1:18899"));
        assert_eq!(sol.account(0).address, "Dev0");
        assert_eq!(sol.account(0).private_key, "sk-Dev0");
        assert_eq!(sol.accounts().len(), 2);
        assert_eq!(sol.tokens().len(), 1);
        assert_eq!(sol.token("USDC").decimals, 6);
        assert!(sol.try_token("WBTC").is_none());
        assert_eq!(sol.entry().name, "solana-1");
    }

    #[test]
    fn connect_uses_the_default_state_dir() {
        // Exercises the cwd-based entry point. The repo may or may not have a
        // live `.wharfnet`, so only assert it returns a `Result` without
        // panicking — not its contents.
        let _ = Localnet::connect();
    }

    #[test]
    fn connect_without_a_manifest_errors_with_a_hint() {
        let dir = tempdir().unwrap();
        let err = Localnet::connect_from(dir.path()).unwrap_err();
        assert!(err.to_string().contains("wharfnet up"), "{err}");
    }

    #[test]
    fn per_kind_accessors_pick_the_right_chain() {
        let dir = tempdir().unwrap();
        write_multichain(dir.path());
        let net = Localnet::connect_from(dir.path()).unwrap();

        assert_eq!(net.evm().name(), "anvil-1");
        assert_eq!(net.solana().name(), "solana-1");
        assert_eq!(net.starknet().name(), "starknet-1");

        // ws is present only for Solana; explorer only for the two that booted one.
        assert_eq!(net.evm().ws_url(), None);
        assert_eq!(net.solana().ws_url(), Some("ws://127.0.0.1:8900"));
        assert_eq!(net.solana().explorer(), None);
        assert_eq!(net.starknet().explorer(), Some("http://127.0.0.1:5050/ui"));

        // entry() exposes fields not surfaced directly.
        assert_eq!(net.evm().entry().contracts[0].name, "Multicall3");
        assert_eq!(
            net.starknet().entry().fork.as_deref(),
            Some("https://sepolia")
        );
    }

    #[test]
    fn chains_iterates_all_and_lookups_resolve_by_name_and_kind() {
        let dir = tempdir().unwrap();
        write_multichain(dir.path());
        let net = Localnet::connect_from(dir.path()).unwrap();

        let names: Vec<&str> = net.chains().map(|c| c.name()).collect();
        assert_eq!(names, vec!["anvil-1", "solana-1", "starknet-1"]);

        assert_eq!(net.chain("anvil-1").unwrap().kind(), "evm");
        assert_eq!(net.of_kind("starknet").unwrap().name(), "starknet-1");
    }

    #[test]
    fn selecting_a_missing_chain_or_kind_reports_clearly() {
        let dir = tempdir().unwrap();
        write_sample(dir.path());
        let net = Localnet::connect_from(dir.path()).unwrap();

        let by_name = net.chain("anvil-1").unwrap_err();
        assert!(by_name.to_string().contains("anvil-1"), "{by_name}");
        let by_kind = net.of_kind("evm").unwrap_err();
        assert!(by_kind.to_string().contains("evm"), "{by_kind}");
    }

    #[test]
    fn token_abi_is_wired_to_the_embedded_abis() {
        let dir = tempdir().unwrap();
        write_multichain(dir.path());
        let net = Localnet::connect_from(dir.path()).unwrap();

        // EVM + Starknet ship ABIs; Solana SPL does not.
        assert!(net.evm().token_abi("USDC").is_some());
        assert!(net.evm().token_abi("NRT").is_some());
        assert!(net.starknet().token_abi("REB").is_some());
        assert!(net.starknet().token_abi("ETH").is_none());
        assert!(net.solana().token_abi("USDC").is_none());
    }

    #[test]
    #[should_panic(expected = "no account #5")]
    fn account_out_of_range_panics() {
        let dir = tempdir().unwrap();
        write_sample(dir.path());
        let net = Localnet::connect_from(dir.path()).unwrap();
        net.solana().account(5);
    }

    #[test]
    #[should_panic(expected = "no token 'DOGE'")]
    fn token_missing_panics() {
        let dir = tempdir().unwrap();
        write_sample(dir.path());
        let net = Localnet::connect_from(dir.path()).unwrap();
        net.solana().token("DOGE");
    }

    #[test]
    #[should_panic(expected = "no evm chain")]
    fn per_kind_accessor_panics_when_kind_absent() {
        let dir = tempdir().unwrap();
        write_sample(dir.path()); // solana only
        let net = Localnet::connect_from(dir.path()).unwrap();
        net.evm();
    }
}

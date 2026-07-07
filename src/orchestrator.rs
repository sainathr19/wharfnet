//! The orchestrator: turn the configured engines into a running localnet,
//! check health, write the manifest, and tear everything down.
//!
//! The public `up`/`down`/`status` use the default state dir and project name;
//! the `_in` variants take them as parameters so they can be driven against an
//! isolated temp dir in tests.

use anyhow::{Result, bail};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::docker;
use crate::engine::{Engine, EvmEngine};
use crate::manifest::Manifest;
use crate::ui;

const DEFAULT_PROJECT: &str = "wharfnet";
const DEFAULT_STATE_DIR: &str = ".wharfnet";
const READY_TIMEOUT: Duration = Duration::from_secs(90);

/// Header prepended to every generated compose file (embedded at compile time).
const COMPOSE_HEADER: &str = include_str!("resources/docker/compose.header.yml");

fn compose_path(base: &Path) -> PathBuf {
    base.join("docker-compose.yml")
}

fn manifest_path(base: &Path) -> PathBuf {
    base.join("wharfnet.json")
}

/// The chains wharfnet boots. Two Anvil-backed EVM chains today; Solana and
/// Starknet engines are appended here as they land.
fn engines() -> Vec<Box<dyn Engine>> {
    vec![
        Box::new(EvmEngine::anvil("anvil-1", 8545, 31337)),
        Box::new(EvmEngine::anvil("anvil-2", 8546, 31338)),
    ]
}

fn render_compose(engines: &[Box<dyn Engine>]) -> String {
    let mut out = String::from(COMPOSE_HEADER);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    for engine in engines {
        out.push_str(&engine.compose_service());
    }
    out
}

/// Write each engine's staged files (chain snapshots, etc.) under the state
/// dir before boot. Engines that share a snapshot write identical bytes to the
/// same path, so this is idempotent.
fn stage_files(base: &Path, engines: &[Box<dyn Engine>]) -> Result<()> {
    for engine in engines {
        for file in engine.staged_files() {
            let dest = base.join(file.rel_path);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, file.contents)?;
        }
    }
    Ok(())
}

/// Print the generated compose file to stdout without booting anything.
/// Useful for inspecting or debugging what wharfnet will run.
pub fn print_compose() -> Result<()> {
    print!("{}", render_compose(&engines()));
    Ok(())
}

pub fn up() -> Result<()> {
    up_in(Path::new(DEFAULT_STATE_DIR), DEFAULT_PROJECT)
}

pub fn down() -> Result<()> {
    down_in(Path::new(DEFAULT_STATE_DIR), DEFAULT_PROJECT)
}

pub fn status() -> Result<()> {
    status_in(Path::new(DEFAULT_STATE_DIR), DEFAULT_PROJECT)
}

fn up_in(base: &Path, project: &str) -> Result<()> {
    docker::ensure_available()?;
    let engines = engines();

    fs::create_dir_all(base)?;
    fs::write(compose_path(base), render_compose(&engines))?;
    stage_files(base, &engines)?;

    println!("⚓ wharfnet: booting {} chain(s)...", engines.len());

    let pb = ui::spinner("pulling images & starting containers…");
    if let Err(e) = docker::compose_up(&compose_path(base), project) {
        ui::finish_err(&pb, "failed to start containers");
        return Err(e);
    }
    ui::finish_ok(&pb, "containers started");

    for engine in &engines {
        let pb = ui::spinner(format!(
            "waiting for {} (:{})…",
            engine.name(),
            engine.host_port()
        ));
        if wait_for_rpc(engine.host_port(), READY_TIMEOUT) {
            ui::finish_ok(&pb, format!("{} ready", engine.name()));
        } else {
            ui::finish_err(&pb, format!("{} timed out", engine.name()));
            bail!(
                "{} did not become ready within {}s",
                engine.name(),
                READY_TIMEOUT.as_secs()
            );
        }
    }

    let manifest = Manifest::new(engines.iter().map(|e| e.manifest_entry()).collect());
    manifest.write(&manifest_path(base))?;

    println!(
        "\n✅ localnet up. Endpoints written to {}\n",
        manifest_path(base).display()
    );
    for chain in &manifest.chains {
        println!(
            "   {} [{}]  {}  (chainId {})",
            chain.name, chain.kind, chain.rpc, chain.chain_id
        );
        for token in &chain.tokens {
            println!("      {:<5} {}", token.symbol, token.address);
        }
    }
    println!("\nTear down with: wharfnet down");
    Ok(())
}

fn down_in(base: &Path, project: &str) -> Result<()> {
    let compose = compose_path(base);
    if !compose.exists() {
        println!(
            "wharfnet: nothing to tear down (no {} found).",
            compose.display()
        );
        return Ok(());
    }

    docker::ensure_available()?;

    let pb = ui::spinner("tearing down localnet…");
    if let Err(e) = docker::compose_down(&compose, project) {
        ui::finish_err(&pb, "teardown failed");
        return Err(e);
    }
    ui::finish_ok(&pb, "localnet down");
    let _ = fs::remove_file(manifest_path(base));
    Ok(())
}

fn status_in(base: &Path, project: &str) -> Result<()> {
    let manifest_file = manifest_path(base);
    if !manifest_file.exists() {
        println!("wharfnet: localnet is not running. Start it with `wharfnet up`.");
        return Ok(());
    }

    let manifest = Manifest::read(&manifest_file)?;
    println!("wharfnet localnet — {} chain(s):\n", manifest.chains.len());
    for chain in &manifest.chains {
        println!("  {} [{}]", chain.name, chain.kind);
        println!("     rpc      {}", chain.rpc);
        println!("     chainId  {}", chain.chain_id);
        if let Some(account) = chain.accounts.first() {
            println!("     account  {}", account.address);
        }
        for token in &chain.tokens {
            println!(
                "     token    {:<5} {} ({} dec)",
                token.symbol, token.address, token.decimals
            );
        }
        println!();
    }

    let compose = compose_path(base);
    if compose.exists() {
        println!("containers:");
        let _ = docker::compose_ps(&compose, project);
    }
    Ok(())
}

fn wait_for_rpc(port: u16, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if rpc_ready(port) {
            return true;
        }
        sleep(Duration::from_secs(1));
    }
    false
}

/// Minimal, dependency-free JSON-RPC health check: send `eth_chainId` and look
/// for a `result` in the response.
fn rpc_ready(port: u16) -> bool {
    let addr = format!("127.0.0.1:{port}");
    let Ok(mut stream) = TcpStream::connect(&addr) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));

    let body = r#"{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}"#;
    let request = format!(
        "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response.contains("\"result\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Account, ChainEntry, Token};
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use tempfile::tempdir;

    #[test]
    fn render_compose_has_header_and_all_services() {
        let out = render_compose(&engines());
        assert!(out.starts_with("# Generated by wharfnet"));
        assert!(out.contains("services:"));
        assert!(out.contains("anvil-1:"));
        assert!(out.contains("anvil-2:"));
    }

    #[test]
    fn print_compose_is_ok() {
        assert!(print_compose().is_ok());
    }

    #[test]
    fn engines_returns_default_set() {
        let engines = engines();
        assert_eq!(engines.len(), 2);
        assert_eq!(engines[0].name(), "anvil-1");
        assert_eq!(engines[1].name(), "anvil-2");
    }

    #[test]
    fn engines_use_distinct_ports_and_chain_ids() {
        let engines = engines();
        let ports: Vec<u16> = engines.iter().map(|e| e.host_port()).collect();
        let chain_ids: Vec<u64> = engines.iter().map(|e| e.manifest_entry().chain_id).collect();
        assert_eq!(ports, vec![8545, 8546]);
        assert_eq!(chain_ids, vec![31337, 31338]);
        // No two chains may share a host port or the compose file won't bind.
        assert_ne!(ports[0], ports[1]);
    }

    #[test]
    fn stage_files_writes_the_token_snapshot() {
        let dir = tempdir().unwrap();
        stage_files(dir.path(), &engines()).unwrap();
        let snapshot = dir.path().join("state/anvil-tokens.json");
        assert!(snapshot.exists(), "snapshot should be staged under state/");
        let data = fs::read_to_string(&snapshot).unwrap();
        // Sanity: the snapshot really contains our deployed USDC address.
        assert!(data.contains("5fbdb2315678afecb367f032d93f642f64180aa3"));
    }

    #[test]
    fn paths_are_joined_under_base() {
        let base = Path::new("/tmp/whatever");
        assert_eq!(
            compose_path(base),
            Path::new("/tmp/whatever/docker-compose.yml")
        );
        assert_eq!(manifest_path(base), Path::new("/tmp/whatever/wharfnet.json"));
    }

    #[test]
    fn status_in_reports_not_running_on_empty_dir() {
        let dir = tempdir().unwrap();
        assert!(status_in(dir.path(), "wharfnet-test").is_ok());
    }

    #[test]
    fn status_in_prints_manifest_when_present() {
        let dir = tempdir().unwrap();
        let manifest = Manifest::new(vec![ChainEntry {
            name: "anvil-1".into(),
            kind: "evm".into(),
            rpc: "http://127.0.0.1:8545".into(),
            chain_id: 31337,
            accounts: vec![Account {
                address: "0xabc".into(),
                private_key: "0xdef".into(),
                balance: "10000 ETH".into(),
            }],
            tokens: vec![Token {
                symbol: "USDC".into(),
                name: "USD Coin".into(),
                address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".into(),
                decimals: 6,
            }],
        }]);
        manifest.write(&manifest_path(dir.path())).unwrap();
        // No docker-compose.yml in the dir → the compose_ps branch is skipped,
        // so this needs no Docker.
        assert!(status_in(dir.path(), "wharfnet-test").is_ok());
    }

    #[test]
    fn down_in_is_noop_when_nothing_to_tear_down() {
        let dir = tempdir().unwrap();
        assert!(down_in(dir.path(), "wharfnet-test").is_ok());
    }

    // ---- health check, exercised against a one-shot mock TCP server ----

    fn spawn_mock_rpc(response_body: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        port
    }

    #[test]
    fn rpc_ready_true_when_result_present() {
        let port = spawn_mock_rpc(r#"{"jsonrpc":"2.0","id":1,"result":"0x7a69"}"#);
        std::thread::sleep(Duration::from_millis(50));
        assert!(rpc_ready(port));
    }

    #[test]
    fn rpc_ready_false_without_result() {
        let port = spawn_mock_rpc(r#"{"jsonrpc":"2.0","id":1,"error":"boom"}"#);
        std::thread::sleep(Duration::from_millis(50));
        assert!(!rpc_ready(port));
    }

    #[test]
    fn rpc_ready_false_when_nothing_listening() {
        assert!(!rpc_ready(1));
    }

    #[test]
    fn wait_for_rpc_succeeds_when_server_is_up() {
        let port = spawn_mock_rpc(r#"{"result":"ok"}"#);
        std::thread::sleep(Duration::from_millis(50));
        assert!(wait_for_rpc(port, Duration::from_secs(3)));
    }

    #[test]
    fn wait_for_rpc_times_out_when_nothing_listening() {
        assert!(!wait_for_rpc(2, Duration::from_millis(10)));
    }
}

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

use super::config::{self, Config};
use super::docker;
use super::engine::{Engine, HealthProbe, StateMode};
use super::manifest::Manifest;
use super::ui;
use crate::evm::engine::EvmEngine;
use crate::starknet::engine::StarknetEngine;

pub(crate) const DEFAULT_PROJECT: &str = "wharfnet";
pub(crate) const DEFAULT_STATE_DIR: &str = ".wharfnet";
const READY_TIMEOUT: Duration = Duration::from_secs(90);

/// Pinned Otterscan image. Otterscan is a static frontend that speaks straight
/// to the chain's RPC via Anvil's `ots_*` API — no indexer or database.
const OTTERSCAN_IMAGE: &str = "otterscan/otterscan:v2.11.0";
/// Compose service template for an Otterscan explorer.
const OTTERSCAN_SERVICE_TEMPLATE: &str = include_str!("../resources/docker/services/otterscan.yml");
/// First host port for explorers; each subsequent chain's explorer takes the next.
const EXPLORER_BASE_PORT: u16 = 5100;

/// How `up` should treat any previously saved session state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpMode {
    /// Boot fresh from the baked snapshot; leave any saved session untouched.
    Fresh,
    /// Restore the saved session if present (else boot fresh) and keep saving.
    Resume,
    /// Discard any saved session, then boot fresh.
    Reset,
}

impl UpMode {
    fn state_mode(self) -> StateMode {
        match self {
            UpMode::Resume => StateMode::Persistent,
            UpMode::Fresh | UpMode::Reset => StateMode::Ephemeral,
        }
    }
}

/// Header prepended to every generated compose file (embedded at compile time).
const COMPOSE_HEADER: &str = include_str!("../resources/docker/compose.header.yml");

pub(crate) fn compose_path(base: &Path) -> PathBuf {
    base.join("docker-compose.yml")
}

pub(crate) fn manifest_path(base: &Path) -> PathBuf {
    base.join("wharfnet.json")
}

/// Build the engines described by `config`, dispatching on each chain's `kind`.
/// `config::validate` has already guaranteed the kind is supported and that an
/// EVM chain carries a numeric `chain_id`, so the parsing here can't realistically
/// fail; it's an internal invariant, not user-facing validation.
///
/// `explorer` mirrors the caller's explorer preference (`up` on, `up --bare`
/// off). EVM chains pair with a separate Otterscan container built later, but a
/// Starknet chain serves its explorer in-process, so the flag is baked into the
/// engine here to reach its `--ui` compose arg.
fn engines_for(config: &Config, explorer: bool) -> Vec<Box<dyn Engine>> {
    config
        .chains
        .iter()
        .map(|c| engine_for(c, explorer))
        .collect()
}

fn engine_for(c: &config::ChainConfig, explorer: bool) -> Box<dyn Engine> {
    match c.kind.as_str() {
        "evm" => {
            let chain_id = c
                .chain_id
                .as_deref()
                .expect("validate() guarantees an evm chain_id")
                .parse::<u64>()
                .expect("validate() guarantees the evm chain_id is numeric");
            let mut engine = EvmEngine::anvil(&c.name, c.port, chain_id).block_time(c.block_time);
            if let Some(url) = &c.fork_url {
                engine = engine.fork(url.clone(), c.fork_block);
            }
            Box::new(engine)
        }
        "starknet" => Box::new(StarknetEngine::devnet(&c.name, c.port).ui(explorer)),
        other => unreachable!("validate() rejects unsupported kind '{other}'"),
    }
}

/// A resolved Otterscan explorer for one chain: its service name, the host port
/// it's published on, the generated frontend config, and the URL to open.
struct ExplorerService {
    service_name: String,
    chain_name: String,
    host_port: u16,
    config_rel_path: String,
    config_file: String,
    config_json: String,
    url: String,
}

/// Build one explorer per explorer-capable chain, assigning host ports from
/// `EXPLORER_BASE_PORT` upward in engine order. Empty when none support one.
fn explorer_services(engines: &[Box<dyn Engine>]) -> Vec<ExplorerService> {
    let mut services = Vec::new();
    let mut port = EXPLORER_BASE_PORT;
    for engine in engines {
        let Some(target) = engine.explorer_target() else {
            continue;
        };
        let host_port = port;
        port += 1;
        let config_file = format!("otterscan-{}.json", target.chain_name);
        // The browser (on the host) hits both the chain RPC and the explorer's
        // own bundled assets, so both URLs use published host ports.
        let config_json = format!(
            "{{\n  \"erigonURL\": \"http://127.0.0.1:{rpc}\",\n  \"assetsURLPrefix\": \"http://127.0.0.1:{exp}\"\n}}\n",
            rpc = target.rpc_host_port,
            exp = host_port,
        );
        services.push(ExplorerService {
            service_name: format!("explorer-{}", target.chain_name),
            chain_name: target.chain_name,
            host_port,
            config_rel_path: format!("state/{config_file}"),
            config_file,
            config_json,
            url: format!("http://127.0.0.1:{host_port}"),
        });
    }
    services
}

fn render_explorer_service(svc: &ExplorerService) -> String {
    OTTERSCAN_SERVICE_TEMPLATE
        .replace("{{NAME}}", &svc.service_name)
        .replace("{{IMAGE}}", OTTERSCAN_IMAGE)
        .replace("{{HOST_PORT}}", &svc.host_port.to_string())
        .replace("{{CONFIG_FILE}}", &svc.config_file)
}

fn render_compose(
    engines: &[Box<dyn Engine>],
    mode: StateMode,
    explorers: &[ExplorerService],
) -> String {
    let mut out = String::from(COMPOSE_HEADER);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    for engine in engines {
        out.push_str(&engine.compose_service(mode));
    }
    for svc in explorers {
        out.push_str(&render_explorer_service(svc));
    }
    out
}

/// Write each explorer's generated `config.json` under the state dir so the
/// compose file can mount it into the Otterscan container.
fn stage_explorer_configs(base: &Path, explorers: &[ExplorerService]) -> Result<()> {
    for svc in explorers {
        let dest = base.join(&svc.config_rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, &svc.config_json)?;
    }
    Ok(())
}

/// Write each engine's staged files (chain snapshots, session seeds) under the
/// state dir before boot. A file marked `if_absent` is only written when it
/// doesn't already exist, so a saved session is never overwritten. Engines that
/// share a snapshot write identical bytes to the same path, so this is
/// idempotent.
fn stage_files(base: &Path, engines: &[Box<dyn Engine>], mode: StateMode) -> Result<()> {
    for engine in engines {
        for file in engine.staged_files(mode) {
            let dest = base.join(&file.rel_path);
            if file.if_absent && dest.exists() {
                continue;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, file.contents)?;
        }
    }
    Ok(())
}

/// Delete every saved per-chain session snapshot under the state dir. Used by
/// `up --reset` to guarantee a clean slate. Missing dir / files are fine.
fn clear_sessions(base: &Path) -> Result<()> {
    let state = base.join("state");
    let Ok(entries) = fs::read_dir(&state) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        if is_session_file(&entry.file_name().to_string_lossy()) {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

/// Whether any saved per-chain session snapshot exists under the state dir.
fn has_saved_session(base: &Path) -> bool {
    let Ok(entries) = fs::read_dir(base.join("state")) else {
        return false;
    };
    entries
        .flatten()
        .any(|e| is_session_file(&e.file_name().to_string_lossy()))
}

fn is_session_file(name: &str) -> bool {
    name.starts_with("session-") && name.ends_with(".json")
}

/// Print the generated compose file to stdout without booting anything.
/// Useful for inspecting or debugging what wharfnet will run.
pub fn print_compose(explorer: bool, config_path: Option<&Path>) -> Result<()> {
    let engines = engines_for(&config::load(config_path)?, explorer);
    let explorers = if explorer {
        explorer_services(&engines)
    } else {
        Vec::new()
    };
    print!(
        "{}",
        render_compose(&engines, StateMode::Ephemeral, &explorers)
    );
    Ok(())
}

pub fn up(mode: UpMode, explorer: bool, config_path: Option<&Path>) -> Result<()> {
    up_in(
        Path::new(DEFAULT_STATE_DIR),
        DEFAULT_PROJECT,
        mode,
        explorer,
        config_path,
    )
}

pub fn down() -> Result<()> {
    down_in(Path::new(DEFAULT_STATE_DIR), DEFAULT_PROJECT)
}

pub fn status() -> Result<()> {
    status_in(Path::new(DEFAULT_STATE_DIR), DEFAULT_PROJECT)
}

pub(crate) fn up_in(
    base: &Path,
    project: &str,
    mode: UpMode,
    explorer: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    docker::ensure_available()?;
    let engines = engines_for(&config::load(config_path)?, explorer);
    let state_mode = mode.state_mode();
    let explorers = if explorer {
        explorer_services(&engines)
    } else {
        Vec::new()
    };

    if mode == UpMode::Reset {
        clear_sessions(base)?;
    }
    // Note whether we're resuming before staging seeds any missing sessions.
    let resuming = mode == UpMode::Resume && has_saved_session(base);

    fs::create_dir_all(base)?;
    fs::write(
        compose_path(base),
        render_compose(&engines, state_mode, &explorers),
    )?;
    stage_files(base, &engines, state_mode)?;
    stage_explorer_configs(base, &explorers)?;

    match mode {
        UpMode::Resume if resuming => {
            println!(
                "⚓ wharfnet: resuming saved session on {} chain(s)...",
                engines.len()
            )
        }
        UpMode::Resume => {
            println!(
                "⚓ wharfnet: starting a new persistent session on {} chain(s)...",
                engines.len()
            )
        }
        UpMode::Fresh | UpMode::Reset => {
            println!("⚓ wharfnet: booting {} chain(s)...", engines.len())
        }
    }

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
        if wait_ready(engine.host_port(), &engine.health_probe(), READY_TIMEOUT) {
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

    let mut entries: Vec<_> = engines.iter().map(|e| e.manifest_entry()).collect();
    for svc in &explorers {
        if let Some(entry) = entries.iter_mut().find(|c| c.name == svc.chain_name) {
            entry.explorer = Some(svc.url.clone());
        }
    }
    let manifest = Manifest::new(entries);
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
        if let Some(fork) = &chain.fork {
            println!("      fork      {fork}");
        }
        if let Some(url) = &chain.explorer {
            println!("      explorer  {url}");
        }
        for token in &chain.tokens {
            println!("      token  {:<5} {}", token.symbol, token.address);
        }
        for c in &chain.contracts {
            println!("      infra  {:<16} {}", c.name, c.address);
        }
    }

    match mode {
        UpMode::Resume => {
            println!(
                "\n💾 Persistent: balances, txs & deployments survive `down` → `up --resume`."
            );
        }
        UpMode::Fresh if has_saved_session(base) => {
            println!(
                "\nℹ️  A saved session exists. Restore it with `up --resume`, or discard it with `up --reset`."
            );
        }
        _ => {}
    }

    println!("\nTear down with: wharfnet down");
    Ok(())
}

pub(crate) fn down_in(base: &Path, project: &str) -> Result<()> {
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
        if let Some(fork) = &chain.fork {
            println!("     fork     {fork}");
        }
        if let Some(url) = &chain.explorer {
            println!("     explorer {url}");
        }
        for token in &chain.tokens {
            println!(
                "     token    {:<5} {} ({} dec)",
                token.symbol, token.address, token.decimals
            );
        }
        for c in &chain.contracts {
            println!("     contract {:<16} {}", c.name, c.address);
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

fn wait_ready(port: u16, probe: &HealthProbe, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if probe_ready(port, probe) {
            return true;
        }
        sleep(Duration::from_secs(1));
    }
    false
}

/// Minimal, dependency-free readiness check driven by the engine's [`HealthProbe`]:
/// either POST a JSON-RPC call and look for a `result`, or GET a path and look for
/// an HTTP `200`.
fn probe_ready(port: u16, probe: &HealthProbe) -> bool {
    match probe {
        HealthProbe::JsonRpc { method } => {
            let body = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":[]}}"#);
            let request = format!(
                "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            match http_roundtrip(port, &request) {
                Some(response) => response.contains("\"result\""),
                None => false,
            }
        }
        HealthProbe::HttpGet { path } => {
            let request =
                format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
            match http_roundtrip(port, &request) {
                Some(response) => response.starts_with("HTTP/1.1 200"),
                None => false,
            }
        }
    }
}

/// Send a raw HTTP request to `127.0.0.1:port` and return the response text, or
/// `None` if the connection or exchange fails.
fn http_roundtrip(port: u16, request: &str) -> Option<String> {
    let addr = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect(&addr).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::manifest::{Account, ChainEntry, Contract, Token};
    use std::net::TcpListener;
    use tempfile::tempdir;

    /// The default engine set (two Anvil chains + one Starknet chain), with
    /// explorers off — the generic checks don't care about the `--ui` flag; the
    /// tests that do build their own engine set with it enabled.
    fn engines() -> Vec<Box<dyn Engine>> {
        engines_for(&Config::default(), false)
    }

    #[test]
    fn render_compose_has_header_and_all_services() {
        let out = render_compose(&engines(), StateMode::Ephemeral, &[]);
        assert!(out.starts_with("# Generated by wharfnet"));
        assert!(out.contains("services:"));
        assert!(out.contains("anvil-1:"));
        assert!(out.contains("anvil-2:"));
        assert!(out.contains("starknet-1:"));
    }

    #[test]
    fn render_compose_reflects_the_state_mode() {
        let ephemeral = render_compose(&engines(), StateMode::Ephemeral, &[]);
        assert!(ephemeral.contains("--load-state"));
        assert!(!ephemeral.contains("--state-interval"));

        let persistent = render_compose(&engines(), StateMode::Persistent, &[]);
        assert!(persistent.contains("session-anvil-1.json"));
        assert!(persistent.contains("session-anvil-2.json"));
        assert!(persistent.contains("--state-interval"));
        // The Starknet chain persists too, via its own session replay.
        assert!(persistent.contains("session-starknet-1.json"));
    }

    #[test]
    fn explorer_services_are_assigned_ports_and_configs_per_chain() {
        let svcs = explorer_services(&engines());
        assert_eq!(svcs.len(), 2, "one explorer per EVM chain");

        assert_eq!(svcs[0].service_name, "explorer-anvil-1");
        assert_eq!(svcs[0].host_port, 5100);
        assert_eq!(svcs[0].url, "http://127.0.0.1:5100");
        assert_eq!(svcs[0].config_rel_path, "state/otterscan-anvil-1.json");
        // anvil-1 is published on 8545, so the browser points there.
        assert!(
            svcs[0]
                .config_json
                .contains("\"erigonURL\": \"http://127.0.0.1:8545\"")
        );

        assert_eq!(svcs[1].service_name, "explorer-anvil-2");
        assert_eq!(svcs[1].host_port, 5101);
        assert!(
            svcs[1]
                .config_json
                .contains("\"erigonURL\": \"http://127.0.0.1:8546\"")
        );
    }

    #[test]
    fn explorer_flag_enables_the_starknet_web_ui_and_bare_disables_it() {
        // With the explorer on, the Starknet chain serves devnet's in-process web
        // UI via `--ui` (no companion Otterscan container — that's EVM-only).
        let out = render_compose(
            &engines_for(&Config::default(), true),
            StateMode::Ephemeral,
            &[],
        );
        assert!(
            out.contains("\"--ui\""),
            "explorer on → starknet devnet serves its web UI: {out}"
        );

        // `up --bare` (explorer off) drops the flag. Match the quoted command
        // token, not a bare substring — the template comment mentions `--ui`.
        let bare = render_compose(
            &engines_for(&Config::default(), false),
            StateMode::Ephemeral,
            &[],
        );
        assert!(
            !bare.contains("\"--ui\""),
            "bare must not serve the web UI: {bare}"
        );
    }

    #[test]
    fn render_compose_appends_explorer_services_when_requested() {
        let engines = engines();
        let svcs = explorer_services(&engines);
        let out = render_compose(&engines, StateMode::Ephemeral, &svcs);
        assert!(out.contains("explorer-anvil-1:"));
        assert!(out.contains("explorer-anvil-2:"));
        assert!(out.contains("otterscan/otterscan:"));
        assert!(out.contains("\"5100:80\""));
        assert!(out.contains("otterscan-anvil-1.json:/usr/share/nginx/html/config.json:ro"));
        assert!(!out.contains("{{"), "no placeholder should remain: {out}");
    }

    #[test]
    fn stage_explorer_configs_writes_each_config() {
        let dir = tempdir().unwrap();
        let svcs = explorer_services(&engines());
        stage_explorer_configs(dir.path(), &svcs).unwrap();
        let cfg = dir.path().join("state/otterscan-anvil-1.json");
        assert!(cfg.exists());
        assert!(
            fs::read_to_string(&cfg)
                .unwrap()
                .contains("http://127.0.0.1:8545")
        );
    }

    #[test]
    fn print_compose_is_ok() {
        assert!(print_compose(false, None).is_ok());
        assert!(print_compose(true, None).is_ok());
    }

    #[test]
    fn engines_returns_default_set() {
        let engines = engines();
        assert_eq!(engines.len(), 3);
        assert_eq!(engines[0].name(), "anvil-1");
        assert_eq!(engines[1].name(), "anvil-2");
        assert_eq!(engines[2].name(), "starknet-1");
    }

    #[test]
    fn engines_use_distinct_ports_and_chain_ids() {
        let engines = engines();
        let ports: Vec<u16> = engines.iter().map(|e| e.host_port()).collect();
        let chain_ids: Vec<String> = engines
            .iter()
            .map(|e| e.manifest_entry().chain_id)
            .collect();
        assert_eq!(ports, vec![8545, 8546, 5050]);
        assert_eq!(chain_ids, vec!["31337", "31338", "0x534e5f5345504f4c4941"]);
        // No two chains may share a host port or the compose file won't bind.
        assert_eq!(
            ports.iter().collect::<std::collections::HashSet<_>>().len(),
            3
        );
    }

    #[test]
    fn stage_files_writes_the_token_snapshot() {
        let dir = tempdir().unwrap();
        stage_files(dir.path(), &engines(), StateMode::Ephemeral).unwrap();
        let snapshot = dir.path().join("state/anvil-tokens.json");
        assert!(snapshot.exists(), "snapshot should be staged under state/");
        let data = fs::read_to_string(&snapshot).unwrap();
        // Sanity: the snapshot really contains our deployed USDC address.
        assert!(data.contains("5fbdb2315678afecb367f032d93f642f64180aa3"));
    }

    #[test]
    fn persistent_staging_seeds_sessions_but_never_clobbers_them() {
        let dir = tempdir().unwrap();

        // First persistent boot seeds a per-chain session from the baked snapshot.
        stage_files(dir.path(), &engines(), StateMode::Persistent).unwrap();
        let session = dir.path().join("state/session-anvil-1.json");
        assert!(session.exists(), "session should be seeded on first boot");
        assert!(
            fs::read_to_string(&session)
                .unwrap()
                .contains("5fbdb2315678afecb367f032d93f642f64180aa3")
        );
        // The Starknet chain seeds its session replay from the baked tokens too.
        assert!(
            dir.path().join("state/session-starknet-1.json").exists(),
            "starknet session should be seeded on first boot"
        );

        // Simulate accumulated runtime state, then re-stage: it must be preserved.
        fs::write(&session, "MY SAVED WORK").unwrap();
        stage_files(dir.path(), &engines(), StateMode::Persistent).unwrap();
        assert_eq!(fs::read_to_string(&session).unwrap(), "MY SAVED WORK");
    }

    #[test]
    fn clear_sessions_removes_only_session_snapshots() {
        let dir = tempdir().unwrap();
        let state = dir.path().join("state");
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("session-anvil-1.json"), "a").unwrap();
        fs::write(state.join("session-anvil-2.json"), "b").unwrap();
        fs::write(state.join("anvil-tokens.json"), "baked").unwrap();

        assert!(has_saved_session(dir.path()));
        clear_sessions(dir.path()).unwrap();
        assert!(!has_saved_session(dir.path()));
        // The baked snapshot is left intact.
        assert!(state.join("anvil-tokens.json").exists());
        assert!(!state.join("session-anvil-1.json").exists());
        assert!(!state.join("session-anvil-2.json").exists());
    }

    #[test]
    fn has_saved_session_false_without_state_dir() {
        let dir = tempdir().unwrap();
        assert!(!has_saved_session(dir.path()));
        // clear on a missing state dir is a no-op, not an error.
        assert!(clear_sessions(dir.path()).is_ok());
    }

    #[test]
    fn paths_are_joined_under_base() {
        let base = Path::new("/tmp/whatever");
        assert_eq!(
            compose_path(base),
            Path::new("/tmp/whatever/docker-compose.yml")
        );
        assert_eq!(
            manifest_path(base),
            Path::new("/tmp/whatever/wharfnet.json")
        );
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
            chain_id: "31337".into(),
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
            contracts: vec![Contract {
                name: "Multicall3".into(),
                address: "0xcA11bde05977b3631167028862bE2a173976CA11".into(),
            }],
            fork: None,
            explorer: Some("http://127.0.0.1:5100".into()),
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

    const EVM_PROBE: HealthProbe = HealthProbe::JsonRpc {
        method: "eth_chainId",
    };
    const HTTP_PROBE: HealthProbe = HealthProbe::HttpGet { path: "/is_alive" };

    /// Serve one connection with a fixed raw HTTP response.
    fn spawn_mock(response: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        port
    }

    const OK_RPC: &str = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"0x7a69\"}";

    #[test]
    fn probe_ready_true_when_json_rpc_result_present() {
        let port = spawn_mock(OK_RPC);
        std::thread::sleep(Duration::from_millis(50));
        assert!(probe_ready(port, &EVM_PROBE));
    }

    #[test]
    fn probe_ready_false_when_json_rpc_has_no_result() {
        let port =
            spawn_mock("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"id\":1,\"error\":\"boom\"}");
        std::thread::sleep(Duration::from_millis(50));
        assert!(!probe_ready(port, &EVM_PROBE));
    }

    #[test]
    fn probe_ready_true_when_http_get_returns_200() {
        let port = spawn_mock("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nAlive!!!");
        std::thread::sleep(Duration::from_millis(50));
        assert!(probe_ready(port, &HTTP_PROBE));
    }

    #[test]
    fn probe_ready_false_when_http_get_returns_non_200() {
        let port = spawn_mock("HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\n\r\n");
        std::thread::sleep(Duration::from_millis(50));
        assert!(!probe_ready(port, &HTTP_PROBE));
    }

    #[test]
    fn probe_ready_false_when_nothing_listening() {
        assert!(!probe_ready(1, &EVM_PROBE));
        assert!(!probe_ready(1, &HTTP_PROBE));
    }

    #[test]
    fn wait_ready_succeeds_when_server_is_up() {
        let port = spawn_mock(OK_RPC);
        std::thread::sleep(Duration::from_millis(50));
        assert!(wait_ready(port, &EVM_PROBE, Duration::from_secs(3)));
    }

    #[test]
    fn wait_ready_times_out_when_nothing_listening() {
        assert!(!wait_ready(2, &EVM_PROBE, Duration::from_millis(10)));
    }
}

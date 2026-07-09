//! Test-only helpers for the docker-backed end-to-end tests.
//!
//! [`Localnet::boot`] brings up a *real* single-chain Anvil localnet in an
//! isolated temp dir and compose project, so several e2e tests can run in
//! parallel without clashing on host ports, container names, or the shared
//! `.wharfnet` state dir. Every test that uses one first checks
//! [`docker_available`] and self-skips when Docker is absent (or when
//! `WHARFNET_SKIP_DOCKER_TESTS` is set, as CI does).

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::runtime::orchestrator::{self, UpMode};

/// Whether the docker-backed e2e tests should run. Honors the CI opt-out so the
/// heavy container tests don't run on machines (or pipelines) without Docker.
pub(crate) fn docker_available() -> bool {
    if std::env::var_os("WHARFNET_SKIP_DOCKER_TESTS").is_some() {
        return false;
    }
    Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A real single-chain localnet booted in an isolated temp dir + compose
/// project, torn down automatically when dropped. Construct it behind a
/// [`docker_available`] check.
pub(crate) struct Localnet {
    // The temp dir is created under the crate root (`.`), not the system temp
    // dir, so its bind-mounted `state/` sits under a path Docker Desktop shares
    // by default on macOS. Dropped after `Drop` runs, so teardown still sees it.
    dir: tempfile::TempDir,
    project: String,
    chain: String,
}

impl Localnet {
    /// Boot one Anvil chain named `chain` on host `port` / `chain_id`, isolated
    /// under its own temp dir and compose project. `block_time` is the
    /// auto-mining interval — pass a large value to effectively freeze
    /// auto-mining so block-count assertions stay deterministic. Panics on
    /// failure (this is test-only code).
    pub(crate) fn boot(chain: &str, port: u16, chain_id: u64, block_time: u64) -> Localnet {
        let dir = tempfile::TempDir::new_in(".").expect("create temp dir under crate root");
        // A single-chain topology on a dedicated high port keeps parallel e2e
        // tests from colliding with each other or a dev localnet on 8545/8546.
        let config = dir.path().join("wharfnet.toml");
        fs::write(
            &config,
            format!(
                "[[chains]]\nname = \"{chain}\"\nport = {port}\nchain_id = {chain_id}\nblock_time = {block_time}\n"
            ),
        )
        .expect("write test config");

        let net = Localnet {
            dir,
            project: format!("wharfnet-e2e-{chain}"),
            chain: chain.to_string(),
        };
        // Construct `net` first so a boot failure still tears down via `Drop`.
        orchestrator::up_in(
            net.base(),
            &net.project,
            UpMode::Fresh,
            false, // no explorer — keep the boot to just the chain (no image pull)
            Some(&config),
        )
        .expect("localnet should boot");
        net
    }

    pub(crate) fn base(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn project(&self) -> &str {
        &self.project
    }

    pub(crate) fn chain(&self) -> &str {
        &self.chain
    }
}

impl Drop for Localnet {
    fn drop(&mut self) {
        // Best-effort teardown; a leaked container would clash with a re-run.
        let _ = orchestrator::down_in(self.base(), &self.project);
    }
}

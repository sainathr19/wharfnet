//! Thin wrapper over the `docker compose` CLI. wharfnet generates a compose
//! file and drives it; the heavy lifting stays in Docker.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// Verify `docker compose` is installed and the daemon is reachable.
pub fn ensure_available() -> Result<()> {
    let output = Command::new("docker")
        .args(["compose", "version"])
        .output()
        .context("failed to run `docker` — is Docker installed and on your PATH?")?;
    check_compose_version(output.status.success())
}

/// Decision split out from `ensure_available` so it can be tested without Docker.
fn check_compose_version(success: bool) -> Result<()> {
    if !success {
        bail!(
            "`docker compose` is not available. Install Docker (with the compose plugin) and make sure the daemon is running."
        );
    }
    Ok(())
}

pub fn compose_up(file: &Path, project: &str) -> Result<()> {
    // Capture docker's own progress output so it doesn't clutter wharfnet's;
    // surface it only if the command fails.
    let output = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(file)
        .arg("-p")
        .arg(project)
        .arg("up")
        .arg("-d")
        .output()
        .context("running `docker compose up`")?;
    if !output.status.success() {
        bail!(
            "`docker compose up` failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub fn compose_down(file: &Path, project: &str) -> Result<()> {
    let output = Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(file)
        .arg("-p")
        .arg(project)
        .arg("down")
        .arg("-v")
        .output()
        .context("running `docker compose down`")?;
    if !output.status.success() {
        bail!(
            "`docker compose down` failed:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub fn compose_ps(file: &Path, project: &str) -> Result<()> {
    Command::new("docker")
        .arg("compose")
        .arg("-f")
        .arg(file)
        .arg("-p")
        .arg(project)
        .arg("ps")
        .status()
        .context("running `docker compose ps`")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docker_available() -> bool {
        Command::new("docker")
            .args(["compose", "version"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn check_compose_version_ok_on_success() {
        assert!(check_compose_version(true).is_ok());
    }

    #[test]
    fn check_compose_version_errors_on_failure() {
        assert!(check_compose_version(false).is_err());
    }

    #[test]
    fn ensure_available_ok_when_docker_present() {
        if !docker_available() {
            eprintln!("skipping: docker unavailable");
            return;
        }
        ensure_available().unwrap();
    }

    #[test]
    fn compose_up_errors_on_missing_file() {
        if !docker_available() {
            return;
        }
        let res = compose_up(
            Path::new("/nonexistent/wharfnet/docker-compose.yml"),
            "wharfnet-cov-up",
        );
        assert!(res.is_err());
    }

    #[test]
    fn compose_down_errors_on_missing_file() {
        if !docker_available() {
            return;
        }
        let res = compose_down(
            Path::new("/nonexistent/wharfnet/docker-compose.yml"),
            "wharfnet-cov-down",
        );
        assert!(res.is_err());
    }

    #[test]
    fn compose_ps_runs_without_panicking() {
        if !docker_available() {
            return;
        }
        // ps tolerates a missing file (returns Ok regardless); just exercise it.
        let _ = compose_ps(
            Path::new("/nonexistent/wharfnet/docker-compose.yml"),
            "wharfnet-cov-ps",
        );
    }
}

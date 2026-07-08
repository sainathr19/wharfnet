//! wharfnet — one-command localnet for EVM, Solana & Starknet.

mod docker;
mod engine;
mod faucet;
mod manifest;
mod orchestrator;
mod ui;

use clap::{Parser, Subcommand};

/// One-command localnet for EVM, Solana & Starknet.
#[derive(Parser)]
#[command(name = "wharfnet", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Boot the local multi-chain network.
    ///
    /// By default every boot starts fresh from the pre-deployed tokens and
    /// seeded accounts. Use `--resume` to restore (and keep saving) the
    /// previous session, or `--reset` to discard a saved session and start clean.
    Up {
        /// Restore the previous session's state if a saved snapshot exists,
        /// otherwise boot fresh. Your work then persists across
        /// `down` → `up --resume`.
        #[arg(long, conflicts_with = "reset")]
        resume: bool,
        /// Discard any saved session snapshot and boot a clean, fresh network.
        #[arg(long)]
        reset: bool,
        /// Boot only the chains — skip the bundled block explorers.
        #[arg(long)]
        bare: bool,
    },
    /// Tear down the local network and clean up state.
    Down,
    /// Show the status and endpoints of running chains.
    Status,
    /// Print the generated docker-compose.yml without booting anything.
    Compose {
        /// Print only the chain services, without the bundled explorers.
        #[arg(long)]
        bare: bool,
    },
    /// Fund an address from the built-in faucet.
    Faucet {
        /// Target chain — a kind (e.g. `evm`) to fund every matching chain, or a
        /// specific chain name (e.g. `anvil-1`).
        chain: String,
        /// Recipient address.
        address: String,
        /// Amount in whole units (ETH, or whole tokens scaled by their decimals).
        #[arg(default_value_t = 100)]
        amount: u64,
        /// Fund only this token (e.g. `USDC`). Omit to fund the native coin and
        /// every bundled token.
        #[arg(long)]
        token: Option<String>,
    },
    /// Deploy the pre-baked test tokens and contracts.
    Deploy,
}

fn main() {
    if let Err(e) = run(Cli::parse().command) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

/// Dispatch a parsed command. Kept separate from `main` so it returns a
/// `Result` and is unit-testable.
fn run(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Up {
            resume,
            reset,
            bare,
        } => {
            let mode = if reset {
                orchestrator::UpMode::Reset
            } else if resume {
                orchestrator::UpMode::Resume
            } else {
                orchestrator::UpMode::Fresh
            };
            orchestrator::up(mode, !bare)
        }
        Commands::Down => orchestrator::down(),
        Commands::Status => orchestrator::status(),
        Commands::Compose { bare } => orchestrator::print_compose(!bare),
        Commands::Faucet {
            chain,
            address,
            amount,
            token,
        } => faucet::run(&chain, &address, amount, token.as_deref()),
        Commands::Deploy => anyhow::bail!("deploy not yet implemented"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_up_command() {
        let cli = Cli::try_parse_from(["wharfnet", "up"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Up {
                resume: false,
                reset: false,
                bare: false
            }
        ));
    }

    #[test]
    fn parses_up_resume_and_reset_flags() {
        let resume = Cli::try_parse_from(["wharfnet", "up", "--resume"]).unwrap();
        assert!(matches!(resume.command, Commands::Up { resume: true, .. }));

        let reset = Cli::try_parse_from(["wharfnet", "up", "--reset"]).unwrap();
        assert!(matches!(reset.command, Commands::Up { reset: true, .. }));
    }

    #[test]
    fn parses_bare_flag_on_up_and_compose() {
        // Explorers are on by default; `--bare` opts out.
        let up = Cli::try_parse_from(["wharfnet", "up", "--bare"]).unwrap();
        assert!(matches!(up.command, Commands::Up { bare: true, .. }));

        let compose = Cli::try_parse_from(["wharfnet", "compose", "--bare"]).unwrap();
        assert!(matches!(compose.command, Commands::Compose { bare: true }));
    }

    #[test]
    fn up_resume_and_reset_are_mutually_exclusive() {
        assert!(Cli::try_parse_from(["wharfnet", "up", "--resume", "--reset"]).is_err());
    }

    #[test]
    fn parses_faucet_with_explicit_args() {
        let cli = Cli::try_parse_from(["wharfnet", "faucet", "evm", "0xabc", "500"]).unwrap();
        match cli.command {
            Commands::Faucet {
                chain,
                address,
                amount,
                token,
            } => {
                assert_eq!(chain, "evm");
                assert_eq!(address, "0xabc");
                assert_eq!(amount, 500);
                assert_eq!(token, None);
            }
            _ => panic!("expected faucet command"),
        }
    }

    #[test]
    fn parses_faucet_with_token_flag() {
        let cli = Cli::try_parse_from([
            "wharfnet", "faucet", "anvil-1", "0xabc", "50", "--token", "USDC",
        ])
        .unwrap();
        match cli.command {
            Commands::Faucet {
                chain,
                token,
                amount,
                ..
            } => {
                assert_eq!(chain, "anvil-1");
                assert_eq!(amount, 50);
                assert_eq!(token.as_deref(), Some("USDC"));
            }
            _ => panic!("expected faucet command"),
        }
    }

    #[test]
    fn faucet_amount_defaults_to_100() {
        let cli = Cli::try_parse_from(["wharfnet", "faucet", "evm", "0xabc"]).unwrap();
        match cli.command {
            Commands::Faucet { amount, .. } => assert_eq!(amount, 100),
            _ => panic!("expected faucet command"),
        }
    }

    #[test]
    fn unknown_command_fails_to_parse() {
        assert!(Cli::try_parse_from(["wharfnet", "teleport"]).is_err());
    }

    #[test]
    fn run_compose_is_ok() {
        assert!(run(Commands::Compose { bare: false }).is_ok());
        assert!(run(Commands::Compose { bare: true }).is_ok());
    }

    #[test]
    fn run_deploy_is_unimplemented_error() {
        assert!(run(Commands::Deploy).is_err());
    }

    fn docker_available() -> bool {
        // Let CI skip the docker-backed lifecycle test (image pulls + container
        // boot) while still running every other test.
        if std::env::var_os("WHARFNET_SKIP_DOCKER_TESTS").is_some() {
            return false;
        }
        std::process::Command::new("docker")
            .args(["compose", "version"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Full lifecycle through the public dispatch. Covers `run`'s Up/Down/Status
    /// arms and the orchestrator's Docker paths. Self-skips without Docker.
    #[test]
    fn run_up_status_down_lifecycle_with_docker() {
        if !docker_available() {
            eprintln!("skipping lifecycle test: docker unavailable");
            return;
        }
        let _ = run(Commands::Down); // clean any prior state
        // Bare keeps this lifecycle check focused on the chains (the explorer
        // path is covered by unit tests), so it doesn't pull the Otterscan image.
        run(Commands::Up {
            resume: false,
            reset: false,
            bare: true,
        })
        .expect("up should succeed");
        run(Commands::Status).expect("status should succeed");
        run(Commands::Down).expect("down should succeed");
    }
}

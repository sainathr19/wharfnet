//! wharfnet — one-command localnet for EVM, Solana & Starknet.

mod docker;
mod engine;
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
    Up,
    /// Tear down the local network and clean up state.
    Down,
    /// Show the status and endpoints of running chains.
    Status,
    /// Print the generated docker-compose.yml without booting anything.
    Compose,
    /// Fund an address from the built-in faucet.
    Faucet {
        /// Target chain (e.g. evm, solana, starknet).
        chain: String,
        /// Recipient address.
        address: String,
        /// Amount to send (chain-native units).
        #[arg(default_value_t = 100)]
        amount: u64,
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
        Commands::Up => orchestrator::up(),
        Commands::Down => orchestrator::down(),
        Commands::Status => orchestrator::status(),
        Commands::Compose => orchestrator::print_compose(),
        Commands::Faucet {
            chain,
            address,
            amount,
        } => anyhow::bail!(
            "faucet not yet implemented — would send {amount} on {chain} to {address}"
        ),
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
        assert!(matches!(cli.command, Commands::Up));
    }

    #[test]
    fn parses_faucet_with_explicit_args() {
        let cli = Cli::try_parse_from(["wharfnet", "faucet", "evm", "0xabc", "500"]).unwrap();
        match cli.command {
            Commands::Faucet {
                chain,
                address,
                amount,
            } => {
                assert_eq!(chain, "evm");
                assert_eq!(address, "0xabc");
                assert_eq!(amount, 500);
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
        assert!(run(Commands::Compose).is_ok());
    }

    #[test]
    fn run_faucet_is_unimplemented_error() {
        let err = run(Commands::Faucet {
            chain: "evm".into(),
            address: "0x0".into(),
            amount: 1,
        });
        assert!(err.is_err());
    }

    #[test]
    fn run_deploy_is_unimplemented_error() {
        assert!(run(Commands::Deploy).is_err());
    }

    fn docker_available() -> bool {
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
        run(Commands::Up).expect("up should succeed");
        run(Commands::Status).expect("status should succeed");
        run(Commands::Down).expect("down should succeed");
    }
}

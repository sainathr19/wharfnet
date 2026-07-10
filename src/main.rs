//! wharfnet — one-command localnet for EVM, Solana & Starknet.

mod evm;
mod faucet;
mod runtime;
mod starknet;
#[cfg(test)]
mod testkit;

use clap::{Parser, Subcommand};
use evm::control;
use runtime::orchestrator;
use std::path::PathBuf;

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
        /// Path to a config file (default: ./wharfnet.toml, or $WHARFNET_CONFIG).
        #[arg(long, short = 'c', value_name = "PATH")]
        config: Option<PathBuf>,
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
        /// Path to a config file (default: ./wharfnet.toml, or $WHARFNET_CONFIG).
        #[arg(long, short = 'c', value_name = "PATH")]
        config: Option<PathBuf>,
    },
    /// Fund an address from the built-in faucet.
    Faucet {
        /// Target chain — a kind (`evm`, `starknet`) to fund every matching
        /// chain, or a specific chain name (e.g. `anvil-1`, `starknet-1`).
        chain: String,
        /// Recipient address.
        address: String,
        /// Amount in whole units (native coin, or whole tokens scaled by their
        /// decimals).
        #[arg(default_value_t = 100)]
        amount: u64,
        /// Fund only this token (e.g. `USDC`). Omit to fund the native coin(s)
        /// and every bundled token.
        #[arg(long)]
        token: Option<String>,
    },
    /// EVM chain controls: mine, advance time, impersonate, snapshot/revert.
    Evm {
        #[command(subcommand)]
        command: EvmCommands,
    },
    /// Deploy the pre-baked test tokens and contracts.
    Deploy,
}

/// EVM-specific chain controls, grouped under `wharfnet evm …`. These wrap
/// Anvil's cheat RPCs, so they're intentionally not root commands — other chain
/// kinds get their own namespaces with their own verbs.
#[derive(Subcommand)]
enum EvmCommands {
    /// Mine blocks on a chain.
    Mine {
        /// Number of blocks to mine.
        #[arg(default_value_t = 1)]
        count: u64,
        /// Target chain — `evm` for every EVM chain, or a name like `anvil-1`.
        #[arg(long, default_value = "evm")]
        chain: String,
    },
    /// Fast-forward chain time by N seconds (mines a block to apply it).
    IncreaseTime {
        /// Seconds to advance.
        seconds: u64,
        #[arg(long, default_value = "evm")]
        chain: String,
    },
    /// Set the next block's timestamp to an absolute Unix time (mines a block).
    Warp {
        /// Unix timestamp in seconds.
        timestamp: u64,
        #[arg(long, default_value = "evm")]
        chain: String,
    },
    /// Impersonate an account so you can send txs as it — no private key needed.
    Impersonate {
        /// Address to impersonate.
        address: String,
        /// Stop impersonating the address instead.
        #[arg(long)]
        stop: bool,
        #[arg(long, default_value = "evm")]
        chain: String,
    },
    /// Snapshot chain state; prints an id you can `revert` to later.
    Snapshot {
        #[arg(long, default_value = "evm")]
        chain: String,
    },
    /// Revert chain state to a snapshot id from `wharfnet evm snapshot`.
    Revert {
        /// Snapshot id (e.g. `0x1`).
        id: String,
        #[arg(long, default_value = "evm")]
        chain: String,
    },
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
            config,
        } => {
            let mode = if reset {
                orchestrator::UpMode::Reset
            } else if resume {
                orchestrator::UpMode::Resume
            } else {
                orchestrator::UpMode::Fresh
            };
            orchestrator::up(mode, !bare, config.as_deref())
        }
        Commands::Down => orchestrator::down(),
        Commands::Status => orchestrator::status(),
        Commands::Compose { bare, config } => orchestrator::print_compose(!bare, config.as_deref()),
        Commands::Faucet {
            chain,
            address,
            amount,
            token,
        } => faucet::run(&chain, &address, amount, token.as_deref()),
        Commands::Evm { command } => match command {
            EvmCommands::Mine { count, chain } => control::mine(&chain, count),
            EvmCommands::IncreaseTime { seconds, chain } => control::increase_time(&chain, seconds),
            EvmCommands::Warp { timestamp, chain } => control::warp(&chain, timestamp),
            EvmCommands::Impersonate {
                address,
                stop,
                chain,
            } => control::impersonate(&chain, &address, stop),
            EvmCommands::Snapshot { chain } => control::snapshot(&chain),
            EvmCommands::Revert { id, chain } => control::revert(&chain, &id),
        },
        Commands::Deploy => anyhow::bail!("deploy not yet implemented"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::docker_available;
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
                bare: false,
                config: None
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
        assert!(matches!(
            compose.command,
            Commands::Compose { bare: true, .. }
        ));
    }

    #[test]
    fn parses_config_path_on_up_and_compose() {
        let up = Cli::try_parse_from(["wharfnet", "up", "-c", "custom.toml"]).unwrap();
        assert!(matches!(
            up.command,
            Commands::Up { config: Some(ref p), .. } if p.as_os_str() == "custom.toml"
        ));

        let compose = Cli::try_parse_from(["wharfnet", "compose", "--config", "a/b.toml"]).unwrap();
        assert!(matches!(
            compose.command,
            Commands::Compose { config: Some(ref p), .. } if p.as_os_str() == "a/b.toml"
        ));
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
    fn parses_evm_control_commands() {
        // `evm mine`: count defaults to 1, chain defaults to evm.
        let cli = Cli::try_parse_from(["wharfnet", "evm", "mine"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Evm {
                command: EvmCommands::Mine { count: 1, ref chain }
            } if chain == "evm"
        ));

        let cli =
            Cli::try_parse_from(["wharfnet", "evm", "mine", "10", "--chain", "anvil-2"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Evm {
                command: EvmCommands::Mine { count: 10, ref chain }
            } if chain == "anvil-2"
        ));

        // `evm impersonate --stop`
        let cli =
            Cli::try_parse_from(["wharfnet", "evm", "impersonate", "0xabc", "--stop"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Evm {
                command: EvmCommands::Impersonate { stop: true, .. }
            }
        ));

        // `evm revert <id>`
        let cli = Cli::try_parse_from(["wharfnet", "evm", "revert", "0x1"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Evm {
                command: EvmCommands::Revert { ref id, .. }
            } if id == "0x1"
        ));

        // the old flat forms no longer exist at root
        assert!(Cli::try_parse_from(["wharfnet", "mine"]).is_err());
    }

    #[test]
    fn unknown_command_fails_to_parse() {
        assert!(Cli::try_parse_from(["wharfnet", "teleport"]).is_err());
    }

    #[test]
    fn run_compose_is_ok() {
        assert!(
            run(Commands::Compose {
                bare: false,
                config: None
            })
            .is_ok()
        );
        assert!(
            run(Commands::Compose {
                bare: true,
                config: None
            })
            .is_ok()
        );
    }

    #[test]
    fn run_deploy_is_unimplemented_error() {
        assert!(run(Commands::Deploy).is_err());
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
            config: None,
        })
        .expect("up should succeed");
        run(Commands::Status).expect("status should succeed");
        run(Commands::Down).expect("down should succeed");
    }
}

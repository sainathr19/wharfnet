//! The `wharfnet` command-line interface — argument parsing and dispatch.
//!
//! Kept in the library (rather than `main.rs`) so the binary is a thin shim and
//! the CLI stays unit-testable alongside the rest of the crate.

use crate::evm::control;
use crate::faucet;
use crate::runtime::orchestrator;
use crate::solana::control as sol_control;
use crate::starknet::control as sn_control;
use clap::{Parser, Subcommand};
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
    /// Stream container logs, optionally for one chain or kind.
    Logs {
        /// Chain kind (`evm`, `starknet`) or a chain name (`anvil-1`). Omit for all.
        chain: Option<String>,
        /// Keep streaming new output, like `tail -f`.
        #[arg(long, short = 'f')]
        follow: bool,
    },
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
        /// Target chain — a kind (`evm`, `starknet`, `solana`) to fund every
        /// matching chain, or a specific chain name (e.g. `anvil-1`, `solana-1`).
        chain: String,
        /// Recipient address.
        address: String,
        /// Amount to fund: a decimal number of whole units (e.g. `1.5`), scaled
        /// by the token's decimals. With `--raw`, an exact base-unit integer.
        #[arg(default_value = "100")]
        amount: String,
        /// Fund only this token (e.g. `USDC`). Omit to fund the native coin(s)
        /// and every bundled token.
        #[arg(long)]
        token: Option<String>,
        /// Treat `amount` as raw base units (wei/fri, or a token's smallest unit).
        #[arg(long)]
        raw: bool,
    },
    /// EVM chain controls: mine, advance time, impersonate, snapshot/revert.
    Evm {
        #[command(subcommand)]
        command: EvmCommands,
    },
    /// Starknet chain controls: create blocks, advance time, impersonate.
    Starknet {
        #[command(subcommand)]
        command: StarknetCommands,
    },
    /// Solana chain controls: advance slots, travel through time, pause the clock.
    Solana {
        #[command(subcommand)]
        command: SolanaCommands,
    },
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

/// Starknet-specific chain controls, grouped under `wharfnet starknet …`. These
/// wrap starknet-devnet's cheat JSON-RPC methods. There is no `snapshot`/`revert`
/// (devnet has no numbered-snapshot mechanism), and `impersonate` needs a forked
/// chain — otherwise they mirror the `wharfnet evm` verbs.
#[derive(Subcommand)]
enum StarknetCommands {
    /// Create blocks on a chain.
    Mine {
        /// Number of blocks to create.
        #[arg(default_value_t = 1)]
        count: u64,
        /// Target chain — `starknet` for every Starknet chain, or a name like
        /// `starknet-1`.
        #[arg(long, default_value = "starknet")]
        chain: String,
    },
    /// Fast-forward chain time by N seconds (generates a block to apply it).
    IncreaseTime {
        /// Seconds to advance.
        seconds: u64,
        #[arg(long, default_value = "starknet")]
        chain: String,
    },
    /// Set the chain time to an absolute Unix timestamp (generates a block).
    Warp {
        /// Unix timestamp in seconds.
        timestamp: u64,
        #[arg(long, default_value = "starknet")]
        chain: String,
    },
    /// Impersonate an account so you can send txs as it — no private key needed.
    /// Requires a forked chain (devnet only impersonates in forking mode).
    Impersonate {
        /// Address to impersonate.
        address: String,
        /// Stop impersonating the address instead.
        #[arg(long)]
        stop: bool,
        #[arg(long, default_value = "starknet")]
        chain: String,
    },
}

/// Solana-specific chain controls, grouped under `wharfnet solana …`. These wrap
/// surfpool's `surfnet_*` cheat JSON-RPC methods. `warp` takes a Unix timestamp
/// and is forward-only (surfpool cannot rewind); there is no `impersonate` or
/// `snapshot`/`revert`; and `pause-clock`/`resume-clock` freeze and restart
/// surfpool's automatic slot production for step-by-step control.
#[derive(Subcommand)]
enum SolanaCommands {
    /// Advance the chain by N slots.
    Mine {
        /// Number of slots to advance.
        #[arg(default_value_t = 1)]
        count: u64,
        /// Target chain — `solana` for every Solana chain, or a name like
        /// `solana-1`.
        #[arg(long, default_value = "solana")]
        chain: String,
    },
    /// Fast-forward chain time by N seconds.
    IncreaseTime {
        /// Seconds to advance.
        seconds: u64,
        #[arg(long, default_value = "solana")]
        chain: String,
    },
    /// Set the chain clock to an absolute Unix timestamp (forward-only).
    Warp {
        /// Unix timestamp in seconds.
        timestamp: u64,
        #[arg(long, default_value = "solana")]
        chain: String,
    },
    /// Freeze automatic slot production until `resume-clock`.
    PauseClock {
        #[arg(long, default_value = "solana")]
        chain: String,
    },
    /// Resume automatic slot production after `pause-clock`.
    ResumeClock {
        #[arg(long, default_value = "solana")]
        chain: String,
    },
}

/// Parse the process arguments and run the CLI, exiting non-zero on error.
/// Called by the thin `main.rs` binary.
pub fn main() {
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
        Commands::Logs { chain, follow } => orchestrator::logs(chain.as_deref(), follow),
        Commands::Compose { bare, config } => orchestrator::print_compose(!bare, config.as_deref()),
        Commands::Faucet {
            chain,
            address,
            amount,
            token,
            raw,
        } => faucet::run(&chain, &address, &amount, token.as_deref(), raw),
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
        Commands::Starknet { command } => match command {
            StarknetCommands::Mine { count, chain } => sn_control::mine(&chain, count),
            StarknetCommands::IncreaseTime { seconds, chain } => {
                sn_control::increase_time(&chain, seconds)
            }
            StarknetCommands::Warp { timestamp, chain } => sn_control::warp(&chain, timestamp),
            StarknetCommands::Impersonate {
                address,
                stop,
                chain,
            } => sn_control::impersonate(&chain, &address, stop),
        },
        Commands::Solana { command } => match command {
            SolanaCommands::Mine { count, chain } => sol_control::mine(&chain, count),
            SolanaCommands::IncreaseTime { seconds, chain } => {
                sol_control::increase_time(&chain, seconds)
            }
            SolanaCommands::Warp { timestamp, chain } => sol_control::warp(&chain, timestamp),
            SolanaCommands::PauseClock { chain } => sol_control::pause_clock(&chain),
            SolanaCommands::ResumeClock { chain } => sol_control::resume_clock(&chain),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::docker_available;
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
                raw,
            } => {
                assert_eq!(chain, "evm");
                assert_eq!(address, "0xabc");
                assert_eq!(amount, "500");
                assert_eq!(token, None);
                assert!(!raw);
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
                assert_eq!(amount, "50");
                assert_eq!(token.as_deref(), Some("USDC"));
            }
            _ => panic!("expected faucet command"),
        }
    }

    #[test]
    fn faucet_amount_defaults_to_100() {
        let cli = Cli::try_parse_from(["wharfnet", "faucet", "evm", "0xabc"]).unwrap();
        match cli.command {
            Commands::Faucet { amount, .. } => assert_eq!(amount, "100"),
            _ => panic!("expected faucet command"),
        }
    }

    #[test]
    fn parses_faucet_raw_and_decimal_amount() {
        let cli =
            Cli::try_parse_from(["wharfnet", "faucet", "evm", "0xabc", "1.5", "--raw"]).unwrap();
        match cli.command {
            Commands::Faucet { amount, raw, .. } => {
                assert_eq!(amount, "1.5");
                assert!(raw);
            }
            _ => panic!("expected faucet command"),
        }
    }

    #[test]
    fn parses_logs_command() {
        // No arg → all services, not following.
        let all = Cli::try_parse_from(["wharfnet", "logs"]).unwrap();
        assert!(matches!(
            all.command,
            Commands::Logs {
                chain: None,
                follow: false
            }
        ));

        // A selector plus `-f` to follow.
        let one = Cli::try_parse_from(["wharfnet", "logs", "anvil-1", "-f"]).unwrap();
        assert!(matches!(
            one.command,
            Commands::Logs { chain: Some(ref c), follow: true } if c == "anvil-1"
        ));
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
    fn parses_starknet_control_commands() {
        // `starknet mine`: count defaults to 1, chain defaults to starknet.
        let cli = Cli::try_parse_from(["wharfnet", "starknet", "mine"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Starknet {
                command: StarknetCommands::Mine { count: 1, ref chain }
            } if chain == "starknet"
        ));

        // `starknet increase-time <secs> --chain <name>`
        let cli = Cli::try_parse_from([
            "wharfnet",
            "starknet",
            "increase-time",
            "3600",
            "--chain",
            "starknet-1",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Starknet {
                command: StarknetCommands::IncreaseTime { seconds: 3600, ref chain }
            } if chain == "starknet-1"
        ));

        // `starknet impersonate <addr> --stop`
        let cli = Cli::try_parse_from(["wharfnet", "starknet", "impersonate", "0x123", "--stop"])
            .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Starknet {
                command: StarknetCommands::Impersonate { stop: true, .. }
            }
        ));

        // There's no snapshot/revert under starknet (no devnet analogue).
        assert!(Cli::try_parse_from(["wharfnet", "starknet", "snapshot"]).is_err());
    }

    #[test]
    fn parses_solana_control_commands() {
        // `solana mine`: count defaults to 1, chain defaults to solana.
        let cli = Cli::try_parse_from(["wharfnet", "solana", "mine"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Solana {
                command: SolanaCommands::Mine { count: 1, ref chain }
            } if chain == "solana"
        ));

        // `solana warp <ts> --chain <name>`
        let cli = Cli::try_parse_from([
            "wharfnet",
            "solana",
            "warp",
            "2000000000",
            "--chain",
            "solana-1",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Commands::Solana {
                command: SolanaCommands::Warp { timestamp: 2_000_000_000, ref chain }
            } if chain == "solana-1"
        ));

        // `solana pause-clock` / `resume-clock` take no positional args.
        let cli = Cli::try_parse_from(["wharfnet", "solana", "pause-clock"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Solana {
                command: SolanaCommands::PauseClock { ref chain }
            } if chain == "solana"
        ));
        assert!(Cli::try_parse_from(["wharfnet", "solana", "resume-clock"]).is_ok());

        // There's no impersonate/snapshot under solana (no surfpool analogue).
        assert!(Cli::try_parse_from(["wharfnet", "solana", "impersonate", "x"]).is_err());
        assert!(Cli::try_parse_from(["wharfnet", "solana", "snapshot"]).is_err());
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

    /// `compose` renders from config + templates only — no manifest, no Docker —
    /// so it exercises that dispatch arm and the renderer deterministically.
    #[test]
    fn run_compose_renders_without_docker() {
        run(Commands::Compose {
            bare: true,
            config: None,
        })
        .expect("compose --bare renders");
        run(Commands::Compose {
            bare: false,
            config: None,
        })
        .expect("compose with explorers renders");
    }
}

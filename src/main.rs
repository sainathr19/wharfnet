//! wharfnet — one-command localnet for EVM, Solana & Starknet.
//!
//! This is an early scaffold: the CLI surface is defined, command bodies are
//! stubs to be implemented incrementally.

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
    /// Boot the local multi-chain network (EVM, Solana, Starknet).
    Up,
    /// Tear down the local network and clean up state.
    Down,
    /// Show the status and endpoints of running chains.
    Status,
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
    let cli = Cli::parse();

    match cli.command {
        Commands::Up => todo("up: boot the multi-chain localnet"),
        Commands::Down => todo("down: tear down the localnet"),
        Commands::Status => todo("status: list running chains and endpoints"),
        Commands::Faucet {
            chain,
            address,
            amount,
        } => todo(&format!("faucet: send {amount} on {chain} to {address}")),
        Commands::Deploy => todo("deploy: deploy test tokens and contracts"),
    }
}

fn todo(what: &str) {
    eprintln!("wharfnet: not yet implemented — {what}");
    std::process::exit(1);
}

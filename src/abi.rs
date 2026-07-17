//! Embedded contract interfaces (ABIs) for the bundled test tokens, so tests
//! can instantiate the tokens without fetching or hand-writing an ABI.
//!
//! The EVM ABIs are emitted by `solc` from the Solidity sources in
//! `resources/contracts/src/`; the Starknet ABIs are the `abi` section of the
//! compiled Cairo contract classes. Both are shipped as JSON strings — feed them
//! straight to viem/ethers/alloy (EVM) or starknet.js/starknet-rust (Starknet).
//!
//! Solana's test tokens are plain SPL mints on the native SPL Token program, so
//! there's no custom program interface to ship — use `@solana/spl-token`.

/// EVM token ABIs (JSON), as emitted by `solc`.
pub mod evm {
    /// Standard mintable ERC-20 — covers `USDC` and `WBTC`.
    pub const TEST_TOKEN: &str = include_str!("resources/abi/evm/test_token.json");
    /// Fee-on-transfer token — `FEE`.
    pub const FEE_TOKEN: &str = include_str!("resources/abi/evm/fee_token.json");
    /// Rebasing token (adds `rebase(uint256)`) — `REB`.
    pub const REBASING_TOKEN: &str = include_str!("resources/abi/evm/rebasing_token.json");
    /// USDT-style token whose `transfer`/`approve` return nothing — `NRT`.
    pub const NO_RETURN_TOKEN: &str = include_str!("resources/abi/evm/no_return_token.json");
}

/// Starknet (Cairo) token ABIs (JSON), from the compiled contract classes.
pub mod starknet {
    /// Standard mintable Cairo ERC-20 — covers `USDC` and `WBTC`.
    pub const TEST_TOKEN: &str = include_str!("resources/abi/starknet/test_token.json");
    /// Fee-on-transfer token — `FEE`.
    pub const FEE_TOKEN: &str = include_str!("resources/abi/starknet/fee_token.json");
    /// Rebasing token (adds `rebase`) — `REB`.
    pub const REBASING_TOKEN: &str = include_str!("resources/abi/starknet/rebasing_token.json");
}

/// The ABI (JSON) for a bundled test token, keyed by chain `kind` (`"evm"` or
/// `"starknet"`) and token `symbol`.
///
/// Returns `None` when the token's interface isn't shipped here: Solana SPL
/// tokens (standard SPL Token program) and the Starknet `ETH`/`STRK` fee tokens
/// (provided by devnet, not by wharfnet).
pub fn token_abi(kind: &str, symbol: &str) -> Option<&'static str> {
    match (kind, symbol) {
        ("evm", "USDC" | "WBTC") => Some(evm::TEST_TOKEN),
        ("evm", "FEE") => Some(evm::FEE_TOKEN),
        ("evm", "REB") => Some(evm::REBASING_TOKEN),
        ("evm", "NRT") => Some(evm::NO_RETURN_TOKEN),
        ("starknet", "USDC" | "WBTC") => Some(starknet::TEST_TOKEN),
        ("starknet", "FEE") => Some(starknet::FEE_TOKEN),
        ("starknet", "REB") => Some(starknet::REBASING_TOKEN),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_abi_is_a_valid_json_array() {
        for abi in [
            evm::TEST_TOKEN,
            evm::FEE_TOKEN,
            evm::REBASING_TOKEN,
            evm::NO_RETURN_TOKEN,
            starknet::TEST_TOKEN,
            starknet::FEE_TOKEN,
            starknet::REBASING_TOKEN,
        ] {
            let v: serde_json::Value = serde_json::from_str(abi).expect("ABI must be valid JSON");
            assert!(v.is_array(), "an ABI should be a JSON array of entries");
        }
    }

    #[test]
    fn maps_known_tokens_and_declines_the_rest() {
        assert!(token_abi("evm", "USDC").is_some());
        assert!(token_abi("evm", "NRT").is_some());
        assert!(token_abi("starknet", "REB").is_some());
        // Standard / not-shipped interfaces.
        assert!(token_abi("solana", "USDC").is_none());
        assert!(token_abi("starknet", "ETH").is_none());
        assert!(token_abi("evm", "NOPE").is_none());
    }

    #[test]
    fn evm_test_token_exposes_the_public_mint() {
        // A quick sanity check that the emitted ABI is the mintable one.
        assert!(evm::TEST_TOKEN.contains("\"mint\""));
    }
}

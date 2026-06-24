# wharfnet

**One-command localnet for EVM, Solana & Starknet — built-in faucet, pre-deployed test tokens and more.**

> ⚠️ Early WIP. The CLI surface is scaffolded; command implementations are landing incrementally.

`wharfnet` is the local harbor for your chains: boot EVM, Solana, and Starknet
networks locally with a single command, fund accounts from a unified faucet,
and get standard test tokens deployed at known addresses — then wire it straight
into your integration tests and CI pipelines.

## Why

Cross-chain and multi-VM teams stitch together Anvil, a Solana test validator,
and a Starknet devnet by hand — plus a homemade faucet and glue scripts — every
time they need a local environment. `wharfnet` packages that stitching into one
opinionated, reproducible tool so you can `up` a whole multi-chain stack and
test against it locally or in CI.

## Planned scope (v1)

- **Chains:** 2 EVM chains, Solana, Starknet
- **Faucet:** one API to fund accounts on every chain
- **Test tokens:** standard ERC-20 / SPL / Cairo tokens pre-deployed at known addresses
- **Endpoints manifest:** a single machine-readable file of RPC URLs, chain IDs, and addresses
- **CI integration:** spin up, wait-for-ready, and tear down from a pipeline

## Quickstart (target UX)

```sh
# build
cargo build --release

# boot the local multi-chain network
wharfnet up

# check what's running
wharfnet status

# fund an address
wharfnet faucet evm 0xabc... 100

# deploy the bundled test tokens
wharfnet deploy

# shut everything down
wharfnet down
```

## Status

This repository currently contains the project scaffold and CLI skeleton.
Engine wrappers (EVM / Solana / Starknet), the faucet, token presets, and the
CI helper are in progress.

## License

[MIT](./LICENSE)

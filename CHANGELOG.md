# Changelog

All notable changes to **wharfnet** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

wharfnet is early WIP: nothing has been released yet, so everything below sits
under _Unreleased_ and the CLI surface may still change.

## [Unreleased]

### Added

- **Config file (`wharfnet.toml`)** — customise the chain topology (name, port,
  `chain_id`, `block_time`, and how many chains). Optional: without one, wharfnet
  stays zero-config with its two default Anvil chains. Validated on load (unique
  names/ports/ids, `evm`-only for now).
- **EVM chain-control commands** — `wharfnet evm mine`, `increase-time`, `warp`,
  `impersonate`, `snapshot`, and `revert` wrap Anvil's cheat RPCs to drive a
  running localnet: advance blocks/time, send as any address without a key, and
  snapshot/revert state for test isolation. Grouped under `evm` so each chain
  kind can own its own control verbs.
- **Release pipeline** — pushing a `vX.Y.Z` tag publishes to crates.io via a
  tag-gated workflow: it checks the tag matches `Cargo.toml`, runs tests +
  `cargo audit`, publishes through crates.io Trusted Publishing (OIDC, no stored
  token), and cuts a GitHub Release. A weekly `cargo audit` job scans
  dependencies for advisories. See [RELEASING.md](RELEASING.md).
- **Block explorer (on by default, `up --bare` to skip)** — boots an Otterscan
  instance per EVM chain, wired to the chain's RPC and listed in the manifest.
  Anvil's native `ots_*` API means no indexer, so every `up` is browsable out of
  the box; `--bare` runs just the chains.
- **Persistent state (`up --resume` / `up --reset`)** — resume your last session
  or wipe it and start clean; plain `up` still boots fresh. Keeps balances, txs,
  and deployments across `down` so you can stop and pick up where you left off.
- **Faucet** — `faucet <chain> <address> [amount] [--token SYMBOL]` funds native
  ETH plus every token, or just one, in a single command with no private key.
- **Pre-deployed test tokens** — USDC and WBTC at fixed addresses on every EVM
  chain, each with a public `mint`, so tests use standard tokens with no deploy step.
- **Second EVM chain** — `anvil-2` (`:8546`, chainId `31338`) alongside `anvil-1`,
  for local cross-chain and bridging tests.
- **First EVM chain** — `up` boots a local Anvil chain (`anvil-1`, `:8545`,
  chainId `31337`) via Docker Compose and writes an endpoints manifest: one
  command to a running EVM localnet.
- **CLI scaffold** — Rust/`clap` command surface (`up`, `down`, `status`,
  `compose`, `faucet`, `deploy`) that everything else builds on.

### Notes

- `deploy` is scaffolded but not yet implemented.
- EVM only so far; Solana and Starknet engines are planned.

[Unreleased]: https://github.com/sainathr19/wharfnet/commits/main

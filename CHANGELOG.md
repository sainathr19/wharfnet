# Changelog

All notable changes to **wharfnet** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

wharfnet is early WIP: nothing has been released yet, so everything below sits
under _Unreleased_ and the CLI surface may still change.

## [Unreleased]

### Added

- **Starknet chains** — a `starknet-devnet` chain (`starknet-1`, `:5050`) now
  boots **by default** alongside the two Anvil chains, with deterministic
  predeployed accounts, the ETH/STRK fee tokens, and baked **Cairo test tokens**
  (USDC, WBTC, plus fee-on-transfer FEE and rebasing REB) at fixed addresses —
  all in the unified `status`/manifest. The chain kind is selectable in
  `wharfnet.toml` with `kind = "starknet"`.
- **Starknet faucet** — the same `faucet <chain> <address> [amount] [--token]`
  command funds Starknet addresses: ETH and STRK through devnet's `devnet_mint`
  cheat, and the Cairo test tokens through a signed invoke of their public `mint`
  (submitted by a predeployed dev account, so the recipient needs no key). Amounts
  are whole units scaled by decimals; funding is additive. Signing uses the
  maintained [`starknet-rust`](https://github.com/software-mansion/starknet-rust)
  client on **JSON-RPC 0.10**, matching the pinned devnet image and public testnet.
- **Starknet persistence** — `up --resume` / `up --reset` now cover Starknet
  chains too. Each persists to its own `session-<chain>.json` replay log, seeded
  once from the baked tokens and then dumped on every block (devnet mines one per
  transaction), so balances, mints, and deployments survive `down` → `up --resume`
  and are wiped by `up --reset` — matching the EVM chains.
- **Mainnet forking** — point a chain at a live RPC with `fork_url` (and optional
  `fork_block`) in `wharfnet.toml` and it boots as a fork of that network via
  Anvil's `--fork-url`, so you can test against real balances, contracts, and
  storage locally. `${VAR}` in `fork_url` is expanded from the environment so an
  RPC key stays out of the file, and only a redacted `scheme://host` is recorded.
  A forked chain skips the baked test tokens/contracts since it mirrors live state.
- **Presets — canonical contracts & weird tokens** — every EVM chain now boots
  with Multicall3, Permit2, and the CREATE2 deployer at their real chain-agnostic
  addresses (Multicall3/Permit2 etched from mainnet bytecode), so viem/ethers/
  wagmi and CREATE2 deploy tooling work out of the box. Adds three deliberately
  non-standard test tokens — fee-on-transfer (FEE), rebasing (REB), and
  no-return-value (NRT) — for token-integration testing against real-world quirks.
  All baked into the state snapshot; contracts are listed in the manifest and by
  `status`.
- **Config file (`wharfnet.toml`)** — customise the chain topology (name, port,
  `chain_id`, `block_time`, and how many chains). Optional: without one, wharfnet
  stays zero-config with its two default Anvil chains. Validated on load (unique
  names/ports/ids, `evm`-only for now). Defaults to `./wharfnet.toml`; override
  with `--config`/`-c` on `up`/`compose` or the `WHARFNET_CONFIG` env var.
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
- EVM and Starknet chains are supported; Solana is planned. A bundled explorer
  for Starknet chains is still to come (Otterscan is EVM-only).

[Unreleased]: https://github.com/sainathr19/wharfnet/commits/main

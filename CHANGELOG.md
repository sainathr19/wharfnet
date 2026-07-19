# Changelog

All notable changes to **wharfnet** are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

wharfnet is early WIP — published as a pre-release, so the CLI and library
surface may still change.

## [Unreleased]

### Added

- **`wharfnet status --json`** — machine-readable status output for CI and
  scripts. Emits a stable JSON document instead of the formatted report: a
  top-level `running` flag so a script can tell whether a localnet is up, the
  `project` name, and a `chains` array carrying the exact manifest schema (RPC
  URLs, chain IDs, accounts, tokens). When nothing is running the output is still
  valid JSON (`running: false`, empty `chains`), so a pipeline can branch on it
  without special-casing. The default human-readable output is unchanged.

## [0.1.0-alpha.1] - 2026-07-18

### Added

- **`wharfnet::testkit` — a Rust test-utils API** — the crate is now a library as
  well as a CLI. Add `wharfnet` as a `dev-dependency` and, from an integration
  test, `Localnet::connect()` reads the manifest a running `wharfnet up` wrote and
  hands back typed accessors: `net.solana().rpc_url()`, `.ws_url()`,
  `.token("USDC")`, `.account(0)` (address + private key), `.explorer()`, etc. —
  so tests never hard-code RPC URLs or token addresses. Reuses the existing
  manifest model; the binary is now a thin shim over the library. The CLI moved
  into a `cli` module and the internal e2e harness was renamed to `harness`.
  The bundled test tokens' **contract ABIs** are embedded and exported too —
  `chain.token_abi("USDC")` (and the raw `wharfnet::abi::{evm,starknet}` JSON
  constants), generated from the Solidity sources (`solc`) and the compiled Cairo
  classes — so tests can instantiate a token without fetching or hand-writing an
  ABI. Solana tokens are standard SPL, so no custom interface is shipped.
- **Solana WebSocket RPC** — surfpool's WebSocket endpoint is now published, so
  subscriptions (`slotSubscribe`, `logsSubscribe`) and `confirmTransaction` work
  from the host (previously only the HTTP RPC on 8899 was mapped). It's published
  on the HTTP RPC port **+ 1** (`solana-1` on 8899 → WS on 8900), following
  Solana's own convention — clients like `@solana/web3.js` derive the WS URL from
  the RPC URL, so they connect automatically. Always served (not gated by
  `--bare`), advertised via a new `ws` field in the `status`/manifest, and folded
  into the cross-chain port collision check.
- **Solana block explorer** — every Solana chain now serves surfpool's built-in
  **Studio** UI, on by default (skipped by `up --bare`), matching the EVM
  (Otterscan) and Starknet (`--ui`) explorers. Unlike the Starknet UI — served at
  `/ui` on the RPC port — surfpool runs Studio as a separate in-container service,
  so wharfnet publishes it on the chain's RPC port **+ 10000** (`solana-1` on 8899
  → Studio on 18899) via a second port mapping, and records the URL in the unified
  `status`/manifest. The extra host port is folded into the cross-chain collision
  check so it can't silently clash with another chain's port.
- **Solana persistence** — `up --resume` / `up --reset` now cover Solana chains
  too. Each persists to its own `session-<chain>.sqlite` surfnet database via
  surfpool's `--db`/`--surfnet-id`, so balances, mints, and transactions survive
  `down` → `up --resume` and are wiped by `up --reset` — matching the EVM and
  Starknet chains. Because the SPL test tokens are seeded via cheatcodes rather
  than a baked file, a resumed chain detects they're already present and skips
  re-seeding, so it never clobbers your balances.
- **Solana forking** — the `fork_url` field now works on Solana chains, booting
  them as a **copy-on-read** fork of a live network via surfpool's `--rpc-url`, so
  you can test against real accounts and programs locally. `${VAR}` expansion and
  URL redaction are shared with the EVM/Starknet paths; a forked chain mirrors
  live state, so it seeds none of the baked SPL test tokens (the dev accounts are
  still airdropped over the fork). Unlike the EVM/Starknet forks, **`fork_block`
  is unsupported** — surfpool has no fork-at-slot flag — and is rejected on load.
- **Solana faucet** — the same `faucet <chain> <address> [amount] [--token]`
  command funds Solana addresses: native SOL through the standard `requestAirdrop`
  RPC, and the SPL test tokens through surfpool's `surfnet_setTokenAccount` cheat
  (the recipient needs no key). `--token SOL` funds only the native coin. Amounts
  are decimal (scaled by decimals) or exact base units with `--raw`, and funding
  is additive — SPL top-ups read the current balance first — matching the
  EVM/Starknet funders.
- **Solana test tokens** — every Solana chain now boots with standard SPL test
  tokens (USDC, WBTC) at fixed, deterministic mint addresses, seeded onto the dev
  accounts. Unlike the EVM/Starknet stacks — which load a baked state file —
  surfpool has no program to deploy for SPL, so wharfnet creates the mints
  (`surfnet_setAccount` with a hand-built SPL Mint) and funds the accounts
  (`surfnet_setTokenAccount`) via cheatcodes the moment the RPC is live, through a
  new post-boot engine hook. The mints are listed in the unified `status`/manifest.
  "Weird" Token-2022 test tokens (transfer-fee, interest-bearing) are a follow-up.
- **Solana chain control** — `wharfnet solana mine`, `increase-time`, `warp`,
  `pause-clock`, and `resume-clock` wrap surfpool's `surfnet_*` cheat JSON-RPC to
  drive a running chain, grouped under `solana` so each chain kind owns its own
  verbs. Differences from the EVM/Starknet controls, all from surfpool: `mine`
  advances slots (Solana's block cadence); `warp` is forward-only (surfpool can't
  rewind); there's no `impersonate`/`snapshot`/`revert`; and `pause-clock`/
  `resume-clock` freeze and restart surfpool's automatic slot production for
  step-by-step control.
- **Solana chains** — a `surfpool` chain (`solana-1`, `:8899`) now boots **by
  default** alongside the EVM and Starknet chains. surfpool runs an in-memory SVM
  ("surfnet") that boots in about a second and serves the standard Solana
  JSON-RPC, with three **deterministic funded dev accounts** (keypairs derived
  from documented seeds, funded with 10,000 SOL each at boot) recorded in the
  unified `status`/manifest with their base58 secrets. Readiness is gated on
  surfpool's `getHealth` RPC. The chain kind is selectable in `wharfnet.toml`
  with `kind = "solana"`. SPL test tokens, the faucet, persistence, forking, and
  chain control land in follow-ups.
- **Faster multi-chain boot** — `up` now health-checks every chain concurrently
  instead of one after another, so boot waits on the slowest chain rather than the
  sum of them.
- **Fractional & raw faucet amounts** — `faucet <chain> <address> <amount>` now
  takes a decimal `amount` (e.g. `1.5`), scaled by the token's decimals, instead
  of whole units only. Pass `--raw` to fund an exact base-unit integer (wei/fri,
  or a token's smallest unit). Shared parsing across the EVM and Starknet funders.
- **`logs` command** — `wharfnet logs [chain] [--follow]` streams container logs
  through `docker compose logs`. With no argument it shows every service; pass a
  chain kind (`evm`, `starknet`) or a specific name (`anvil-1`) to filter, and
  `--follow`/`-f` to keep tailing.
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
- **Starknet block explorer (on by default, `up --bare` to skip)** — Starknet
  chains now boot with starknet-devnet's built-in web UI explorer (`--ui`),
  served in-process at `/ui` on the chain's own RPC port and listed in the
  manifest and by `status`. Unlike the EVM chains' Otterscan (a separate
  container per chain, EVM-only), devnet serves its explorer itself, so there's
  no extra container or published port — every `up` is browsable for both stacks.
- **Starknet forking** — the `fork_url`/`fork_block` fields now work on Starknet
  chains too, booting them as a fork of a live network via starknet-devnet's
  `--fork-network`, so you can test against real Starknet contracts, classes, and
  balances locally. `${VAR}` expansion and URL redaction are shared with the EVM
  path; a forked chain mirrors live state, so it skips the baked Cairo test
  tokens. Reuses the same `wharfnet.toml` seam as the EVM `--fork-url`.
- **Starknet chain control** — `wharfnet starknet mine`, `increase-time`, `warp`,
  and `impersonate` wrap starknet-devnet's cheat JSON-RPC to drive a running
  chain, mirroring `wharfnet evm …` and grouped under `starknet` so each chain
  kind owns its own verbs. Two devnet-imposed differences: there's no
  `snapshot`/`revert` (devnet has no numbered-snapshot mechanism), and
  `impersonate` requires a forked chain (it's refused otherwise, with a hint).
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
  `compose`, `faucet`) that everything else builds on.

### Notes

- EVM, Starknet, and Solana chains are supported. The Solana stack currently
  covers boot, funded dev accounts, chain control, SPL test tokens, the faucet,
  forking, and persistence; weird Token-2022 tokens are the main piece left.

[Unreleased]: https://github.com/sainathr19/wharfnet/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/sainathr19/wharfnet/releases/tag/v0.1.0-alpha.1

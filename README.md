# wharfnet

[![ci](https://img.shields.io/github/actions/workflow/status/sainathr19/wharfnet/test.yml?branch=main&label=ci)](https://github.com/sainathr19/wharfnet/actions/workflows/test.yml)
[![docs](https://img.shields.io/badge/docs-sainathr19.github.io%2Fwharfnet-blue)](https://sainathr19.github.io/wharfnet/)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

**One-command localnet for EVM, Solana, Starknet, Bitcoin & Litecoin — built-in faucet, pre-deployed test tokens and more.**

> ⚠️ Early WIP. Five chains — EVM (Anvil), Starknet (starknet-devnet), Solana
> (surfpool), and Bitcoin + Litecoin (regtest) — boot by default with funded
> accounts, a faucet, chain control, and persistence, plus test tokens, forking,
> and a built-in block explorer where the chain supports them. The CLI surface
> may still change. See the [CHANGELOG](./CHANGELOG.md).

`wharfnet` is the local harbor for your chains: boot EVM, Solana, Starknet, and
Bitcoin/Litecoin networks locally with a single command, fund accounts from a
unified faucet, and get standard test tokens deployed at known addresses — then
wire it straight into your integration tests and CI pipelines.

## Goals

Cross-chain and multi-VM teams stitch together Anvil, a Solana test validator,
and a Starknet devnet by hand — plus a homemade faucet and glue scripts — every
time they need a local environment. `wharfnet` packages that stitching into one
opinionated, reproducible tool:

- **One command, many chains.** `wharfnet up` boots EVM, Starknet, Solana, and
  Bitcoin/Litecoin chains together, behind one config, one manifest, and one
  `status`.
- **Batteries included.** Funded dev accounts, test tokens at fixed addresses, a
  unified faucet, chain-control cheats, mainnet forking, and a block explorer per
  chain — no glue scripts.
- **Reproducible by default.** Every boot is deterministic (same accounts, same
  token addresses), so tests and CI get an identical environment every run, with
  opt-in persistence when you want to pick up where you left off.
- **Uniform surface across VMs.** The same verbs (`up`, `faucet`, per-chain
  control) work the same way whether the chain is EVM, Starknet, Solana, Bitcoin,
  or Litecoin.
- **A test-utils library, not just a CLI.** Import `wharfnet::testkit` to read a
  running localnet's endpoints, funded accounts, and token addresses (plus
  embedded ABIs) straight into your Rust tests — no hard-coding.

## Documentation

Full docs — install, configuration, and per-chain guides (tokens, faucet, chain
control, forking, explorer, persistence) — live at
**[sainathr19.github.io/wharfnet](https://sainathr19.github.io/wharfnet/)**:

All the chains are grouped under **[Chains](https://sainathr19.github.io/wharfnet/chains/overview)**, one guide each:

| Chain                          | Default node        | Guide                                                                    |
| ------------------------------ | ------------------- | ------------------------------------------------------------------------ |
| **EVM** (`anvil-1`, `anvil-2`) | Anvil               | [/chains/evm](https://sainathr19.github.io/wharfnet/chains/evm)           |
| **Starknet** (`starknet-1`)    | starknet-devnet     | [/chains/starknet](https://sainathr19.github.io/wharfnet/chains/starknet) |
| **Solana** (`solana-1`)        | surfpool            | [/chains/solana](https://sainathr19.github.io/wharfnet/chains/solana)     |
| **Bitcoin** (`bitcoin-1`)      | bitcoind (regtest)  | [/chains/bitcoin](https://sainathr19.github.io/wharfnet/chains/bitcoin)   |
| **Litecoin** (`litecoin-1`)    | litecoind (regtest) | [/chains/litecoin](https://sainathr19.github.io/wharfnet/chains/litecoin) |

The site is a [Nextra](https://nextra.site) app under [`landing/`](landing/),
deployed to GitHub Pages on every push to `main` (source pages live in
[`landing/content/`](landing/content/)).

Runnable task recipes — fund + transfer, snapshot/revert, fork & impersonate,
Solana airdrops, and a CI workflow — live in [`examples/`](examples/).

## Prerequisites

wharfnet runs each chain as a container, so it needs **Docker with the Compose
plugin** (`docker compose`) and a running daemon. Every command that boots or
drives a chain — `up`, `down`, `faucet`, and `wharfnet evm …` — shells out to
`docker compose`, so CI runners need a Docker daemon available too.

You do **not** need Foundry, a Solana toolchain, or a Starknet devnet installed:
each chain runs from a pinned image, and per-chain tooling (e.g. `cast`) runs
inside the container, so installing Docker is the whole setup. Building from
source also needs a stable **Rust** toolchain.

Without Docker, chain commands fail fast with a clear message; only
`wharfnet compose` (render the Compose file) and `wharfnet status` (read the
manifest) run without it.

## Quickstart

```sh
# build
cargo build --release

# boot the local multi-chain network — chains + a block explorer each
wharfnet up

# boot just the chains, without the explorers
wharfnet up --bare

# resume where you left off — restores balances, txs & deployments
wharfnet up --resume

# discard a saved session and boot clean
wharfnet up --reset

# check what's running
wharfnet status

# machine-readable status for CI and scripts (pipe to jq, etc.)
wharfnet status --json

# fund an address with native coin + every bundled token, on all EVM chains
wharfnet faucet evm 0xabc... 100

# same command funds Starknet and Solana
wharfnet faucet starknet 0x05a1... 100
wharfnet faucet solana 9WzD…AWWM 100

# stream container logs (all, or one chain/kind; -f to follow)
wharfnet logs starknet-1 -f

# shut everything down
wharfnet down
```

Faucet, chain-control, forking, and explorer details live in the per-chain
guides linked [above](#documentation).

## Use it in tests (Rust)

wharfnet is also a **library**. Add it as a `dev-dependency` and connect to a
running localnet from an integration test — no hard-coded URLs or token
addresses, all read from the manifest `wharfnet up` writes:

```toml
# Cargo.toml
[dev-dependencies]
wharfnet = "0.1.0-alpha.1"   # pre-release: pin the exact version
```

```rust
use wharfnet::testkit::Localnet;

let net = Localnet::connect()?;      // reads .wharfnet/wharfnet.json
let sol = net.solana();              // also .evm() / .starknet() / .chain("anvil-2")
let rpc = sol.rpc_url();             // + .ws_url(), .chain_id(), .explorer()
let usdc = sol.token("USDC");        // { address, decimals, .. }
let dev0 = sol.account(0);           // funded signer: address + private_key
let abi = sol.token_abi("USDC");     // embedded contract ABI (EVM/Starknet)
```

The bundled test tokens' contract ABIs are embedded too (`wharfnet::abi`), so you
can instantiate a token without fetching or hand-writing one. See the
[Examples guide](https://sainathr19.github.io/wharfnet/examples) for viem /
web3.js / starknet.js snippets alongside the Rust API.

## Configuration

wharfnet runs zero-config — two Anvil chains, a Starknet chain, a Solana chain,
and Bitcoin + Litecoin regtest chains by default (`anvil-1` :8545, `anvil-2`
:8546, `starknet-1` :5050, `solana-1` :8899, `bitcoin-1` :18443, `litecoin-1`
:19443). To customise the chain topology — including dropping a chain — write your
own `wharfnet.toml` in your project root (a config replaces the defaults
entirely):

```toml
# wharfnet.toml — omit entirely for the built-in defaults
[[chains]]
name = "anvil-1"
port = 8545
chain_id = 31337
block_time = 1      # optional, defaults to 1

[[chains]]
name = "sn-1"
kind = "starknet"   # boots a starknet-devnet chain
port = 5050         # RPC is published at http://127.0.0.1:5050/rpc

[[chains]]
name = "sol-1"
kind = "solana"     # boots a surfpool chain
port = 8899

[[chains]]
name = "bitcoin-1"
kind = "bitcoin"    # boots a bitcoind regtest chain
port = 18443

[[chains]]
name = "litecoin-1"
kind = "litecoin"   # boots a litecoind regtest chain
port = 19443
```

Each chain needs a unique `name` and `port`; `kind` defaults to `evm` and may be
`starknet`, `solana`, `bitcoin`, or `litecoin`. EVM chains also need a numeric
`chain_id`; the others omit it. Accounts and test tokens come from the baked
presets and aren't configured here. Run `wharfnet compose` to see the resolved
setup — and to catch config errors — without booting anything.

Bitcoin and Litecoin run their Core daemons in **regtest** with fixed dev RPC
credentials (`wharfnet:wharfnet`), and at boot each mines a spendable balance into
a `wharfnet` wallet. Fund any address with `wharfnet faucet bitcoin-1 <addr>
<amount>` (native coin only — UTXO chains carry no test tokens), and mine blocks
on demand with `wharfnet bitcoin mine <n>` / `wharfnet litecoin mine <n>`. They
don't fork (`fork_url` is rejected) and run fresh every boot (`--resume`/`--reset`
don't apply).

By default wharfnet reads `./wharfnet.toml`. Point at a different file with
`--config <path>` (or `-c`) on `up`/`compose`, or the `WHARFNET_CONFIG` env var:

```sh
wharfnet up --config fork.toml
WHARFNET_CONFIG=ci.toml wharfnet up
```

Any chain can fork a live network by adding `fork_url` (and, for EVM/Starknet,
`fork_block`) — see the per-chain guides for the specifics.

## State & persistence

By default `wharfnet up` boots a **fresh, deterministic** network every time:
the pre-deployed tokens and seeded accounts are always exactly the same, and
anything you do at runtime (faucet top-ups, transactions, contract deploys) is
discarded on `down`. That's the right default for reproducible tests and CI.

When you'd rather pick up where you left off:

| Command | Behaviour |
| ------- | --------- |
| `wharfnet up` | Fresh boot from the baked snapshot. Runtime changes are not saved. |
| `wharfnet up --resume` | Restore the previous session if one exists (else fresh), and **keep saving** — balances, txs, and deployments survive `down` → `up --resume`. |
| `wharfnet up --reset` | Discard any saved session, then boot fresh. |

Under the hood each chain persists to a per-chain session under
`.wharfnet/state/` that it reloads on the next `--resume`; the exact mechanism
per chain kind is documented in the per-chain guides. `--resume` and `--reset`
are mutually exclusive.

## Contributing

Contributions are welcome — issues and PRs alike.

- **Workflow.** Work lands on `main` through a PR (see the branch rules in
  [RELEASING.md](./RELEASING.md)); `main` is protected. Keep changes focused and
  update the [CHANGELOG](./CHANGELOG.md) `## [Unreleased]` section with anything
  user-facing.
- **Before you push.** CI runs formatting, Clippy, tests, and a dependency audit
  (see `.github/workflows/`). Run them locally first:

  ```sh
  cargo fmt --all
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```

  Some tests boot real containers and need a running Docker daemon.
- **Adding a chain kind or capability.** Chains implement the `Engine` trait in
  `src/runtime/engine.rs`; the per-chain code lives under `src/<kind>/`. The
  per-chain [docs](#documentation) describe how each stack wires up tokens,
  faucet, forking, and its explorer — a good map before you extend one.

Releases are published to crates.io from a version tag — see
[RELEASING.md](./RELEASING.md).

## License

[MIT](./LICENSE)

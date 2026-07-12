# wharfnet

**One-command localnet for EVM, Solana & Starknet — built-in faucet, pre-deployed test tokens and more.**

> ⚠️ Early WIP. The EVM stack — chains, test tokens, faucet, explorer, and
> persistence — works today. A Starknet chain now **boots by default** alongside
> the EVM ones (predeployed accounts, ETH/STRK fee tokens, baked Cairo test
> tokens), the **faucet funds it**, its state **persists across `up --resume`**,
> and it ships with a **built-in block explorer** too. A Solana chain
> (**surfpool**) now **boots by default** as well, with deterministic funded dev
> accounts; its faucet, SPL test tokens, persistence, and forking are landing next.

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

## Status & roadmap

Early WIP, but the **EVM stack works end to end today**. See the
[CHANGELOG](./CHANGELOG.md) for details.

**Working now**

- [x] Two EVM chains (Anvil) — `anvil-1` (:8545), `anvil-2` (:8546)
- [x] Unified faucet — native coin + every token, or a single token via `--token`
- [x] Pre-deployed ERC-20 test tokens (USDC, WBTC + weird tokens) at fixed addresses, public `mint`
- [x] Canonical contracts pre-deployed (Multicall3, Permit2, CREATE2 deployer)
- [x] Block explorer (Otterscan) per EVM chain, on by default
- [x] Persistent state — `up --resume` / `up --reset`
- [x] Optional `wharfnet.toml` to customise the chain topology
- [x] Mainnet forking — `fork_url`/`fork_block` per chain (Anvil `--fork-url`)
- [x] EVM chain control — `wharfnet evm mine | warp | impersonate | snapshot | revert`
- [x] Endpoints manifest — `.wharfnet/wharfnet.json`
- [x] Boot waits for readiness; `down` tears it all down (CI-friendly)
- [x] Starknet chain (`starknet-devnet`) — boots alongside EVM chains with
      predeployed accounts, ETH/STRK fee tokens, and baked Cairo test tokens
      (USDC, WBTC + weird tokens) at fixed addresses, in the unified
      `status`/manifest
- [x] Starknet faucet — same `faucet` command funds ETH/STRK (devnet mint cheat)
      and mints the Cairo test tokens via signed invokes
- [x] Starknet persistence — `up --resume` / `up --reset` keep (or discard) a
      Starknet chain's state across restarts, like the EVM chains
- [x] Starknet block explorer — starknet-devnet's built-in web UI (`--ui`),
      on by default, served in-process at `/ui`
- [x] Starknet forking — `fork_url`/`fork_block` per chain (devnet
      `--fork-network`), mirroring a live Starknet network locally
- [x] Starknet chain control — `wharfnet starknet mine | increase-time | warp |
      impersonate` over devnet's cheat JSON-RPC
- [x] Solana chain (`surfpool`) — boots alongside the EVM and Starknet chains
      with deterministic, funded dev accounts, in the unified `status`/manifest

**Planned**

- [ ] Solana test tokens (SPL) at fixed addresses
- [ ] Solana faucet — same `faucet` command funds SOL + SPL tokens
- [ ] Solana persistence — `up --resume` / `up --reset`
- [ ] Solana forking — `fork_url` (surfpool `--rpc-url`)
- [ ] Solana chain control — time-travel, clock pause/resume

Releases are published to crates.io from a version tag — see
[RELEASING.md](./RELEASING.md).

## Prerequisites

wharfnet runs each chain as a container, so it needs **Docker with the Compose
plugin** (`docker compose`) and a running daemon. Every command that boots or
drives a chain — `up`, `down`, `faucet`, and `wharfnet evm …` — shells out to
`docker compose`, so CI runners need a Docker daemon available too.

You do **not** need Foundry, a Solana toolchain, or a Starknet devnet installed:
each chain runs from a pinned image (Anvil and `starknet-devnet` today) — for EVM
chains `cast` runs inside the container, so installing Docker is the whole setup.
Building from source also needs a stable **Rust** toolchain (see
[Quickstart](#quickstart)).

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

# fund an address with native coin + every bundled token, on all EVM chains
wharfnet faucet evm 0xabc... 100

# fund just one token, on a specific chain
wharfnet faucet anvil-1 0xabc... 100 --token USDC

# same command funds Starknet: ETH/STRK + every Cairo test token
wharfnet faucet starknet 0x05a1... 100
wharfnet faucet starknet-1 0x05a1... 50 --token WBTC

# stream container logs (all, or one chain/kind; -f to follow)
wharfnet logs starknet-1 -f

# shut everything down
wharfnet down
```

## Configuration

wharfnet runs zero-config — two Anvil chains and a Starknet chain by default
(`anvil-1` :8545, `anvil-2` :8546, `starknet-1` :5050). To customise the chain
topology — including dropping the Starknet chain — write your own `wharfnet.toml`
in your project root (a config replaces the defaults entirely):

```toml
# wharfnet.toml — omit entirely for the built-in defaults
[[chains]]
name = "anvil-1"
port = 8545
chain_id = 31337
block_time = 1      # optional, defaults to 1

[[chains]]
name = "l2"
port = 8546
chain_id = 42161

[[chains]]
name = "sn-1"
kind = "starknet"   # boots a starknet-devnet chain
port = 5050         # RPC is published at http://127.0.0.1:5050/rpc
```

Each chain needs a unique `name` and `port`; `kind` defaults to `evm` and may be
`starknet`. EVM chains also need a numeric `chain_id`; Starknet chains omit it —
they use devnet's default (`SN_SEPOLIA`), which isn't configurable yet. Accounts
and test tokens come from the baked presets and aren't configured here. Run
`wharfnet compose` to see the resolved setup — and to catch config errors —
without booting anything.

By default wharfnet reads `./wharfnet.toml`. Point at a different file with
`--config <path>` (or `-c`) on `up`/`compose`, or the `WHARFNET_CONFIG` env var —
handy for keeping several topologies (e.g. `local.toml`, `fork.toml`). An
explicitly named file that doesn't exist is an error; a missing default just
falls back to the built-ins.

```sh
wharfnet up --config fork.toml
WHARFNET_CONFIG=ci.toml wharfnet up
```

## Mainnet forking

Point a chain at a live RPC and it boots as a **fork** of that network — real
balances, contracts, and storage, mutable locally. Add `fork_url` (and optionally
`fork_block` to pin a height) to a chain in `wharfnet.toml`:

```toml
[[chains]]
name = "mainnet"
port = 8545
chain_id = 1
fork_url = "${MAINNET_RPC}"   # ${VAR} is expanded from the environment
fork_block = 21000000         # optional; omit to track the latest block
```

`${VAR}` references are resolved from the environment on load, so an RPC key
never has to live in the file — and the manifest and `status` only ever record a
**redacted** `scheme://host`, never the key. Pinning `fork_block` to a past block
needs an **archive** RPC; forking at the latest block works with an ordinary
full-node endpoint.

A forked chain mirrors live state, so it does **not** load the baked test tokens
or canonical contracts — it already has whatever the source network has. Combine
forking with [chain control](#evm-chain-control): `wharfnet evm impersonate` lets
you send transactions as any address (a whale, a protocol admin) with no key.

```sh
MAINNET_RPC=https://… wharfnet up --config fork.toml --bare
cast call 0xA0b8…eB48 'symbol()(string)' --rpc-url http://127.0.0.1:8545   # -> "USDC"
```

**Starknet chains fork the same way.** Set `fork_url` (and optionally
`fork_block`) on a `kind = "starknet"` chain and it boots as a fork via
starknet-devnet's `--fork-network`, mirroring the origin's contracts and
balances. The same `${VAR}` expansion and redaction apply:

```toml
[[chains]]
name = "sn-fork"
kind = "starknet"
port = 5050
fork_url = "${STARKNET_RPC}"   # a Starknet JSON-RPC endpoint (e.g. Sepolia)
fork_block = 900000            # optional; omit to track the latest block
```

The predeployed dev accounts still apply (devnet funds them over the fork), so
you can send transactions against real forked state right away.

## Test tokens

Every EVM chain boots with test tokens pre-deployed at fixed addresses
(identical on all chains) from a baked-in Anvil state snapshot — no deploy step
required. Each has a **public `mint(address,uint256)`** so a faucet (or your
tests) can top up any address on demand. The first two are standard, well-behaved
ERC-20s; the rest are deliberately **"weird"** for token-integration testing:

| Token | Decimals | Address | Behaviour |
| ----- | -------- | ------- | --------- |
| USDC  | 6  | `0x5FbDB2315678afecb367f032d93F642f64180aa3` | standard |
| WBTC  | 8  | `0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512` | standard |
| FEE   | 18 | `0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0` | fee-on-transfer (1% burned on `transfer`) |
| REB   | 18 | `0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9` | rebasing (`rebase(uint256)` rescales balances) |
| NRT   | 6  | `0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9` | no return value (USDT-style `transfer`/`approve`) |

The weird tokens let you check that your contracts and integrations handle
real-world token quirks — amount-received ≠ amount-sent, balances that move with
no transfer, and calls that don't return a decodable `bool`.

The dev accounts start pre-seeded with a balance of each. Regenerate the
snapshot after editing the token sources with `./scripts/gen-token-state.sh`.

## Canonical contracts

Every EVM chain also boots with the infrastructure contracts that live at the
**same address on every real chain**, so client libraries and deploy tooling
that hardcode these addresses work out of the box — no per-chain wiring:

| Contract         | Address                                      | Used by |
| ---------------- | -------------------------------------------- | ------- |
| Multicall3       | `0xcA11bde05977b3631167028862bE2a173976CA11` | viem / ethers / wagmi batch reads |
| Permit2          | `0x000000000022D473030F116dDEE9F6B43aC78BA3` | Uniswap & signature-based approvals |
| CREATE2 Deployer | `0x4e59b44847b379578588920cA78FbF26c0B4956C` | `forge create --create2`, deterministic deploys |

Multicall3 and Permit2 are etched from their real mainnet bytecode (see
`src/resources/presets/`); the CREATE2 deployer is deployed by Anvil itself.

## Block explorer

`wharfnet up` boots an [Otterscan](https://github.com/otterscan/otterscan)
instance for each EVM chain by default — a lightweight, open-source block
explorer. Pass `--bare` to skip them and run only the chains:

```sh
wharfnet up          # chains + explorers
wharfnet up --bare   # chains only
```

Anvil implements Otterscan's RPC API (`ots_*`), so the explorer needs no indexer
or database — it's a static frontend talking straight to the chain. Each EVM
chain gets its own Otterscan on a dedicated port. **Starknet** chains use
starknet-devnet's own built-in web UI instead (Otterscan is EVM-only): it's
served in-process at `/ui` on the chain's own RPC port, so there's no extra
container or port. Every explorer URL is recorded in the manifest and printed by
`status`:

| Chain      | RPC                         | Explorer                    |
| ---------- | --------------------------- | --------------------------- |
| anvil-1    | `http://127.0.0.1:8545`     | `http://127.0.0.1:5100`     |
| anvil-2    | `http://127.0.0.1:8546`     | `http://127.0.0.1:5101`     |
| starknet-1 | `http://127.0.0.1:5050/rpc` | `http://127.0.0.1:5050/ui`  |

`--bare` skips both the Otterscan containers and devnet's `--ui`.

## EVM chain control

Drive a running localnet with thin wrappers over Anvil's cheat RPCs, grouped
under `wharfnet evm`. Each takes a `--chain` selector (`evm` for every EVM chain,
or a name like `anvil-1`; defaults to `evm`):

```sh
wharfnet evm mine 10                 # mine 10 blocks
wharfnet evm increase-time 86400     # fast-forward time by a day
wharfnet evm warp 1893456000         # set the next block to an absolute Unix time
wharfnet evm impersonate 0xd8dA…6045 # then: cast send … --from 0xd8dA…6045 --unlocked
wharfnet evm impersonate 0xd8dA…6045 --stop
wharfnet evm snapshot                # prints an id, e.g. 0x1
wharfnet evm revert 0x1              # roll state back to that snapshot
```

`impersonate` lets you send transactions as **any** address with no private key
(great with forked state), and `snapshot`/`revert` give tests a cheap reset
point. These live under `evm` because they're Anvil-specific — other chain kinds
get their own namespaces (`wharfnet starknet …`, `wharfnet solana …`).

## Starknet chain control

The Starknet equivalents live under `wharfnet starknet`, wrapping starknet-devnet's
cheat JSON-RPC. Each takes a `--chain` selector (`starknet` for every Starknet
chain, or a name like `starknet-1`; defaults to `starknet`):

```sh
wharfnet starknet mine 10                # create 10 blocks
wharfnet starknet increase-time 86400    # fast-forward time by a day
wharfnet starknet warp 1893456000        # set the chain to an absolute Unix time
wharfnet starknet impersonate 0x0123…    # forked chains only (see below)
wharfnet starknet impersonate 0x0123… --stop
```

Two differences from the EVM verbs, both from starknet-devnet: there's **no
`snapshot`/`revert`** (devnet has no numbered-snapshot mechanism, only block
abort), and **`impersonate` works only on a forked chain** — devnet impersonates
accounts that exist on the forked origin, so on a plain local chain the command
is refused with a hint to set `fork_url` first.

## Starknet chains

`wharfnet up` boots a
[`starknet-devnet`](https://github.com/0xSpaceShard/starknet-devnet) chain by
default (`starknet-1` on :5050), right next to the two EVM chains — one command,
one manifest, one `status`. To run without it, write a `wharfnet.toml` that omits
the Starknet chain (a config replaces the defaults). Poke it directly:

```sh
wharfnet up --bare
curl http://127.0.0.1:5050/is_alive                                   # -> Alive!!!
curl -s -X POST http://127.0.0.1:5050/rpc \
  -d '{"jsonrpc":"2.0","id":1,"method":"starknet_chainId","params":[]}'  # -> SN_SEPOLIA
```

Each Starknet chain comes with **deterministic predeployed accounts** (fixed
`--seed`, so they're identical every boot) and the standard **ETH and STRK fee
tokens** at their canonical addresses — all recorded in the manifest. The RPC is
served at `http://127.0.0.1:<port>/rpc`, and readiness is checked against
devnet's `/is_alive` endpoint.

### Starknet test tokens

Every Starknet chain also boots with a set of **Cairo test tokens** pre-deployed
at fixed addresses (identical on every chain), each with a **public
`mint(recipient, amount)`** and seeded onto the dev accounts. As on the EVM side,
the first two are standard and the rest are deliberately **"weird"** for
token-integration testing:

| Token | Decimals | Address | Behaviour |
| ----- | -------- | ------- | --------- |
| USDC  | 6  | `0x040b582f9ba878be8e78a6ddc665dfdfd55a4deae9ceeb40115abcfa1f8df686` | standard |
| WBTC  | 8  | `0x029a79ea0c5716d63250a0bbf2462509f3c0eed9d29a2e1c02c63fa7b2b1db66` | standard |
| FEE   | 18 | `0x07edc0e8738c7804ad087344c1c54d817f739dd4179f1dd4e11ea5badada47aa` | fee-on-transfer (1% burned on `transfer`) |
| REB   | 18 | `0x06e94ed66ea18ea06a9bed118a8d6ebc3cc19d31ed025bd5abadd3477d300500` | rebasing (`rebase(factor)` rescales balances) |

Sources live in `src/resources/contracts/starknet/` (self-contained Cairo, no
OpenZeppelin dependency). The EVM stack's USDT-style no-return token has no
analogue — Cairo's ERC-20 ABI returns `bool` by the standard. Under the hood the
tokens are baked into a devnet **replay log** that `wharfnet up` re-executes on
boot; regenerate it after editing the sources with
`./scripts/gen-starknet-token-state.sh` (needs `scarb` + `cargo` — the
declare/deploy step runs through `examples/gen_starknet_tokens.rs`, using the same
JSON-RPC-0.10 [`starknet-rust`] client the faucet does).

[`starknet-rust`]: https://github.com/software-mansion/starknet-rust

### Funding a Starknet address

The unified `faucet` command works on Starknet chains too:

```bash
# ETH + STRK + every Cairo test token, on every Starknet chain
wharfnet faucet starknet 0x05a1... 100

# just one token, on a specific chain
wharfnet faucet starknet-1 0x05a1... 50 --token WBTC
```

ETH and STRK are minted through devnet's mint cheat; the Cairo test tokens are
minted by a **signed invoke** of their public `mint`, submitted through the first
predeployed dev account (it only pays gas — the recipient needs no key). Amounts
are whole units, scaled by each token's decimals. Funding is additive, so repeat
top-ups accumulate.

Starknet chains persist across `up --resume`/`--reset` just like the EVM ones —
see [State & persistence](#state--persistence) below. They're browsable too:
each boots with starknet-devnet's built-in web UI explorer at `/ui` on its RPC
port (see [Block explorer](#block-explorer)).

## Solana chains

`wharfnet up` also boots a [`surfpool`](https://github.com/solana-foundation/surfpool)
Solana chain by default (`solana-1` on :8899), alongside the EVM and Starknet
chains. surfpool runs an in-memory SVM ("surfnet") that boots in about a second
and serves the standard Solana JSON-RPC, so the usual tooling (`solana`, `anchor`)
points straight at it. Poke it directly:

```sh
wharfnet up --bare
# standard Solana JSON-RPC on :8899
curl -s -X POST http://127.0.0.1:8899 \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":[]}'   # -> {"result":"ok"}
```

Each Solana chain comes with **deterministic funded dev accounts** — three
keypairs derived from documented seeds (`sha256("wharfnet-solana-dev-<i>")` →
ed25519), so they're identical on every boot and regenerable by anyone (the
Solana analogue of Anvil's fixed test mnemonic). They're well-known throwaway
keys, funded with 10,000 SOL each at boot and recorded in the manifest with their
base58 secrets, so tooling can sign as them. Readiness is checked against
surfpool's `getHealth` RPC.

SPL test tokens, the faucet, persistence, forking, and chain control are landing
next — this first cut boots the chain and funds the dev accounts.

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

Under the hood each chain continuously dumps its state to a per-chain snapshot
(`.wharfnet/state/session-<chain>.json`) that it reloads on the next `--resume`:
EVM chains use Anvil's `--state`, and Starknet chains dump the devnet replay log
on every block (one per transaction). `--resume` and `--reset` are mutually
exclusive.

## License

[MIT](./LICENSE)

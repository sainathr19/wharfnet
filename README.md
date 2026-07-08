# wharfnet

**One-command localnet for EVM, Solana & Starknet — built-in faucet, pre-deployed test tokens and more.**

> ⚠️ Early WIP. The EVM stack — chains, test tokens, faucet, explorer, and
> persistence — works today; Solana and Starknet are next.

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
- [x] Pre-deployed ERC-20 test tokens (USDC, WBTC) at fixed addresses, public `mint`
- [x] Block explorer (Otterscan) per EVM chain, on by default
- [x] Persistent state — `up --resume` / `up --reset`
- [x] Endpoints manifest — `.wharfnet/wharfnet.json`
- [x] Boot waits for readiness; `down` tears it all down (CI-friendly)

**Planned**

- [ ] Solana chain — validator, faucet, SPL tokens
- [ ] Starknet chain — devnet, faucet, Cairo tokens
- [ ] `deploy` command — deploy bundled/custom contracts on demand
- [ ] CI polish — machine-readable `status --json`, non-interactive mode

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

# deploy bundled/custom contracts (planned — not yet implemented)
wharfnet deploy

# shut everything down
wharfnet down
```

## Test tokens

Every EVM chain boots with standard test tokens pre-deployed at fixed addresses
(identical on all chains) from a baked-in Anvil state snapshot — no deploy step
required. Each has a **public `mint(address,uint256)`** so a faucet (or your
tests) can top up any address on demand:

| Token | Decimals | Address |
| ----- | -------- | ------- |
| USDC  | 6        | `0x5FbDB2315678afecb367f032d93F642f64180aa3` |
| WBTC  | 8        | `0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512` |

The dev accounts start pre-seeded with a balance of each. Regenerate the
snapshot after editing the token sources with `./scripts/gen-token-state.sh`.

## Block explorer

`wharfnet up` boots an [Otterscan](https://github.com/otterscan/otterscan)
instance for each EVM chain by default — a lightweight, open-source block
explorer. Pass `--bare` to skip them and run only the chains:

```sh
wharfnet up          # chains + explorers
wharfnet up --bare   # chains only
```

Anvil implements Otterscan's RPC API (`ots_*`), so the explorer needs no indexer
or database — it's a static frontend talking straight to the chain. Each chain
gets its own explorer on a dedicated port, and the URL is recorded in the
manifest and printed by `status`:

| Chain   | RPC                     | Explorer                |
| ------- | ----------------------- | ----------------------- |
| anvil-1 | `http://127.0.0.1:8545` | `http://127.0.0.1:5100` |
| anvil-2 | `http://127.0.0.1:8546` | `http://127.0.0.1:5101` |

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

Under the hood each chain dumps its state to a per-chain snapshot
(`.wharfnet/state/session-<chain>.json`) via Anvil's `--state`, flushed on exit
and periodically while running. `--resume` and `--reset` are mutually exclusive.

## License

[MIT](./LICENSE)

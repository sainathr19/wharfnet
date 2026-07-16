# Solana chains

`wharfnet up` boots a [`surfpool`](https://github.com/solana-foundation/surfpool)
Solana chain by default (`solana-1` on `:8899`), alongside the EVM and Starknet
chains. surfpool runs an in-memory SVM ("surfnet") that boots in about a second
and serves the standard Solana JSON-RPC, so the usual tooling (`solana`, `anchor`)
points straight at it. This page covers everything Solana-specific; see the
[root README](../README.md) for install, quickstart, and configuration basics.

```sh
wharfnet up --bare
# standard Solana JSON-RPC on :8899, WebSocket RPC on :8900
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

## WebSocket RPC

The **WebSocket RPC** is published on the HTTP RPC port + 1 (`solana-1` →
`ws://127.0.0.1:8900`), following Solana's own convention, so clients like
`@solana/web3.js` that derive the WS URL from the RPC URL just work —
subscriptions (`slotSubscribe`, `logsSubscribe`) and `confirmTransaction` all run
against the host. It's always served (not gated by `--bare`) and recorded in the
manifest.

## Test tokens

Every Solana chain also boots with standard **SPL test tokens** at fixed mint
addresses (identical on every chain), each seeded onto the dev accounts:

| Token | Decimals | Mint address                                  |
| ----- | -------- | --------------------------------------------- |
| USDC  | 6        | `94C6wFGeVr5SahK9owBMBhpFPRtvLuZhQQVRh7NYrEp9` |
| WBTC  | 8        | `Fp7Dnb8KKkWWw5RfUPsQBNRrooj75gbNaWoC28AnCn3E` |

Unlike the EVM/Starknet stacks — which bake a state file the node loads at boot —
surfpool needs no program to deploy (the SPL Token program is native), so wharfnet
seeds these at runtime the moment the chain is ready: it creates each mint with
surfpool's `surfnet_setAccount` cheat and funds the dev accounts with
`surfnet_setTokenAccount`. The mint addresses are deterministic (ed25519 of
`sha256("wharfnet-solana-mint-<symbol>")`) and their mint authority is dev account 0.

"Weird" Token-2022 test tokens (transfer-fee, interest-bearing) are planned but
not yet available — surfpool's cheatcodes don't currently create queryable
Token-2022 accounts.

## Faucet

The unified `faucet` command works on Solana chains too:

```sh
# SOL + every SPL token, on every Solana chain
wharfnet faucet solana 9WzD…AWWM 100

# just one token, on a specific chain
wharfnet faucet solana-1 9WzD…AWWM 50 --token WBTC

# just native SOL
wharfnet faucet solana-1 9WzD…AWWM 5 --token SOL
```

SOL is credited with the standard `requestAirdrop` RPC; the SPL tokens are topped
up with surfpool's `surfnet_setTokenAccount` cheat (the recipient needs no key).
Amounts are decimal (scaled by each token's decimals) or exact base units with
`--raw`. Funding is additive, so repeat top-ups accumulate.

## Chain control

The Solana chain-control verbs live under `wharfnet solana`, wrapping surfpool's
`surfnet_*` cheat JSON-RPC. Each takes a `--chain` selector (`solana` for every
Solana chain, or a name like `solana-1`; defaults to `solana`):

```sh
wharfnet solana mine 10                # advance the chain by 10 slots
wharfnet solana increase-time 86400    # fast-forward time by a day
wharfnet solana warp 1893456000        # set the clock to an absolute Unix time
wharfnet solana pause-clock            # freeze automatic slot production
wharfnet solana resume-clock           # resume it
```

Differences from the EVM/Starknet verbs, all from surfpool's design: `mine`
advances **slots** (Solana's block cadence) rather than mining discrete blocks;
`warp` is **forward-only** (surfpool can't rewind, so a past target is refused);
there's **no `impersonate` or `snapshot`/`revert`** (set account state directly
via cheatcodes instead); and `pause-clock`/`resume-clock` are surfpool extras —
surfpool auto-produces slots on a timer, so pausing gives you deterministic,
step-by-step control (`mine` while paused advances exactly N slots).

## Forking

Set `fork_url` on a `kind = "solana"` chain and surfpool boots as a
**copy-on-read** fork of that network, fetching accounts from the source RPC on
first access. The predeployed dev accounts are still airdropped over the fork, so
you have funded signers immediately:

```toml
[[chains]]
name = "sol-fork"
kind = "solana"
port = 8899
fork_url = "${SOLANA_RPC}"   # a Solana JSON-RPC endpoint (e.g. mainnet-beta)
```

One difference from the EVM/Starknet forks: **`fork_block` is not supported for
Solana** — surfpool has no fork-at-slot flag, so a Solana fork always tracks the
datasource's current slot. Setting `fork_block` on a `kind = "solana"` chain is
rejected on load.

## Block explorer

Each Solana chain serves surfpool's built-in **Studio** explorer. Unlike the
Starknet UI — served at `/ui` on the RPC port — surfpool runs Studio as a
separate in-container service, so wharfnet publishes it on the chain's RPC port
**+ 10000** and records the URL in the manifest:

| Chain    | RPC                     | Explorer                 |
| -------- | ----------------------- | ------------------------ |
| solana-1 | `http://127.0.0.1:8899` | `http://127.0.0.1:18899` |

Pass `--bare` to skip it.

## Persistence

Solana chains persist across `up --resume` / `up --reset` like the EVM/Starknet
chains: surfpool writes a surfnet SQLite db (`session-<chain>.sqlite`) via `--db`
under `.wharfnet/state/`, reloaded on the next `--resume`. See
[State & persistence](../README.md#state--persistence) in the root README for the
shared model.

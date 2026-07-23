# src/resources/

Static assets embedded into the `wharfnet` binary at compile time (via
`include_str!`). Co-located with the code that uses them. Edit the files here
instead of hand-writing config in Rust.

## Layout

- `docker/` — docker-compose building blocks
  - `compose.header.yml` — header prepended to every generated compose file
  - `services/` — one template per engine. `{{PLACEHOLDERS}}` are substituted at
    runtime by the matching `Engine` impl in the per-kind `engine.rs`.
    - `anvil.yml` — Anvil (EVM) service
    - `solana.yml` — surfpool (Solana) service
    - `starknet.yml` — starknet-devnet service
    - `utxo.yml` — bitcoind / litecoind (Bitcoin, Litecoin) regtest service
    - `zksync.yml` — anvil-zksync (zkSync) service
    - `otterscan.yml`, `btc-rpc-explorer.yml` — bundled block-explorer sidecars
- `contracts/` — test-token sources (ERC-20 / Cairo) baked into the chain snapshots
- `abi/`, `presets/`, `state/` — embedded ABIs, canonical-contract presets, and
  baked chain state (see `presets/README.md`)

## Source vs. generated

Files here are **source** and are committed. The compose file wharfnet writes at
runtime (`.wharfnet/docker-compose.yml` in your working directory) is a
**generated artifact** — gitignored and rewritten on every `wharfnet up`.

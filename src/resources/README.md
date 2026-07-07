# src/resources/

Static assets embedded into the `wharfnet` binary at compile time (via
`include_str!`). Co-located with the code that uses them. Edit the files here
instead of hand-writing config in Rust.

## Layout

- `docker/` — docker-compose building blocks
  - `compose.header.yml` — header prepended to every generated compose file
  - `services/` — one template per engine. `{{PLACEHOLDERS}}` are substituted at
    runtime by the matching `Engine` impl in `src/engine.rs`.
    - `anvil.yml` — Anvil (EVM) service
    - _(M2)_ `solana.yml`, `starknet.yml`, …
- _(M4)_ `contracts/` — test-token sources (ERC-20 / SPL / Cairo) deployed by
  `wharfnet deploy`

## Source vs. generated

Files here are **source** and are committed. The compose file wharfnet writes at
runtime (`.wharfnet/docker-compose.yml` in your working directory) is a
**generated artifact** — gitignored and rewritten on every `wharfnet up`.

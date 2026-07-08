# Canonical infra bytecode

Runtime bytecode of well-known contracts that live at the **same address on
every real EVM chain**. `gen-token-state.sh` etches these into the baked Anvil
snapshot (via `anvil_setCode`) so every wharfnet chain has them pre-deployed and
libraries/tooling that hardcode these addresses work out of the box.

| File             | Contract   | Canonical address                            |
| ---------------- | ---------- | -------------------------------------------- |
| `multicall3.hex` | Multicall3 | `0xcA11bde05977b3631167028862bE2a173976CA11` |
| `permit2.hex`    | Permit2    | `0x000000000022D473030F116dDEE9F6B43aC78BA3` |

Each file is the exact `0x`-prefixed runtime bytecode returned by `eth_getCode`
for that address on Ethereum mainnet — the deployed code is immutable, so it's
identical on every chain. Both contracts are stateless at deploy time (Permit2
recomputes its EIP-712 domain separator when `block.chainid` differs from the
one cached in its bytecode), so etching the runtime code is equivalent to a real
deployment.

Refresh them from mainnet with:

```sh
cast code 0xcA11bde05977b3631167028862bE2a173976CA11 --rpc-url <mainnet> > multicall3.hex
cast code 0x000000000022D473030F116dDEE9F6B43aC78BA3 --rpc-url <mainnet> > permit2.hex
```

The CREATE2 deterministic deployer (`0x4e59b44847b379578588920cA78FbF26c0B4956C`)
is **not** here — Anvil deploys it automatically, so it's already in the snapshot.

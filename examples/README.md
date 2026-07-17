# Examples

Task-oriented recipes for driving a running wharfnet localnet — "how do I do X".
Each script is self-contained and reads the endpoints, accounts, and token
addresses straight from the manifest at `.wharfnet/wharfnet.json`, so nothing is
hard-coded and they keep working if you change the topology.

Start the network first, then run any recipe:

```sh
wharfnet up          # or: wharfnet up --bare
./examples/evm/fund-and-transfer.sh
```

## Recipes

| Recipe | What it shows |
| ------ | ------------- |
| [evm/fund-and-transfer.sh](evm/fund-and-transfer.sh) | Fund an address from the faucet, then send an ERC-20 transfer and read balances |
| [evm/snapshot-revert.sh](evm/snapshot-revert.sh) | Snapshot state, mutate it, and roll back — the test-isolation pattern |
| [evm/fork-and-impersonate.sh](evm/fork-and-impersonate.sh) | Fork mainnet, impersonate a whale, and move real USDC with no key |
| [solana/airdrop-and-tokens.sh](solana/airdrop-and-tokens.sh) | Airdrop SOL and top up SPL tokens, then read balances over JSON-RPC |
| [starknet/fund-and-read.sh](starknet/fund-and-read.sh) | Fund ETH/STRK and the Cairo test tokens, then read an ERC-20 balance |
| [ci/github-actions.yml](ci/github-actions.yml) | Boot the localnet in CI, run integration tests, tear it down |

## Prerequisites

- **Docker** with the Compose plugin — every recipe boots chains through it.
- **[jq](https://jqlang.github.io/jq/)** — the scripts read the manifest with it.
- **curl** — for the raw JSON-RPC calls (Solana/Starknet recipes).
- **[Foundry](https://book.getfoundry.sh/) (`cast`)** — only the EVM recipes use it.

Each recipe prints the exact commands it runs, so you can copy individual steps
into your own test setup or CI pipeline.

> These are shell recipes against the CLI + standard chain tooling. To consume
> the localnet from application code, point your usual client at the manifest's
> `rpc`/`ws` URLs — e.g. viem/ethers for EVM, `@solana/web3.js` for Solana (it
> derives the WebSocket URL from the RPC URL), or starknet.js for Starknet.

// Changelog timeline data — newest first. Dates are the commit dates the
// feature landed; `tag` is the PR it shipped in (optional). Descriptions may
// use `backticks` for inline code, rendered as code chips.
export const categories = ['Solana', 'Starknet', 'EVM', 'Core']

export const entries = [
  {
    date: 'July 16, 2026',
    category: 'Solana',
    tag: '#14',
    title: 'Solana WebSocket RPC',
    changes: [
      "surfpool's WebSocket endpoint is now published on the HTTP RPC port `+ 1` (`solana-1` → `ws://127.0.0.1:8900`), so subscriptions (`slotSubscribe`, `logsSubscribe`) and `confirmTransaction` work from the host.",
      'Advertised via a new `ws` field in the `status`/manifest; clients like `@solana/web3.js` derive the URL automatically.'
    ]
  },
  {
    date: 'July 16, 2026',
    category: 'Solana',
    tag: '#14',
    title: 'Solana block explorer',
    changes: [
      "Every Solana chain now serves surfpool's built-in **Studio** UI, on by default and skipped by `up --bare`, published on the chain's RPC port `+ 10000` (`solana-1` → `http://127.0.0.1:18899`)."
    ]
  },
  {
    date: 'July 12, 2026',
    category: 'Solana',
    tag: '#14',
    title: 'Solana persistence',
    changes: [
      '`up --resume` / `up --reset` now cover Solana chains, each persisting to its own `session-<chain>.sqlite` surfnet database via surfpool’s `--db`.',
      'A resumed chain detects the SPL test tokens are already present and skips re-seeding, so it never clobbers your balances.'
    ]
  },
  {
    date: 'July 12, 2026',
    category: 'Solana',
    tag: '#14',
    title: 'Solana forking',
    changes: [
      '`fork_url` now works on Solana chains, booting them as a **copy-on-read** fork of a live network via surfpool’s `--rpc-url`. `fork_block` is unsupported (surfpool has no fork-at-slot flag) and is rejected on load.'
    ]
  },
  {
    date: 'July 12, 2026',
    category: 'Solana',
    tag: '#14',
    title: 'Solana faucet',
    changes: [
      'The unified `faucet` command funds Solana addresses: native SOL through `requestAirdrop`, and the SPL test tokens through surfpool’s `surfnet_setTokenAccount` cheat (the recipient needs no key). Additive, with `--raw` for exact base units.'
    ]
  },
  {
    date: 'July 12, 2026',
    category: 'Solana',
    tag: '#14',
    title: 'Solana test tokens',
    changes: [
      'Every Solana chain boots with standard SPL test tokens (USDC, WBTC) at fixed mint addresses, seeded onto the dev accounts via cheatcodes the moment the RPC is live — no program to deploy.'
    ]
  },
  {
    date: 'July 12, 2026',
    category: 'Solana',
    tag: '#14',
    title: 'Solana chain control',
    changes: [
      '`wharfnet solana mine | increase-time | warp | pause-clock | resume-clock` wrap surfpool’s `surfnet_*` cheat RPC. `mine` advances slots, `warp` is forward-only, and `pause-clock`/`resume-clock` give step-by-step slot control.'
    ]
  },
  {
    date: 'July 12, 2026',
    category: 'Solana',
    tag: '#14',
    title: 'Solana chains',
    changes: [
      'A `surfpool` chain (`solana-1`, `:8899`) now boots **by default** alongside the EVM and Starknet chains, with three deterministic dev accounts funded with 10,000 SOL each. Selectable via `kind = "solana"` in `wharfnet.toml`.'
    ]
  },
  {
    date: 'July 11, 2026',
    category: 'Core',
    tag: '#13',
    title: 'Faster multi-chain boot',
    changes: [
      '`up` now health-checks every chain concurrently, so boot waits on the slowest chain rather than the sum of them.'
    ]
  },
  {
    date: 'July 11, 2026',
    category: 'Core',
    tag: '#13',
    title: 'Fractional & raw faucet amounts',
    changes: [
      '`faucet <chain> <address> <amount>` takes a decimal `amount` (e.g. `1.5`), scaled by the token’s decimals. Pass `--raw` to fund an exact base-unit integer.'
    ]
  },
  {
    date: 'July 11, 2026',
    category: 'Core',
    tag: '#13',
    title: '`logs` command',
    changes: [
      '`wharfnet logs [chain] [--follow]` streams container logs through `docker compose logs` — all services, or filtered by chain kind or name.'
    ]
  },
  {
    date: 'July 10, 2026',
    category: 'Starknet',
    tag: '#11',
    title: 'Starknet chains',
    changes: [
      'A `starknet-devnet` chain (`starknet-1`, `:5050`) boots **by default** with deterministic predeployed accounts, the ETH/STRK fee tokens, and baked **Cairo test tokens** (USDC, WBTC, FEE, REB) at fixed addresses.'
    ]
  },
  {
    date: 'July 10, 2026',
    category: 'Starknet',
    tag: '#11',
    title: 'Starknet faucet, forking & control',
    changes: [
      'The unified `faucet` funds ETH/STRK and the Cairo test tokens via signed invokes on **JSON-RPC 0.10**.',
      '`fork_url`/`fork_block` mirror a live Starknet network; `wharfnet starknet mine | increase-time | warp | impersonate` drive a running chain.'
    ]
  },
  {
    date: 'July 10, 2026',
    category: 'Starknet',
    tag: '#11',
    title: 'Starknet persistence & explorer',
    changes: [
      '`up --resume` / `up --reset` cover Starknet chains via a per-chain replay log.',
      "starknet-devnet's built-in web UI explorer is served in-process at `/ui` on the chain's RPC port, on by default."
    ]
  },
  {
    date: 'July 8, 2026',
    category: 'EVM',
    title: 'Mainnet forking',
    changes: [
      'Point a chain at a live RPC with `fork_url` (and optional `fork_block`) in `wharfnet.toml` and it boots as a fork via Anvil’s `--fork-url`. `${VAR}` keys are expanded from the environment and never recorded.'
    ]
  },
  {
    date: 'July 8, 2026',
    category: 'EVM',
    title: 'Canonical contracts & weird tokens',
    changes: [
      'Every EVM chain boots with Multicall3, Permit2, and the CREATE2 deployer at their real addresses, plus deliberately non-standard test tokens — fee-on-transfer (FEE), rebasing (REB), and no-return (NRT) — for integration testing.'
    ]
  },
  {
    date: 'July 8, 2026',
    category: 'EVM',
    title: 'EVM chain control & block explorer',
    changes: [
      '`wharfnet evm mine | increase-time | warp | impersonate | snapshot | revert` wrap Anvil’s cheat RPCs.',
      'An Otterscan explorer boots per EVM chain by default (`up --bare` to skip) — no indexer, thanks to Anvil’s native `ots_*` API.'
    ]
  },
  {
    date: 'July 8, 2026',
    category: 'Core',
    title: 'Config file & persistent state',
    changes: [
      '`wharfnet.toml` customises the chain topology (name, port, `chain_id`, `block_time`); override the path with `--config` or `WHARFNET_CONFIG`.',
      '`up --resume` / `up --reset` keep or wipe balances, txs, and deployments across `down`.'
    ]
  },
  {
    date: 'July 7, 2026',
    category: 'EVM',
    title: 'Faucet & pre-deployed test tokens',
    changes: [
      'USDC and WBTC deploy at fixed addresses on every EVM chain, each with a public `mint`.',
      '`faucet <chain> <address> [amount] [--token SYMBOL]` funds native ETH plus every token, or just one, with no private key.'
    ]
  },
  {
    date: 'July 7, 2026',
    category: 'EVM',
    title: 'First & second EVM chains',
    changes: [
      '`up` boots a local Anvil chain (`anvil-1`, `:8545`, chainId `31337`) via Docker Compose and writes an endpoints manifest, with a second chain (`anvil-2`, `:8546`) for cross-chain tests.'
    ]
  },
  {
    date: 'June 24, 2026',
    category: 'Core',
    title: 'CLI scaffold',
    changes: [
      'The Rust/`clap` command surface (`up`, `down`, `status`, `compose`, `faucet`) that everything else builds on.'
    ]
  }
]

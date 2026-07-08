#!/usr/bin/env bash
#
# gen-token-state.sh — build the Anvil state snapshot that `wharfnet up` loads
# into every EVM chain. It deploys the bundled test tokens, etches the canonical
# infra contracts, and dumps the resulting state to a snapshot.
#
# Tokens are deployed from Anvil dev account 0 at fixed nonces, so their
# addresses are deterministic and identical on every chain:
#   nonce 0  USDC -> 0x5FbDB2315678afecb367f032d93F642f64180aa3
#   nonce 1  WBTC -> 0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512
#   nonce 2  FEE  -> 0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0  (fee-on-transfer)
#   nonce 3  REB  -> 0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9  (rebasing)
#   nonce 4  NRT  -> 0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9  (no return value)
#
# Canonical infra etched at its real mainnet address (bytecode in resources/presets):
#   Multicall3 -> 0xcA11bde05977b3631167028862bE2a173976CA11
#   Permit2    -> 0x000000000022D473030F116dDEE9F6B43aC78BA3
# The CREATE2 deterministic deployer (0x4e59…4956C) is deployed by Anvil itself.
#
# Regenerate after changing the token sources, seed amounts, or preset bytecode:
#   ./scripts/gen-token-state.sh
#
set -euo pipefail

IMAGE="ghcr.io/foundry-rs/foundry:stable"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONTRACTS="$ROOT/src/resources/contracts"
PRESETS="$ROOT/src/resources/presets"
OUT_DIR="$ROOT/src/resources/state"
STATE_FILE="$OUT_DIR/anvil-tokens.json"

# Anvil dev account 0 — the deployer (well-known throwaway key).
DEPLOYER_PK="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
# Dev accounts 0..2 get an initial balance of each token.
DEV_ACCOUNTS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"

mkdir -p "$OUT_DIR"
echo "⚓ generating token state snapshot -> $STATE_FILE"

docker run --rm \
  -v "$CONTRACTS":/contracts:ro \
  -v "$PRESETS":/presets:ro \
  -v "$OUT_DIR":/out \
  -e DEPLOYER_PK="$DEPLOYER_PK" \
  -e DEV_ACCOUNTS="$DEV_ACCOUNTS" \
  --entrypoint sh "$IMAGE" -eu -c '
    set -eu
    RPC="http://127.0.0.1:8545"

    # Boot a throwaway anvil that dumps its state to /out on exit.
    anvil --host 0.0.0.0 --port 8545 --chain-id 31337 \
          --dump-state /out/anvil-tokens.json --state-interval 2 --silent &
    ANVIL_PID=$!

    # Wait for the RPC to answer.
    i=0
    until cast block-number --rpc-url "$RPC" >/dev/null 2>&1; do
      i=$((i + 1)); [ "$i" -gt 150 ] && { echo "anvil never became ready"; exit 1; }
      sleep 0.2
    done

    # Work on a writable copy so the mounted source stays clean.
    cp -r /contracts /tmp/work && cd /tmp/work

    deployed_to() { sed -n "s/^Deployed to: \(0x[0-9a-fA-F]*\)/\1/p"; }

    # Standard tokens (nonces 0,1) — TestToken takes name/symbol/decimals.
    USDC=$(forge create src/TestToken.sol:TestToken --rpc-url "$RPC" \
      --private-key "$DEPLOYER_PK" --broadcast \
      --constructor-args "USD Coin" "USDC" 6 | deployed_to)
    WBTC=$(forge create src/TestToken.sol:TestToken --rpc-url "$RPC" \
      --private-key "$DEPLOYER_PK" --broadcast \
      --constructor-args "Wrapped BTC" "WBTC" 8 | deployed_to)

    # Weird tokens (nonces 2,3,4) — parameters are baked into each contract.
    FEE=$(forge create src/WeirdTokens.sol:FeeOnTransferToken --rpc-url "$RPC" \
      --private-key "$DEPLOYER_PK" --broadcast | deployed_to)
    REB=$(forge create src/WeirdTokens.sol:RebasingToken --rpc-url "$RPC" \
      --private-key "$DEPLOYER_PK" --broadcast | deployed_to)
    NRT=$(forge create src/WeirdTokens.sol:NoReturnToken --rpc-url "$RPC" \
      --private-key "$DEPLOYER_PK" --broadcast | deployed_to)

    for v in "$USDC" "$WBTC" "$FEE" "$REB" "$NRT"; do
      [ -n "$v" ] || { echo "deploy failed"; exit 1; }
    done
    echo "USDC=$USDC WBTC=$WBTC FEE=$FEE REB=$REB NRT=$NRT"

    # Etch canonical infra at its real mainnet address from the preset bytecode.
    MULTICALL3="0xcA11bde05977b3631167028862bE2a173976CA11"
    PERMIT2="0x000000000022D473030F116dDEE9F6B43aC78BA3"
    cast rpc anvil_setCode "$MULTICALL3" "$(cat /presets/multicall3.hex)" --rpc-url "$RPC" >/dev/null
    cast rpc anvil_setCode "$PERMIT2"    "$(cat /presets/permit2.hex)"    --rpc-url "$RPC" >/dev/null
    echo "etched Multicall3 + Permit2"

    # Seed dev accounts with 1,000,000 of each token (raw units per decimals).
    seed() { # symbol addr amount
      for A in $DEV_ACCOUNTS; do
        cast send "$2" "mint(address,uint256)" "$A" "$3" \
          --rpc-url "$RPC" --private-key "$DEPLOYER_PK" >/dev/null
      done
    }
    seed USDC "$USDC" 1000000000000                    # 1e6  * 1e6
    seed WBTC "$WBTC" 100000000000000                  # 1e6  * 1e8
    seed FEE  "$FEE"  1000000000000000000000000        # 1e6  * 1e18
    seed REB  "$REB"  1000000000000000000000000        # 1e6  * 1e18
    seed NRT  "$NRT"  1000000000000                    # 1e6  * 1e6

    echo "sanity: USDC.balanceOf(dev0) = $(cast call "$USDC" "balanceOf(address)(uint256)" 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 --rpc-url "$RPC")"
    echo "sanity: REB.balanceOf(dev0)  = $(cast call "$REB"  "balanceOf(address)(uint256)" 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 --rpc-url "$RPC")"
    echo "sanity: Multicall3.code      = $(cast code "$MULTICALL3" --rpc-url "$RPC" | cut -c1-12)…"
    echo "sanity: Permit2.code         = $(cast code "$PERMIT2"    --rpc-url "$RPC" | cut -c1-12)…"

    # Let a periodic dump land, then stop anvil so it flushes final state.
    sleep 3
    kill -INT "$ANVIL_PID" 2>/dev/null || true
    wait "$ANVIL_PID" 2>/dev/null || true

    ls -la /out/anvil-tokens.json
  '

echo "✅ wrote $STATE_FILE"

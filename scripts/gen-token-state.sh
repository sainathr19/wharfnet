#!/usr/bin/env bash
#
# gen-token-state.sh — deploy the bundled test tokens to a throwaway Anvil and
# dump its state to a snapshot that `wharfnet up` loads into every EVM chain.
#
# The tokens are deployed from Anvil dev account 0 at nonces 0 and 1, so their
# addresses are deterministic:
#   USDC -> 0x5FbDB2315678afecb367f032d93F642f64180aa3
#   WBTC -> 0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512
#
# Regenerate the snapshot after changing TestToken.sol or the seed amounts:
#   ./scripts/gen-token-state.sh
#
set -euo pipefail

IMAGE="ghcr.io/foundry-rs/foundry:stable"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONTRACTS="$ROOT/src/resources/contracts"
OUT_DIR="$ROOT/src/resources/state"
STATE_FILE="$OUT_DIR/anvil-tokens.json"

# Anvil dev account 0 — the deployer (well-known throwaway key).
DEPLOYER_PK="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
# Dev accounts 0..2 get an initial balance of each token.
DEV_ACCOUNTS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
# Seed amounts (raw units): 1,000,000 USDC (6 dec) and 100 WBTC (8 dec).
USDC_SEED="1000000000000"
WBTC_SEED="10000000000"

mkdir -p "$OUT_DIR"
echo "⚓ generating token state snapshot -> $STATE_FILE"

docker run --rm \
  -v "$CONTRACTS":/contracts:ro \
  -v "$OUT_DIR":/out \
  -e DEPLOYER_PK="$DEPLOYER_PK" \
  -e DEV_ACCOUNTS="$DEV_ACCOUNTS" \
  -e USDC_SEED="$USDC_SEED" \
  -e WBTC_SEED="$WBTC_SEED" \
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

    USDC=$(forge create src/TestToken.sol:TestToken --rpc-url "$RPC" \
      --private-key "$DEPLOYER_PK" --broadcast \
      --constructor-args "USD Coin" "USDC" 6 | deployed_to)
    WBTC=$(forge create src/TestToken.sol:TestToken --rpc-url "$RPC" \
      --private-key "$DEPLOYER_PK" --broadcast \
      --constructor-args "Wrapped BTC" "WBTC" 8 | deployed_to)

    [ -n "$USDC" ] && [ -n "$WBTC" ] || { echo "deploy failed"; exit 1; }
    echo "USDC deployed to $USDC"
    echo "WBTC deployed to $WBTC"

    for A in $DEV_ACCOUNTS; do
      cast send "$USDC" "mint(address,uint256)" "$A" "$USDC_SEED" \
        --rpc-url "$RPC" --private-key "$DEPLOYER_PK" >/dev/null
      cast send "$WBTC" "mint(address,uint256)" "$A" "$WBTC_SEED" \
        --rpc-url "$RPC" --private-key "$DEPLOYER_PK" >/dev/null
    done

    echo "sanity: USDC.balanceOf(dev0) = $(cast call "$USDC" "balanceOf(address)(uint256)" 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 --rpc-url "$RPC")"
    echo "sanity: WBTC.decimals()      = $(cast call "$WBTC" "decimals()(uint8)" --rpc-url "$RPC")"

    # Let a periodic dump land, then stop anvil so it flushes final state.
    sleep 3
    kill -INT "$ANVIL_PID" 2>/dev/null || true
    wait "$ANVIL_PID" 2>/dev/null || true

    ls -la /out/anvil-tokens.json
  '

echo "✅ wrote $STATE_FILE"

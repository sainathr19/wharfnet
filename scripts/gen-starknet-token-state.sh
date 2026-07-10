#!/usr/bin/env bash
#
# gen-starknet-token-state.sh — build the starknet-devnet replay log that
# `wharfnet up` loads into every Starknet chain. It declares the Cairo test-token
# classes, deploys the tokens at deterministic addresses, and seeds the dev
# accounts with an initial balance of each.
#
# A devnet "dump" is a replay log of JSON-RPC requests (declares, deploys,
# invokes); on boot devnet re-executes it, so the tokens are present on every
# fresh chain. StarknetEngine boots with `--dump-on request --dump-path <this>`.
#
# Tokens deploy from seed-0 account 0 via UDC with fixed salts (--not-unique), so
# their addresses are deterministic given the pinned Cairo toolchain + devnet
# image. The printed addresses are hardcoded in src/starknet/engine.rs:
#   salt 0x1  USDC (6dp)   standard
#   salt 0x2  WBTC (8dp)   standard
#   salt 0x3  FEE  (18dp)  fee-on-transfer
#   salt 0x4  REB  (18dp)  rebasing
#
# Requires host-side: docker, scarb, starkli.
# Regenerate after changing the Cairo sources, seed amounts, or the pinned image:
#   ./scripts/gen-starknet-token-state.sh
#
set -euo pipefail

IMAGE="shardlabs/starknet-devnet-rs:0.4.3"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONTRACTS="$ROOT/src/resources/contracts/starknet"
OUT_DIR="$ROOT/src/resources/state"
STATE_FILE="$OUT_DIR/starknet-tokens.json"
PORT=5793
NAME="wharfnet-tokengen"

# seed-0 dev accounts — must match src/starknet/engine.rs. Account 0 is the deployer.
DEPLOYER="0x64b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691"
DEPLOYER_PK="0x71d7bb07b9a64f6f78ac4c816aff4da9"
DEV_ACCOUNTS=(
  "0x64b48806902a367c8598f4f95c305e8c1a1acba5f082d294a43793113115691"
  "0x78662e7352d062084b0010068b99288486c2d8b914f6e2a55ce945f8792c8b1"
  "0x49dfb8ce986e21d354ac93ea65e6a11f639c1934ea253e5ff14ca62eca0f38e"
)

for bin in docker scarb starkli curl; do
  command -v "$bin" >/dev/null || { echo "error: missing required tool '$bin'" >&2; exit 1; }
done

mkdir -p "$OUT_DIR"
WORK="$(mktemp -d)"
mkdir -p "$WORK/state"
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; rm -rf "$WORK"; }
trap cleanup EXIT

echo "⚓ compiling Cairo test tokens…"
( cd "$CONTRACTS" && scarb build )
CLASSDIR="$CONTRACTS/target/dev"

echo "⚓ booting throwaway devnet…"
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" -p "$PORT:5050" -v "$WORK/state:/state" \
  "$IMAGE" --seed 0 --dump-on request --dump-path /state/starknet-tokens.json >/dev/null
for i in $(seq 1 30); do
  curl -sf "http://127.0.0.1:$PORT/is_alive" >/dev/null 2>&1 && break
  [ "$i" = 30 ] && { echo "error: devnet never became ready" >&2; exit 1; }
  sleep 1
done

export STARKNET_RPC="http://127.0.0.1:$PORT/rpc"
ACCT="$WORK/acct.json"
starkli account fetch "$DEPLOYER" --output "$ACCT" >/dev/null 2>&1
AUTH=(--account "$ACCT" --private-key "$DEPLOYER_PK")

declare_class() { # sierra_file -> class hash (last stdout line)
  starkli declare "$1" "${AUTH[@]}" 2>/dev/null | tail -1
}
deploy() { # class salt [ctor args...] -> deterministic address
  local class="$1" salt="$2"
  shift 2
  starkli deploy "$class" "$@" --salt "$salt" --not-unique "${AUTH[@]}" 2>&1 \
    | grep -i "deployed at address" | grep -oE '0x[0-9a-fA-F]+' | head -1
}
seed() { # token amount — mint to every dev account
  local token="$1" amount="$2" acct
  for acct in "${DEV_ACCOUNTS[@]}"; do
    starkli invoke "$token" mint "$acct" "u256:$amount" "${AUTH[@]}" >/dev/null 2>&1
  done
}

echo "⚓ declaring classes…"
TT="$(declare_class "$CLASSDIR/wharfnet_tokens_TestToken.contract_class.json")"
FT="$(declare_class "$CLASSDIR/wharfnet_tokens_FeeToken.contract_class.json")"
RT="$(declare_class "$CLASSDIR/wharfnet_tokens_RebasingToken.contract_class.json")"

echo "⚓ deploying tokens…"
USDC="$(deploy "$TT" 0x1 "str:USD Coin" "str:USDC" 6)"
WBTC="$(deploy "$TT" 0x2 "str:Wrapped BTC" "str:WBTC" 8)"
FEE="$(deploy "$FT" 0x3)"
REB="$(deploy "$RT" 0x4)"

echo "⚓ seeding dev accounts (1,000,000 of each token)…"
seed "$USDC" 1000000000000                   # 1e6 * 1e6
seed "$WBTC" 100000000000000                  # 1e6 * 1e8
seed "$FEE"  1000000000000000000000000        # 1e6 * 1e18
seed "$REB"  1000000000000000000000000        # 1e6 * 1e18

echo "⚓ dumping replay log…"
curl -sf -X POST "http://127.0.0.1:$PORT/dump" >/dev/null
cp "$WORK/state/starknet-tokens.json" "$STATE_FILE"

cat <<EOF

✅ wrote $STATE_FILE ($(wc -c <"$STATE_FILE" | tr -d ' ') bytes)

   Deterministic token addresses — hardcode these in src/starknet/engine.rs:
     USDC  $USDC
     WBTC  $WBTC
     FEE   $FEE
     REB   $REB

   Class hashes:
     TestToken      $TT
     FeeToken       $FT
     RebasingToken  $RT
EOF

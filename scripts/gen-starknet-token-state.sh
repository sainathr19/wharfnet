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
# The declare/deploy/seed step runs through `examples/gen_starknet_tokens.rs`,
# which uses the same `starknet-rust` client the runtime faucet does — so there's
# no external Starknet CLI to keep in sync with the devnet's JSON-RPC version.
# Tokens deploy from seed-0 account 0 via the legacy UDC with fixed salts
# (unique=false), so their addresses are deterministic given the pinned Cairo
# toolchain + devnet image. The printed addresses are hardcoded in
# src/starknet/engine.rs:
#   salt 0x1  USDC (6dp)   standard
#   salt 0x2  WBTC (8dp)   standard
#   salt 0x3  FEE  (18dp)  fee-on-transfer
#   salt 0x4  REB  (18dp)  rebasing
#
# Requires host-side: docker, scarb, cargo, curl.
# Regenerate after changing the Cairo sources, seed amounts, or the pinned image:
#   ./scripts/gen-starknet-token-state.sh
#
set -euo pipefail

IMAGE="shardlabs/starknet-devnet-rs:0.9.1"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONTRACTS="$ROOT/src/resources/contracts/starknet"
OUT_DIR="$ROOT/src/resources/state"
STATE_FILE="$OUT_DIR/starknet-tokens.json"
PORT=5793
NAME="wharfnet-tokengen"

for bin in docker scarb cargo curl; do
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

echo "⚓ booting throwaway devnet ($IMAGE)…"
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" -p "$PORT:5050" -v "$WORK/state:/state" \
  "$IMAGE" --seed 0 --dump-on block --dump-path /state/starknet-tokens.json >/dev/null
for i in $(seq 1 30); do
  curl -sf "http://127.0.0.1:$PORT/is_alive" >/dev/null 2>&1 && break
  [ "$i" = 30 ] && { echo "error: devnet never became ready" >&2; exit 1; }
  sleep 1
done

echo "⚓ declaring, deploying & seeding via starknet-rust…"
ADDRS="$(cargo run --quiet --example gen_starknet_tokens -- "http://127.0.0.1:$PORT/rpc" "$CLASSDIR")"

echo "⚓ dumping replay log…"
curl -sf -X POST "http://127.0.0.1:$PORT/rpc" \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"devnet_dump","params":{}}' >/dev/null
cp "$WORK/state/starknet-tokens.json" "$STATE_FILE"

cat <<EOF

✅ wrote $STATE_FILE ($(wc -c <"$STATE_FILE" | tr -d ' ') bytes)

   Deterministic token addresses — hardcode these in src/starknet/engine.rs:
$ADDRS
EOF

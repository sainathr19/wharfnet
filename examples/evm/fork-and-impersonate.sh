#!/usr/bin/env bash
# Fork Ethereum mainnet, impersonate a whale, and move real USDC with no private
# key — the pattern for testing against live protocol state locally. This recipe
# boots its own fork network and tears it down at the end.
#
# Needs: docker (wharfnet), jq, Foundry's `cast`, and an archive/full RPC URL in
# $MAINNET_RPC.
set -euo pipefail

: "${MAINNET_RPC:?set MAINNET_RPC to an Ethereum RPC URL, e.g. https://eth.llamarpc.com}"

# Real mainnet USDC, and a well-known address that holds a lot of it. Forked
# state is live, so these are the actual mainnet values.
USDC="0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
WHALE="0x28C6c06298d514Db089934071355E5743bf21d60"   # Binance hot wallet

# Write a throwaway config that forks mainnet. ${MAINNET_RPC} is expanded by
# wharfnet from the environment, so the key never lands in the file or manifest.
CONFIG=$(mktemp -t wharfnet-fork-XXXX.toml)
trap 'wharfnet down >/dev/null 2>&1 || true; rm -f "$CONFIG"' EXIT
cat >"$CONFIG" <<'TOML'
[[chains]]
name = "mainnet"
port = 8545
chain_id = 1
fork_url = "${MAINNET_RPC}"
TOML

echo "==> boot a mainnet fork"
wharfnet up --config "$CONFIG" --bare

MANIFEST=".wharfnet/wharfnet.json"
RPC=$(jq -r '.chains[] | select(.name=="mainnet") | .rpc' "$MANIFEST")
RECIPIENT=$(jq -r '.chains[] | select(.name=="mainnet") | .accounts[0].address' "$MANIFEST")

bal() { cast call "$USDC" "balanceOf(address)(uint256)" "$1" --rpc-url "$RPC"; }
echo "whale USDC:     $(bal "$WHALE")"
echo "recipient USDC: $(bal "$RECIPIENT")"

# 1) Unlock the whale so we can send transactions as it without its key.
echo
echo "==> impersonate the whale and send 100 USDC to a dev account"
wharfnet evm impersonate "$WHALE" --chain mainnet

# 2) Send as the impersonated account. `--unlocked --from` tells cast to use the
#    node-unlocked sender rather than signing locally.
cast send "$USDC" "transfer(address,uint256)" "$RECIPIENT" 100000000 \
  --from "$WHALE" --unlocked --rpc-url "$RPC" >/dev/null

wharfnet evm impersonate "$WHALE" --stop --chain mainnet

echo
echo "after transfer:"
echo "  whale USDC:     $(bal "$WHALE")"
echo "  recipient USDC: $(bal "$RECIPIENT")"

#!/usr/bin/env bash
# Snapshot chain state, mutate it, then roll back — the pattern for isolating
# tests so each one starts from a known state without a full reboot. Assumes
# `wharfnet up` is already running.
#
# Needs: docker (wharfnet), jq, and Foundry's `cast`.
set -euo pipefail

MANIFEST=".wharfnet/wharfnet.json"
[ -f "$MANIFEST" ] || { echo "No manifest at $MANIFEST — run 'wharfnet up' first." >&2; exit 1; }

RPC=$(jq -r '.chains[] | select(.name=="anvil-1") | .rpc' "$MANIFEST")
ACCT=$(jq -r '.chains[] | select(.name=="anvil-1") | .accounts[0].address' "$MANIFEST")

block() { cast block-number --rpc-url "$RPC"; }
balance() { cast balance "$ACCT" --rpc-url "$RPC"; }

echo "before: block=$(block)  balance=$(balance) wei"

# 1) Take a snapshot. The command prints an id like `0x1`; grab the first hex
#    token from its output so this works regardless of the surrounding text.
echo
echo "==> wharfnet evm snapshot --chain anvil-1"
SNAP=$(wharfnet evm snapshot --chain anvil-1 | grep -oE '0x[0-9a-fA-F]+' | head -1)
echo "snapshot id: $SNAP"

# 2) Mutate state — mine some blocks and fund the account. In a real test this
#    is where your transactions / deploys would run.
echo
echo "==> mine 5 blocks + faucet 50 ETH"
wharfnet evm mine 5 --chain anvil-1 >/dev/null
wharfnet faucet anvil-1 "$ACCT" 50 --token ETH >/dev/null
echo "after mutation: block=$(block)  balance=$(balance) wei"

# 3) Revert to the snapshot — block height and balances return to step 1.
echo
echo "==> wharfnet evm revert $SNAP --chain anvil-1"
wharfnet evm revert "$SNAP" --chain anvil-1 >/dev/null
echo "after revert: block=$(block)  balance=$(balance) wei"

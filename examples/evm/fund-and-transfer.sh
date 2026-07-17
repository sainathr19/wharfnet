#!/usr/bin/env bash
# Fund an address from the faucet, then send an ERC-20 transfer and read the
# resulting balances — the everyday "give my test account some USDC and move it
# around" flow. Assumes `wharfnet up` is already running.
#
# Needs: docker (wharfnet), jq, and Foundry's `cast`.
set -euo pipefail

MANIFEST=".wharfnet/wharfnet.json"
[ -f "$MANIFEST" ] || { echo "No manifest at $MANIFEST — run 'wharfnet up' first." >&2; exit 1; }

# Pull everything from the manifest so nothing is hard-coded.
RPC=$(jq -r '.chains[] | select(.name=="anvil-1") | .rpc' "$MANIFEST")
USDC=$(jq -r '.chains[] | select(.name=="anvil-1") | .tokens[] | select(.symbol=="USDC") | .address' "$MANIFEST")
SENDER=$(jq -r '.chains[] | select(.name=="anvil-1") | .accounts[0].address' "$MANIFEST")
SENDER_PK=$(jq -r '.chains[] | select(.name=="anvil-1") | .accounts[0].private_key' "$MANIFEST")
RECIPIENT=$(jq -r '.chains[] | select(.name=="anvil-1") | .accounts[1].address' "$MANIFEST")

echo "RPC:       $RPC"
echo "USDC:      $USDC"
echo "sender:    $SENDER"
echo "recipient: $RECIPIENT"
echo

# 1) Top up the recipient with 1,000 USDC from the faucet (no key needed — the
#    faucet calls the token's public mint). Amount is whole units, scaled by the
#    token's 6 decimals.
echo "==> faucet: mint 1,000 USDC to the recipient"
wharfnet faucet anvil-1 "$RECIPIENT" 1000 --token USDC

bal() { cast call "$USDC" "balanceOf(address)(uint256)" "$1" --rpc-url "$RPC"; }
echo "recipient USDC after faucet: $(bal "$RECIPIENT")"

# 2) Send 250 USDC from the recipient back to the sender. 250 * 10^6 base units.
echo
echo "==> transfer 250 USDC recipient -> sender"
RECIPIENT_PK=$(jq -r '.chains[] | select(.name=="anvil-1") | .accounts[1].private_key' "$MANIFEST")
cast send "$USDC" "transfer(address,uint256)" "$SENDER" 250000000 \
  --private-key "$RECIPIENT_PK" --rpc-url "$RPC" >/dev/null

# 3) Read both balances to confirm the move.
echo
echo "final balances (base units, 6 decimals):"
echo "  recipient: $(bal "$RECIPIENT")"
echo "  sender:    $(bal "$SENDER")"

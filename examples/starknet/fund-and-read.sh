#!/usr/bin/env bash
# Fund an address with the Cairo test tokens from the faucet, then read an ERC-20
# balance over the Starknet JSON-RPC. Assumes `wharfnet up` is already running.
#
# Needs: docker (wharfnet), jq, curl. No Starknet toolchain required.
set -euo pipefail

MANIFEST=".wharfnet/wharfnet.json"
[ -f "$MANIFEST" ] || { echo "No manifest at $MANIFEST — run 'wharfnet up' first." >&2; exit 1; }

RPC=$(jq -r '.chains[] | select(.name=="starknet-1") | .rpc' "$MANIFEST")
USDC=$(jq -r '.chains[] | select(.name=="starknet-1") | .tokens[] | select(.symbol=="USDC") | .address' "$MANIFEST")
ACCT=$(jq -r '.chains[] | select(.name=="starknet-1") | .accounts[1].address' "$MANIFEST")

# Cairo entry points are snake_case; the felt selector is the 250-bit-masked
# keccak of the name. Derive it yourself with:  cast keccak balance_of
BALANCE_OF="0x035a73cd311a05d46deda634c5ee045db92f811b4e74bca4437fcb5302b7af33"

echo "RPC:     $RPC"
echo "USDC:    $USDC"
echo "account: $ACCT"

# 1) Fund 100 USDC via a signed `mint` invoke from a predeployed dev account
#    (the recipient needs no key). Amount is whole units, scaled by 6 decimals.
echo
echo "==> faucet: mint 100 USDC to the account"
wharfnet faucet starknet-1 "$ACCT" 100 --token USDC

# 2) Read balance_of(account) with starknet_call. An ERC-20 balance is a u256,
#    returned as two felts [low, high]; recombine them into one integer.
echo
echo "==> starknet_call balance_of"
RESULT=$(curl -s "$RPC" -X POST -H 'content-type: application/json' -d "{
  \"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"starknet_call\",
  \"params\":[
    {\"contract_address\":\"$USDC\",\"entry_point_selector\":\"$BALANCE_OF\",\"calldata\":[\"$ACCT\"]},
    \"latest\"
  ]}")

echo "$RESULT" | jq -e '.result' >/dev/null 2>&1 || { echo "call failed: $RESULT" >&2; exit 1; }
# Recombine the [low, high] felts into one integer (base units, 6 decimals).
echo "$RESULT" | python3 -c \
  "import sys,json; a=json.load(sys.stdin)['result']; print('USDC balance (base units):', int(a[0],16)+(int(a[1],16)<<128))"

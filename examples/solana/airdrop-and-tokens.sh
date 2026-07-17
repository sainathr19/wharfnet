#!/usr/bin/env bash
# Airdrop SOL and top up an SPL token from the faucet, then read both balances
# over the standard Solana JSON-RPC. Assumes `wharfnet up` is already running.
#
# Needs: docker (wharfnet), jq, curl. No Solana toolchain required — this uses
# raw JSON-RPC, exactly what @solana/web3.js does under the hood.
set -euo pipefail

MANIFEST=".wharfnet/wharfnet.json"
[ -f "$MANIFEST" ] || { echo "No manifest at $MANIFEST — run 'wharfnet up' first." >&2; exit 1; }

RPC=$(jq -r '.chains[] | select(.name=="solana-1") | .rpc' "$MANIFEST")
USDC=$(jq -r '.chains[] | select(.name=="solana-1") | .tokens[] | select(.symbol=="USDC") | .address' "$MANIFEST")
# Use the second dev account as our "test wallet" recipient.
WALLET=$(jq -r '.chains[] | select(.name=="solana-1") | .accounts[1].address' "$MANIFEST")

echo "RPC:    $RPC"
echo "USDC:   $USDC"
echo "wallet: $WALLET"

rpc() { # rpc METHOD JSON_PARAMS
  curl -s "$RPC" -X POST -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":$2}"
}

# 1) Fund the wallet: 5 SOL plus a top-up of every SPL test token. SOL goes
#    through requestAirdrop; SPL tokens through a surfpool cheat (no key needed).
echo
echo "==> faucet: 5 SOL + SPL tokens to the wallet"
wharfnet faucet solana-1 "$WALLET" 5

# 2) Read the native SOL balance (lamports; 1 SOL = 1e9 lamports).
echo
LAMPORTS=$(rpc getBalance "[\"$WALLET\"]" | jq -r '.result.value')
echo "SOL balance: $LAMPORTS lamports"

# 3) Read the USDC token balance for this owner + mint (jsonParsed gives a
#    human-readable uiAmountString).
echo
echo "==> getTokenAccountsByOwner (USDC)"
rpc getTokenAccountsByOwner \
  "[\"$WALLET\", {\"mint\":\"$USDC\"}, {\"encoding\":\"jsonParsed\"}]" \
  | jq -r '.result.value[0].account.data.parsed.info.tokenAmount.uiAmountString
           | "USDC balance: " + (. // "0")'

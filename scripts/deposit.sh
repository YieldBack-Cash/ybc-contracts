#!/bin/bash
set -e

SOURCE="user"
NETWORK="testnet"

VAULT_ADDR="${1:?Usage: ./deposit.sh <VAULT_ADDRESS> <YM_ADDRESS> <AMOUNT>}"
YM_ADDR="${2:?Usage: ./deposit.sh <VAULT_ADDRESS> <YM_ADDRESS> <AMOUNT>}"
AMOUNT="${3:?Usage: ./deposit.sh <VAULT_ADDRESS> <YM_ADDRESS> <AMOUNT>}"

LEDGER=$(( $(curl -s "https://horizon-testnet.stellar.org/ledgers?order=desc&limit=1" \
  | jq -r '._embedded.records[0].sequence') + 1000000 ))

echo "Approving YM to spend vault shares"
stellar contract invoke \
  --id "$VAULT_ADDR" \
  --source-account "$SOURCE" --network "$NETWORK" \
  -- approve --owner "$SOURCE" \
  --spender "$YM_ADDR" \
  --amount "$AMOUNT" --live_until_ledger "$LEDGER" 2>/dev/null

echo "Depositing $AMOUNT into YM"
stellar contract invoke \
  --id "$YM_ADDR" \
  --source-account "$SOURCE" --network "$NETWORK" \
  -- deposit --from "$SOURCE" --shares_amount "$AMOUNT" 2>/dev/null

PT_ADDR=$(stellar contract invoke --id "$YM_ADDR" --source-account "$SOURCE" --network "$NETWORK" -- get_principal_token 2>/dev/null | tr -d '"')
YT_ADDR=$(stellar contract invoke --id "$YM_ADDR" --source-account "$SOURCE" --network "$NETWORK" -- get_yield_token 2>/dev/null | tr -d '"')

PT_BAL=$(stellar contract invoke --id "$PT_ADDR" --source-account "$SOURCE" --network "$NETWORK" --send=no -- balance --id "$SOURCE" 2>/dev/null)
YT_BAL=$(stellar contract invoke --id "$YT_ADDR" --source-account "$SOURCE" --network "$NETWORK" --send=no -- balance --id "$SOURCE" 2>/dev/null)

echo "PT balance: $PT_BAL"
echo "YT balance: $YT_BAL"

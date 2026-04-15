#!/bin/bash
set -e

SOURCE="user"
NETWORK="testnet"

VAULT_ADDR="${1:?Usage: ./bump_rate.sh <VAULT_ADDRESS> <YM_ADDRESS>}"
YM_ADDR="${2:?Usage: ./bump_rate.sh <VAULT_ADDRESS> <YM_ADDRESS>}"

CURRENT_RATE=$(stellar contract invoke --id "$YM_ADDR" --source-account "$SOURCE" --network "$NETWORK" --send=no -- get_exchange_rate 2>/dev/null | tr -d '"')
NEW_RATE=$(( CURRENT_RATE + CURRENT_RATE / 10 ))

echo "Exchange rate: $CURRENT_RATE -> $NEW_RATE (+10%)"

stellar contract invoke \
  --id "$VAULT_ADDR" \
  --source-account "$SOURCE" --network "$NETWORK" \
  -- set_exchange_rate --rate "$NEW_RATE" 2>/dev/null

if [[ "${3}" == "--claim" || "${3}" == "--accrue" ]]; then
  YT_ADDR=$(stellar contract invoke --id "$YM_ADDR" --source-account "$SOURCE" --network "$NETWORK" -- get_yield_token 2>/dev/null | tr -d '"')

  if [[ "${3}" == "--claim" ]]; then
    echo "Claiming yield"
    CLAIMED=$(stellar contract invoke \
      --id "$YT_ADDR" \
      --source-account "$SOURCE" --network "$NETWORK" \
      -- claim_yield --user "$SOURCE" 2>/dev/null)
    echo "Yield claimed: $CLAIMED vault shares"
  else
    echo "Accruing yield"
    stellar contract invoke \
      --id "$YT_ADDR" \
      --source-account "$SOURCE" --network "$NETWORK" \
      -- transfer --from "$SOURCE" --to_muxed "$SOURCE" --amount 0 2>/dev/null
    echo "Yield accrued (not claimed)"
  fi
fi
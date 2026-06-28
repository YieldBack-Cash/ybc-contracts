#!/bin/bash
set -e

SOURCE="admin"
NETWORK="testnet"
PROJECT_DIR="$HOME/computerscience/blockchain/ybc-contracts"

POOL_ADDR="CCEBVDYM32YNYCVNRXQKDFFPISJJCV557CDZEIRBEE4NCV4KHPQ44HGF"
UNDERLYING_ADDR="CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC"

ADMIN_ADDR=$(stellar keys address "$SOURCE")

echo "Uploading fee vault WASM"
FEE_VAULT_HASH=$(stellar contract upload \
  --wasm "$PROJECT_DIR/wasms/fee_vault_v2.wasm" \
  --source-account "$SOURCE" \
  --network "$NETWORK")

echo "Deploying fee vault"
FEE_VAULT_ADDR=$(stellar contract deploy \
  --wasm-hash "$FEE_VAULT_HASH" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- \
  --admin "$ADMIN_ADDR" \
  --pool "$POOL_ADDR" \
  --underlying "$UNDERLYING_ADDR")

echo ""
echo "Admin:     $ADMIN_ADDR"
echo "Pool:      $POOL_ADDR"
echo "Underlying: $UNDERLYING_ADDR"
echo "Fee Vault: $FEE_VAULT_ADDR"
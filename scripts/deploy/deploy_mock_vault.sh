#!/bin/bash
set -e

SOURCE="user"
NETWORK="testnet"
PROJECT_DIR="$HOME/computerscience/blockchain/ybc-contracts"
WASM_DIR="$PROJECT_DIR/target/wasm32v1-none/release"

echo "Uploading mock vault WASM"
VAULT_HASH=$(stellar contract upload \
  --wasm "$WASM_DIR/mock_vault.wasm" \
  --source-account "$SOURCE" \
  --network "$NETWORK")

echo "Deploying mock vault"
VAULT_ADDR=$(stellar contract deploy \
  --wasm-hash "$VAULT_HASH" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- \
  --admin "$SOURCE" \
  --name "Mock Vault" \
  --symbol "mVAULT" \
  --decimals 7)

echo "Vault: $VAULT_ADDR"
#!/bin/bash
set -e

SOURCE="admin"
NETWORK="testnet"
PROJECT_DIR="$HOME/computerscience/blockchain/ybc-contracts"
WASM_DIR="$PROJECT_DIR/target/wasm32v1-none/release"

ADMIN_ADDR=$(stellar keys address "$SOURCE")

echo "Building WASMs"
cargo build \
  --manifest-path "$PROJECT_DIR/Cargo.toml" \
  --target wasm32v1-none \
  --release \
  -p principal_token \
  -p yield_token \
  -p yield_manager \
  -p amm \
  -p factory

echo "Uploading WASMs"
PT_HASH=$(stellar contract upload \
  --wasm "$WASM_DIR/principal_token.wasm" \
  --source-account "$SOURCE" \
  --network "$NETWORK")

YT_HASH=$(stellar contract upload \
  --wasm "$WASM_DIR/yield_token.wasm" \
  --source-account "$SOURCE" \
  --network "$NETWORK")

YM_HASH=$(stellar contract upload \
  --wasm "$WASM_DIR/yield_manager.wasm" \
  --source-account "$SOURCE" \
  --network "$NETWORK")

AMM_HASH=$(stellar contract upload \
  --wasm "$WASM_DIR/amm.wasm" \
  --source-account "$SOURCE" \
  --network "$NETWORK")

FACTORY_HASH=$(stellar contract upload \
  --wasm "$WASM_DIR/factory.wasm" \
  --source-account "$SOURCE" \
  --network "$NETWORK")

echo "Deploying factory"
FACTORY_ADDR=$(stellar contract deploy \
  --wasm-hash "$FACTORY_HASH" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- \
  --admin "$ADMIN_ADDR" \
  --wasm_hashes "{ \"pt\": \"$PT_HASH\", \"yt\": \"$YT_HASH\", \"ym\": \"$YM_HASH\", \"amm\": \"$AMM_HASH\" }")

echo ""
echo "Admin:    $ADMIN_ADDR"
echo "Factory:  $FACTORY_ADDR"
echo "PT hash:  $PT_HASH"
echo "YT hash:  $YT_HASH"
echo "YM hash:  $YM_HASH"
echo "AMM hash: $AMM_HASH"
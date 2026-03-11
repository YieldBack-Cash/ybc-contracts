#!/bin/bash
set -e

SOURCE="user"
NETWORK="testnet"
PROJECT_DIR="$HOME/compsci/blockchain/ybc"
WASM_DIR="$PROJECT_DIR/target/wasm32v1-none/release"

echo "Uploading WASMs"
PT_HASH=$(stellar contract upload --wasm "$WASM_DIR/principal_token.wasm" --source-account "$SOURCE" --network "$NETWORK" 2>/dev/null)
YT_HASH=$(stellar contract upload --wasm "$WASM_DIR/yield_token.wasm" --source-account "$SOURCE" --network "$NETWORK" 2>/dev/null)
YM_HASH=$(stellar contract upload --wasm "$WASM_DIR/yield_manager.wasm" --source-account "$SOURCE" --network "$NETWORK" 2>/dev/null)
AMM_HASH=$(stellar contract upload --wasm "$PROJECT_DIR/wasms/comet_v1.0.0.wasm" --source-account "$SOURCE" --network "$NETWORK" 2>/dev/null)
FACTORY_HASH=$(stellar contract upload --wasm "$WASM_DIR/factory.wasm" --source-account "$SOURCE" --network "$NETWORK" 2>/dev/null)

echo "Deploying factory"
FACTORY_ADDR=$(stellar contract deploy \
  --wasm-hash "$FACTORY_HASH" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- \
  --admin "$SOURCE" \
  --wasm_hashes "{\"pt\": \"$PT_HASH\", \"yt\": \"$YT_HASH\", \"ym\": \"$YM_HASH\", \"amm\": \"$AMM_HASH\"}" 2>/dev/null)


VAULT_ADDR="${1:?Usage: ./deploy.sh <VAULT_ADDRESS>}"
MATURITY=$(( $(date +%s) + 180 )) # 3 minutes from now

echo "Deploying Tokens/Escrow (maturity: $MATURITY)"
YM_ADDR=$(stellar contract invoke \
  --id "$FACTORY_ADDR" \
  --source-account "$SOURCE" \
  --network "$NETWORK" \
  -- \
  deploy_yield_manager \
  --vault "$VAULT_ADDR" \
  --vault_type 0 \
  --maturity "$MATURITY" 2>/dev/null)

PT_ADDR=$(stellar contract invoke --id "$FACTORY_ADDR" --source-account "$SOURCE" --network "$NETWORK" -- get_current_pt_token 2>/dev/null)
YT_ADDR=$(stellar contract invoke --id "$FACTORY_ADDR" --source-account "$SOURCE" --network "$NETWORK" -- get_current_yt_token 2>/dev/null)

echo "Factory:       $FACTORY_ADDR"
echo "Vault:         $VAULT_ADDR"
echo "Yield Manager: $YM_ADDR"
echo "PT:            $PT_ADDR"
echo "YT:            $YT_ADDR"
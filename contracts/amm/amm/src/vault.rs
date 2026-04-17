use crate::storage::{get_market_state, get_balance_b};
use soroban_sdk::Env;
use vault_interface::VaultContractClient;

/// Returns the value of `shares` vault shares denominated in the underlying asset.
pub(crate) fn convert_vault_shares_to_assets(e: &Env, shares: i128) -> i128 {
    let market = get_market_state(e);
    let client = VaultContractClient::new(e, &market.token_b);
    client.convert_to_assets(&shares)
}

/// Returns the number of vault shares equivalent to `assets` units of the underlying asset.
/// Uses the per-share rate from `convert_to_assets(1)` and performs integer division.
pub(crate) fn convert_assets_to_vault_shares(e: &Env, assets: i128) -> i128 {
    let market = get_market_state(e);
    let client = VaultContractClient::new(e, &market.token_b);
    let rate = client.convert_to_assets(&1i128);
    assets / rate
}

/// Returns the current vault share balance of the pool converted to underlying asset units.
pub(crate) fn get_asset_balance_b(e: &Env) -> i128 {
    convert_vault_shares_to_assets(e, get_balance_b(e))
}
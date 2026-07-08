use crate::math::FP_SCALE;
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
/// Probes the rate with `FP_SCALE` shares (rather than 1) so integer division in
/// `convert_to_assets` doesn't truncate the rate to zero for sub-unity exchange rates.
pub(crate) fn convert_assets_to_vault_shares(e: &Env, assets: i128) -> i128 {
    let market = get_market_state(e);
    let client = VaultContractClient::new(e, &market.token_b);
    let probe_assets = client.convert_to_assets(&FP_SCALE);
    assets * FP_SCALE / probe_assets
}

/// Returns the current vault share balance of the pool converted to underlying asset units.
pub(crate) fn get_asset_balance_b(e: &Env) -> i128 {
    convert_vault_shares_to_assets(e, get_balance_b(e))
}
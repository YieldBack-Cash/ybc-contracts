use crate::math::FP_SCALE;
use crate::storage::get_ym;
use amm_interface::AmmError;
use soroban_sdk::Env;
use yield_manager_interface::YieldManagerClient;

/// The share/asset rate the pool prices against, read once per invocation.
///
/// Sourced from `YieldManager::get_exchange_rate`, not from the vault: PT
/// settles at the YM's high-water-marked rate, and the two diverge when a
/// vault loses value. Pricing against the vault's own rate would then value PT
/// above what the YM will pay out, for anyone willing to mint it
/// (`tests/integration/src/tests/rate_divergence.rs`).
///
/// Read once because pre-maturity the YM forwards to the vault, the most
/// expensive call in a swap, and the rate cannot change mid-transaction. Past
/// maturity the YM answers from its locked storage without touching the vault.
///
/// The name predates sourcing the rate from the YM.
pub(crate) struct VaultRate {
    /// Assets per `FP_SCALE` shares. `get_exchange_rate` is defined as assets
    /// per `SCALAR_7` shares and both scales are 1e7, so no rescaling.
    assets_per_scale: i128,
}

impl VaultRate {
    pub(crate) fn load(e: &Env) -> Result<Self, AmmError> {
        let assets_per_scale = YieldManagerClient::new(e, &get_ym(e)).get_exchange_rate();
        if assets_per_scale <= 0 {
            return Err(AmmError::ZeroExchangeRate);
        }
        Ok(VaultRate { assets_per_scale })
    }

    /// Value of `shares` vault shares in the underlying. Floors. Uses the
    /// cached rate so it is the exact inverse of `to_shares`.
    pub(crate) fn to_assets(&self, shares: i128) -> Result<i128, AmmError> {
        Ok(shares
            .checked_mul(self.assets_per_scale)
            .ok_or(AmmError::MathOverflow)?
            / FP_SCALE)
    }

    /// The raw rate, handed to flash-swap receivers so they need not re-read it.
    pub(crate) fn assets_per_scale(&self) -> i128 {
        self.assets_per_scale
    }

    /// Vault shares equivalent to `assets` units of the underlying. Floors.
    pub(crate) fn to_shares(&self, assets: i128) -> Result<i128, AmmError> {
        Ok(assets.checked_mul(FP_SCALE).ok_or(AmmError::MathOverflow)? / self.assets_per_scale)
    }
}

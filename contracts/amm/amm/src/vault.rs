use crate::math::FP_SCALE;
use crate::storage::get_market_state;
use soroban_sdk::Env;
use vault_interface::VaultContractClient;

/// The vault's share/asset exchange rate, read once and reused for every
/// conversion in an invocation.
///
/// This exists for cost, and the cost is not small. Each `convert_to_assets`
/// is a cross-contract call, and against a lending vault it is an expensive
/// one — Blend's accrues interest and reads the underlying pool's reserve to
/// answer. Every swap used to make four such calls (reserve pricing before the
/// trade, the traded amount, the fee cut, and reserve pricing after), which is
/// what pushed the flash-swap zaps past the per-transaction budget on testnet.
/// Loading once takes that to one.
///
/// The rate cannot move mid-invocation anyway, so caching changes no result.
pub(crate) struct VaultRate {
    /// Assets per `FP_SCALE` shares. Probed with `FP_SCALE` rather than 1 so
    /// integer division inside the vault doesn't truncate the rate to zero for
    /// sub-unity exchange rates.
    assets_per_scale: i128,
}

impl VaultRate {
    pub(crate) fn load(e: &Env) -> Self {
        let market = get_market_state(e);
        let client = VaultContractClient::new(e, &market.token_b);
        let assets_per_scale = client.convert_to_assets(&FP_SCALE);
        assert!(assets_per_scale > 0, "vault reported a zero exchange rate");
        VaultRate { assets_per_scale }
    }

    /// Value of `shares` vault shares, denominated in the underlying asset.
    ///
    /// Derived from the probe rather than asking the vault to convert this
    /// exact amount, so it stays the arithmetic inverse of `to_shares` — the
    /// two are now mutually consistent, which the previous mix of a direct call
    /// and a probe-derived division was not.
    pub(crate) fn to_assets(&self, shares: i128) -> i128 {
        shares
            .checked_mul(self.assets_per_scale)
            .expect("overflow converting shares to assets")
            / FP_SCALE
    }

    /// Vault shares equivalent to `assets` units of the underlying.
    pub(crate) fn to_shares(&self, assets: i128) -> i128 {
        assets
            .checked_mul(FP_SCALE)
            .expect("overflow converting assets to shares")
            / self.assets_per_scale
    }
}

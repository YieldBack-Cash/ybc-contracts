use crate::math::FP_SCALE;
use crate::storage::get_ym;
use soroban_sdk::Env;
use yield_manager_interface::YieldManagerClient;

/// The share/asset exchange rate this pool prices against, read once and reused
/// for every conversion in an invocation.
///
/// # Why the yield manager and not the vault
///
/// This used to call `convert_to_assets` on the vault directly. That was wrong,
/// and not merely stale: PT is a claim on `face / rate` vault shares where
/// `rate` is the YIELD MANAGER's rate, because that is what `redeem_principal`
/// settles at. The vault's own rate never enters PT's payout.
///
/// The two numbers are equal for as long as the vault only appreciates, which
/// is what hid the difference. They separate the moment a vault loses value:
/// the YM high-water-marks (`update_exchange_rate` keeps the stored value when
/// the vault reports lower) while a direct vault read follows it down. The pool
/// then valued PT face at the depressed rate while the YM would only ever
/// redeem it at the high-water mark — pricing PT above what it can pay out, for
/// anyone holding it, in unlimited size for anyone willing to mint it. See
/// `tests/integration/src/tests/rate_divergence.rs`.
///
/// Reading the YM makes the pool price PT against the number that will actually
/// settle it, in every vault regime, which is the property that was missing.
///
/// # Cost
///
/// Each load is a cross-contract call, and pre-maturity the YM turns it into a
/// vault read — against a lending vault an expensive one, since Blend's accrues
/// interest and reads the underlying pool's reserve to answer. Every swap used
/// to make four such calls (reserve pricing before the trade, the traded amount,
/// the fee cut, and reserve pricing after), which is what pushed the flash-swap
/// zaps past the per-transaction budget on testnet. Loading once takes that to
/// one, and that count is unchanged here: the underlying vault is still read
/// exactly once per invocation, just through the YM rather than directly.
///
/// Measured on testnet against the live Blend vault, steady state:
///
/// ```text
///   vault.convert_to_assets   8,124 stroops   (what this used to call)
///   ym.get_exchange_rate     12,564 stroops   (what it calls now)
/// ```
///
/// So ~4,440 stroops per swap for the extra frame plus the YM's instance write —
/// the rate ratchets on nearly every ledger against a live lending vault, so that
/// write is the steady-state case, not the exception. Both figures exclude the
/// periodic TTL rent bump, which the first call after a quiet spell pays either
/// way (14,990,731 stroops when measured); routing swaps through the YM now keeps
/// its instance alive as a side effect, which `storage.rs` wants anyway — an
/// expired YM instance bricks the whole market.
///
/// Past maturity it is strictly cheaper than it was: the YM's rate is locked, so
/// it answers from its own storage without touching the vault at all.
///
/// The rate cannot move mid-invocation anyway, so caching changes no result.
pub(crate) struct VaultRate {
    /// Assets per `FP_SCALE` shares.
    ///
    /// `get_exchange_rate` is defined as assets per `SCALAR_7` shares and the
    /// YM's `SCALAR_7` is this module's `FP_SCALE` — both 1e7 — so the figure
    /// needs no rescaling.
    assets_per_scale: i128,
}

impl VaultRate {
    pub(crate) fn load(e: &Env) -> Self {
        let assets_per_scale = YieldManagerClient::new(e, &get_ym(e)).get_exchange_rate();
        assert!(
            assets_per_scale > 0,
            "yield manager reported a zero exchange rate"
        );
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

    /// The rate itself: assets per `FP_SCALE` shares.
    ///
    /// Exposed so a flash swap can hand the figure to its receiver rather than
    /// leave it to repeat this read. The receiver is the yield manager, which is
    /// where the figure came from — so it is now the YM's own rate making a round
    /// trip back to it, and the callback can use it as-is. That makes it a pure
    /// saving: without it the callback would re-enter the YM's rate path and,
    /// pre-maturity, pay for a second vault read returning the same number.
    pub(crate) fn assets_per_scale(&self) -> i128 {
        self.assets_per_scale
    }

    /// Vault shares equivalent to `assets` units of the underlying.
    pub(crate) fn to_shares(&self, assets: i128) -> i128 {
        assets
            .checked_mul(FP_SCALE)
            .expect("overflow converting assets to shares")
            / self.assets_per_scale
    }
}

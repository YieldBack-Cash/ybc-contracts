use soroban_sdk::{contractevent, Address};

#[contractevent(topics = ["routed_yt_buy"], data_format = "vec")]
pub struct RoutedYtBuy {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub yt_out: i128,
    pub max_v_in: i128,
}

#[contractevent(topics = ["routed_yt_sell"], data_format = "vec")]
pub struct RoutedYtSell {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub yt_in: i128,
    pub min_v_out: i128,
}

/// Underlying asset entered the protocol: `asset_in` was deposited into the
/// vault and became `shares_out` vault shares. Emitted at the vault boundary
/// itself rather than per-zap, so an indexer sees every asset inflow through one
/// event no matter which zap produced it. The leg that consumes those shares
/// (AMM swap, YM mint) publishes its own event as usual.
#[contractevent(topics = ["zap_in"], data_format = "vec")]
pub struct ZappedIn {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub asset: Address,
    pub asset_in: i128,
    pub shares_out: i128,
}

/// Mirror of [`ZappedIn`]: `shares_in` vault shares were redeemed back into
/// `asset_out` of the underlying and paid to the user.
#[contractevent(topics = ["zap_out"], data_format = "vec")]
pub struct ZappedOut {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub asset: Address,
    pub shares_in: i128,
    pub asset_out: i128,
}

/// Base-asset counterpart of [`ExitedExpired`]. Carries only what the router
/// itself knows: it no longer measures the PT burned or the shares redeemed,
/// because the yield manager now settles both in one call and reports them in
/// its own `RedeemToAsset` event.
#[contractevent(topics = ["exit_expired_to_asset"], data_format = "vec")]
pub struct ExitedExpiredToAsset {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub lp_shares: i128,
    pub asset_out: i128,
}

#[contractevent(topics = ["exit_expired"], data_format = "vec")]
pub struct ExitedExpired {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub lp_shares: i128,
    pub pt_redeemed: i128,
    pub shares_out: i128,
}
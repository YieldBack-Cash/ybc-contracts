//! One event per user-facing action, named after the entrypoint that emits it.
//!
//! A reader should be able to understand what a user did from a single record,
//! without knowing how the router is built. So every event here is published
//! once, by the entrypoint the user called, and reports what actually moved:
//! asset amounts are what the user paid or received, measured by the router
//! net of every refund and sweep, or reported back by the yield manager when
//! it settles the whole leg; token amounts are what the user asked for and
//! received.
//! Bounds the user set (`max_*`, `min_*`) are not repeated; they are already in
//! the transaction's arguments. The legs underneath (pool trades, yield-manager
//! mints and burns, vault deposits and redemptions) publish their own events
//! from their own contracts, and nobody has to read them to follow the story.
//!
//! Every event carries `vault` and `to` as topics and `maturity` as its first
//! data field, so an indexer can file it under the market without joining on
//! the transaction. Data is positional (`data_format = "vec"`); appending a
//! field changes the layout, and the indexer's generated layouts must be
//! regenerated with it.
//!
//! The thin wrappers (`swap_v_for_pt`, `swap_pt_for_v`, `deposit`, `withdraw`,
//! `split`, `recombine`) and the views emit nothing of their own: each wrapper
//! is a single call into a contract whose event already says everything.

use soroban_sdk::{contractevent, Address};

// ── Share-denominated YT trades ─────────────────────────────────────────────

/// `swap_v_for_yt`: bought `yt_out` YT for `v_in` vault shares.
#[contractevent(topics = ["swap_v_for_yt"], data_format = "vec")]
pub struct SwapVForYt {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub v_in: i128,
    pub yt_out: i128,
}

/// `swap_yt_for_v`: sold `yt_in` YT for `v_out` vault shares.
#[contractevent(topics = ["swap_yt_for_v"], data_format = "vec")]
pub struct SwapYtForV {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub yt_in: i128,
    pub v_out: i128,
}

// ── Zaps: in and out of a market holding only the base asset ────────────────

/// `zap_asset_for_pt`: paid `asset_in` of `asset` for `pt_out` PT.
#[contractevent(topics = ["zap_asset_for_pt"], data_format = "vec")]
pub struct ZapAssetForPt {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub asset_in: i128,
    pub pt_out: i128,
}

/// `zap_pt_for_asset`: sold `pt_in` PT for `asset_out` of `asset`.
#[contractevent(topics = ["zap_pt_for_asset"], data_format = "vec")]
pub struct ZapPtForAsset {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub pt_in: i128,
    pub asset_out: i128,
}

/// `zap_asset_for_yt`: paid `asset_in` of `asset` for `yt_out` YT.
#[contractevent(topics = ["zap_asset_for_yt"], data_format = "vec")]
pub struct ZapAssetForYt {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub asset_in: i128,
    pub yt_out: i128,
}

/// `zap_yt_for_asset`: sold `yt_in` YT for `asset_out` of `asset`.
#[contractevent(topics = ["zap_yt_for_asset"], data_format = "vec")]
pub struct ZapYtForAsset {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub yt_in: i128,
    pub asset_out: i128,
}

/// `zap_asset_for_split`: turned `asset_in` of `asset` into `tokens_out` PT
/// and the same amount of YT.
#[contractevent(topics = ["zap_asset_for_split"], data_format = "vec")]
pub struct ZapAssetForSplit {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub asset_in: i128,
    pub tokens_out: i128,
}

/// `zap_split_for_asset`: burned `tokens_in` PT and the same amount of YT for
/// `asset_out` of `asset`.
#[contractevent(topics = ["zap_split_for_asset"], data_format = "vec")]
pub struct ZapSplitForAsset {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub tokens_in: i128,
    pub asset_out: i128,
}

/// `zap_asset_for_lp`: paid `asset_in` of `asset` for `lp_out` LP shares,
/// buying `pt_bought` PT along the way for the pool's PT leg.
#[contractevent(topics = ["zap_asset_for_lp"], data_format = "vec")]
pub struct ZapAssetForLp {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub asset_in: i128,
    pub pt_bought: i128,
    pub lp_out: i128,
}

/// `zap_lp_for_asset`: burned `lp_in` LP shares, sold `pt_sold` of the PT leg
/// back into the pool, and received `asset_out` of `asset`.
#[contractevent(topics = ["zap_lp_for_asset"], data_format = "vec")]
pub struct ZapLpForAsset {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub lp_in: i128,
    pub pt_sold: i128,
    pub asset_out: i128,
}

// ── Expired-market exits ────────────────────────────────────────────────────

/// `exit_expired`: burned `lp_shares` LP shares, redeemed `pt_redeemed` PT and
/// claimed the YT's yield, all paid as `shares_out` vault shares.
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

/// `exit_expired_to_asset`: the base-asset counterpart of [`ExitedExpired`].
/// Carries only what the router itself knows: the yield manager settles the PT
/// and the shares in one call and reports both in its own `RedeemToAsset`
/// event.
#[contractevent(topics = ["exit_expired_to_asset"], data_format = "vec")]
pub struct ExitedExpiredToAsset {
    #[topic]
    pub vault: Address,
    #[topic]
    pub to: Address,
    pub maturity: u64,
    pub asset: Address,
    pub lp_shares: i128,
    pub asset_out: i128,
}

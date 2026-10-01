//! Amounts named `amount`, `mint_amount`, `pt_amount`, `burned`, `yt_out` and
//! `pt_borrowed` are PT/YT in asset units (7 decimals); everything named
//! `shares_*`, `v_*` or `user_cost` is vault shares. `exchange_rate` is assets
//! per `SCALAR_7` shares: the current rate before maturity; after it, the live
//! rate in `RedeemPrincipal` / `RedeemToAsset` and the locked rate in
//! `DistributeYield`.

use soroban_sdk::{contractevent, Address};

/// `set_token_contracts` ran.
#[contractevent(topics = ["token_contracts_set"])]
pub struct TokenContractsSet {
    #[topic]
    pub pt: Address,
    #[topic]
    pub yt: Address,
}

/// `set_pool` ran.
#[contractevent(topics = ["pool_set"])]
pub struct PoolSet {
    #[topic]
    pub pool: Address,
}

/// `deposit`: `shares_amount` vault shares became `mint_amount` PT and YT each.
#[contractevent(topics = ["deposit"], data_format = "vec")]
pub struct Deposit {
    #[topic]
    pub from: Address,
    pub shares_amount: i128,
    pub mint_amount: i128,
    pub exchange_rate: i128,
}

/// `redeem_combined`: `amount` PT and YT each became `shares_returned` vault shares.
#[contractevent(topics = ["redeem_combined"], data_format = "vec")]
pub struct RedeemCombined {
    #[topic]
    pub from: Address,
    pub amount: i128,
    pub shares_returned: i128,
    pub exchange_rate: i128,
}

/// `redeem_principal`: `pt_amount` PT redeemed at face value for `shares_returned`.
#[contractevent(topics = ["redeem_principal"], data_format = "vec")]
pub struct RedeemPrincipal {
    #[topic]
    pub from: Address,
    pub pt_amount: i128,
    pub shares_returned: i128,
    pub exchange_rate: i128,
}

/// `distribute_yield`: `shares_amount` vault shares actually paid to a YT holder.
#[contractevent(topics = ["distribute_yield"], data_format = "vec")]
pub struct DistributeYield {
    #[topic]
    pub to: Address,
    pub shares_amount: i128,
    pub exchange_rate: i128,
}

/// `on_flash_receive_pt` (a YT buy): `yt_out` YT minted to `user` for `user_cost`
/// shares, the pool having advanced the rest of `v_to_mint`.
#[contractevent(topics = ["flash_deposit"], data_format = "vec")]
pub struct FlashDeposit {
    #[topic]
    pub user: Address,
    #[topic]
    pub amm: Address,
    pub yt_out: i128,
    pub v_to_mint: i128,
    pub user_cost: i128,
    pub exchange_rate: i128,
}

/// `on_flash_receive_v` (a YT sell): `pt_borrowed` PT and as much YT burned;
/// `v_owed` shares repaid to the pool and `v_to_user` paid out.
#[contractevent(topics = ["flash_redeem"], data_format = "vec")]
pub struct FlashRedeem {
    #[topic]
    pub user: Address,
    #[topic]
    pub amm: Address,
    pub pt_borrowed: i128,
    pub v_owed: i128,
    pub v_to_user: i128,
    pub exchange_rate: i128,
}

/// `collect_surplus`: `amount` vault shares swept to the treasury.
#[contractevent(topics = ["surplus_collected"], data_format = "vec")]
pub struct SurplusCollected {
    #[topic]
    pub treasury: Address,
    pub amount: i128,
}

/// Base asset entered the market through the YM: `asset_in` became
/// `shares_in` vault shares (custodied by the YM) and minted `mint_amount`
/// of PT and YT each.
#[contractevent(topics = ["deposit_asset"], data_format = "vec")]
pub struct DepositAsset {
    #[topic]
    pub from: Address,
    pub asset_in: i128,
    pub shares_in: i128,
    pub mint_amount: i128,
    pub exchange_rate: i128,
}

/// A redemption paid in the base asset: `redeem_combined_to_asset` before
/// maturity, or `exit_expired_to_asset` after it, where `shares_redeemed` also
/// includes loose shares the caller handed in (so `burned` can be 0). `burned`
/// PT and `shares_redeemed` of custody became `asset_out`.
#[contractevent(topics = ["redeem_to_asset"], data_format = "vec")]
pub struct RedeemToAsset {
    #[topic]
    pub from: Address,
    pub burned: i128,
    pub shares_redeemed: i128,
    pub asset_out: i128,
    pub exchange_rate: i128,
}

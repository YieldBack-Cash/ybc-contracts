#![no_std]

use soroban_sdk::{contractclient, contracterror, Address, Env};

/// Every way a pool call can fail on its own checks.
///
/// Lives in the interface crate, like `YieldManagerError`, so callers holding
/// only an `AmmClient` can decode it. Typed so clients see `Error(Contract, #n)`
/// rather than an opaque trap.
///
/// Codes are part of the client-facing API: append new variants, never
/// renumber existing ones.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum AmmError {
    // ── Construction ─────────────────────────────────────────────────────────
    /// `expiry_ts` is not in the future.
    ExpiryNotInFuture = 1,
    /// `apy_min` is negative, or `current_apy` is not strictly inside the band.
    InvalidApyBand = 2,
    /// `apy_max` is above the protocol cap.
    ApyMaxTooHigh = 3,
    /// The APY band is narrower than the protocol minimum.
    BandTooNarrow = 4,
    /// `fee_apy` is zero, negative or above the cap.
    FeeApyOutOfRange = 5,
    /// `reserve_fee_rate` is negative or above the cap.
    ReserveFeeRateOutOfRange = 6,

    // ── Arguments and market state ───────────────────────────────────────────
    /// An amount argument is zero or negative where that is not allowed.
    InvalidAmount = 7,
    /// The market has reached expiry; trading and deposits are closed.
    MarketExpired = 8,
    /// The pool has no liquidity to price against.
    EmptyPool = 9,
    /// The yield manager reported a zero exchange rate.
    ZeroExchangeRate = 10,

    // ── Trade pricing ────────────────────────────────────────────────────────
    /// The trade needs more PT than the pool holds.
    InsufficientPtLiquidity = 11,
    /// The trade needs more vault shares than the pool holds.
    InsufficientVLiquidity = 12,
    /// The post-trade PT proportion falls outside the curve's bounds.
    ProportionOutOfBounds = 13,
    /// The trade would push the exchange rate below 1 (PT above face value).
    ExchangeRateBelowOne = 14,
    /// The trade is too small to price: its V side rounds to zero.
    TradeTooSmall = 15,
    /// The swap costs more vault shares than `v_in_max` (`swap_v_for_pt`).
    /// `max_v_in` on the flash path is enforced by the receiver, with its own
    /// error.
    MaxVInExceeded = 16,
    /// The swap pays fewer vault shares than `min_v_out`.
    MinVOutNotMet = 17,

    // ── Flash swaps ──────────────────────────────────────────────────────────
    /// The flash-swap receiver is not the pool's trusted yield manager.
    UntrustedReceiver = 18,
    /// The flash-swap callback did not settle the exact amounts owed.
    FlashSwapNotSettled = 19,

    // ── Liquidity ────────────────────────────────────────────────────────────
    /// The deposit amounts cannot meet the caller's minimums at the pool ratio.
    DepositMinNotMet = 20,
    /// The deposit would mint no LP shares (or not enough to cover the
    /// minimum-liquidity burn on a first deposit).
    DepositTooSmall = 21,
    /// The caller holds fewer LP shares than they asked to burn.
    InsufficientShares = 22,
    /// The withdrawal pays less than `min_a` / `min_b`.
    WithdrawMinNotMet = 23,

    // ── Internal ─────────────────────────────────────────────────────────────
    /// Fixed-point arithmetic overflowed.
    MathOverflow = 24,
    /// Pool state violates an invariant the curve relies on.
    InvalidPoolState = 25,
}

#[contractclient(name = "AmmClient")]
/// The pool's public surface. Leg `a` is PT and leg `b` is vault shares ("V")
/// throughout; amounts are in each token's own units (7 decimals).
pub trait AmmInterface {
    /// Buy exactly `pt_out` PT for at most `v_in_max` vault shares from `to`.
    fn swap_v_for_pt(env: Env, to: Address, pt_out: i128, v_in_max: i128) -> Result<(), AmmError>;
    /// Sell exactly `pt_in` PT from `to` for at least `min_v_out` vault shares.
    fn swap_pt_for_v(env: Env, to: Address, pt_in: i128, min_v_out: i128) -> Result<(), AmmError>;
    /// Buy YT: the pool advances vault shares to `receiver` (the yield manager),
    /// which mints `yt_out` PT+YT, keeps the PT for the pool and gives `user` the
    /// YT for at most `max_v_in` shares.
    fn flash_swap_pt(
        env: Env,
        receiver: Address,
        yt_out: i128,
        user: Address,
        max_v_in: i128,
    ) -> Result<(), AmmError>;
    /// Sell YT: the pool lends `pt_to_borrow` PT to `receiver` (the yield
    /// manager), which burns it with the user's YT and repays the pool in
    /// shares; `user` receives at least `min_v_out` of the remainder.
    fn flash_swap_v(
        env: Env,
        receiver: Address,
        pt_to_borrow: i128,
        user: Address,
        min_v_out: i128,
    ) -> Result<(), AmmError>;
    /// Add liquidity. The pool takes the two legs in its reserve ratio (any
    /// ratio on an empty pool) and refunds the rest; each leg must reach its
    /// `min_*`.
    fn deposit(
        env: Env,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    ) -> Result<(), AmmError>;
    /// Burn `share_amount` LP shares for a pro-rata slice of both reserves.
    /// Returns `(pt, vault shares)` paid.
    fn withdraw(
        env: Env,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> Result<(i128, i128), AmmError>;
    /// `(PT reserve, vault-share reserve)`.
    fn get_reserves(env: Env) -> (i128, i128);
    /// The pool's implied rate after the last trade, ln-space, 1e7-scaled.
    fn get_implied_rate(env: Env) -> i128;
    fn get_treasury(env: Env) -> Address;
    /// Share of each trade's fee sent to the treasury, 1e7-scaled.
    fn get_reserve_fee_rate(env: Env) -> i128;
    fn balance_shares(env: Env, user: Address) -> i128;
    fn get_total_shares(env: Env) -> i128;
}

// `vault_rate` on both callbacks is the rate the pool priced this trade at:
// `YieldManager::get_exchange_rate`, assets per 1e7 shares, handed down so the
// receiver need not re-read it (pre-maturity that read reaches the vault). The
// name predates sourcing it from the YM. Receivers should validate it, and
// only the registered pool may call them.

#[contractclient(name = "FlashSwapPtReceiverClient")]
pub trait FlashSwapPtReceiver {
    /// Called by the AMM during `flash_swap_pt` (buy YT). The pool has already advanced
    /// `v_from_pool` vault shares to the receiver as payment for the PT it is buying. The
    /// receiver must mint `yt_out` (PT + YT) using that V plus the user's top-up, deliver
    /// `yt_out` YT to `user`, and return exactly `yt_out` PT to `amm` before returning.
    /// The user's total V cost must not exceed `max_v_in`.
    fn on_flash_receive_pt(
        env: Env,
        yt_out: i128,
        v_from_pool: i128,
        user: Address,
        max_v_in: i128,
        vault_rate: i128,
        amm: Address,
    );
}

#[contractclient(name = "FlashSwapVReceiverClient")]
pub trait FlashSwapVReceiver {
    /// Called by the AMM during `flash_swap_v`. The receiver is lent `pt_borrowed` PT and must,
    /// before returning, deliver `v_owed` vault shares back to `amm`. Anything beyond `v_owed`
    /// is the trade's net output.
    fn on_flash_receive_v(
        env: Env,
        pt_borrowed: i128,
        v_owed: i128,
        user: Address,
        min_v_out: i128,
        vault_rate: i128,
        amm: Address,
    );
}

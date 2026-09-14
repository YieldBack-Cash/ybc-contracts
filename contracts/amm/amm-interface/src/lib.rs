#![no_std]

use soroban_sdk::{contractclient, contracterror, Address, Env};

/// Every way a pool call can fail on its own checks.
///
/// Lives in the interface crate, like `YieldManagerError`, so callers holding
/// only an `AmmClient` can decode it. A bare `assert!` reaches a client as
/// `UnreachableCodeReached` whichever check tripped (issue #19); these surface
/// as `Error(Contract, #n)` instead.
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
    /// The swap costs more vault shares than `v_in_max` / `max_v_in`.
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
pub trait AmmInterface {
    fn swap_v_for_pt(env: Env, to: Address, pt_out: i128, v_in_max: i128) -> Result<(), AmmError>;
    fn swap_pt_for_v(env: Env, to: Address, pt_in: i128, min_v_out: i128) -> Result<(), AmmError>;
    fn flash_swap_pt(
        env: Env,
        receiver: Address,
        yt_out: i128,
        user: Address,
        max_v_in: i128,
    ) -> Result<(), AmmError>;
    fn flash_swap_v(
        env: Env,
        receiver: Address,
        pt_to_borrow: i128,
        user: Address,
        min_v_out: i128,
    ) -> Result<(), AmmError>;
    fn deposit(
        env: Env,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    ) -> Result<(), AmmError>;
    fn withdraw(
        env: Env,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> Result<(i128, i128), AmmError>;
    fn get_reserves(env: Env) -> (i128, i128);
    fn get_implied_rate(env: Env) -> i128;
    fn get_treasury(env: Env) -> Address;
    fn get_reserve_fee_rate(env: Env) -> i128;
    fn balance_shares(env: Env, user: Address) -> i128;
    fn get_total_shares(env: Env) -> i128;
}

// `vault_rate` on both callbacks below is the share/asset rate — assets per 1e7
// shares — that the AMM loaded at the top of the flash swap to price the trade, handed
// down rather than re-fetched.
//
// The NAME is historical. The AMM sources this from `YieldManager::get_exchange_rate`,
// NOT from the vault: PT settles at the yield manager's rate, and the two diverge the
// moment a vault loses value. See `amm/src/vault.rs` for the full argument.
//
// It exists purely to remove a redundant read. Pre-maturity the YM turns the call into
// a vault read, and against a lending vault that is expensive (it accrues interest and
// materialises the pool's whole reserve record); the rate cannot move mid-transaction,
// so the receiver asking again would spend that cost to recompute a value the caller
// is already holding.
//
// The direction matters. The value flows from the contract that OWNS the number the
// protocol settles at to the one that merely prices against it, never the reverse. In
// practice that makes this hint the yield manager's own stored figure round-tripping
// back to it, so applying it is idempotent and its non-decreasing floor is a no-op —
// the pool cannot supply any other value. Receivers should still treat it as an input
// to validate rather than a fact, and only the registered pool can supply one at all.

#[contractclient(name = "FlashSwapPtReceiverClient")]
pub trait FlashSwapPtReceiver {
    /// Called by the AMM during `flash_swap_pt` (buy YT). The pool has already advanced
    /// `v_from_pool` vault shares to the receiver as payment for the PT it is buying. The
    /// receiver must mint `yt_out` (PT + YT) using that V plus the user's top-up, deliver
    /// `yt_out` YT to `user`, and return exactly `yt_out` PT to `amm` before returning.
    /// The user's total V cost must not exceed `max_v_in`.
    fn on_flash_receive_pt(env: Env, yt_out: i128, v_from_pool: i128, user: Address, max_v_in: i128, vault_rate: i128, amm: Address);
}

#[contractclient(name = "FlashSwapVReceiverClient")]
pub trait FlashSwapVReceiver {
    /// Called by the AMM during `flash_swap_v`. The receiver is lent `pt_borrowed` PT and must,
    /// before returning, deliver `v_owed` vault shares back to `amm`. Anything beyond `v_owed`
    /// is the trade's net output.
    fn on_flash_receive_v(env: Env, pt_borrowed: i128, v_owed: i128, user: Address, min_v_out: i128, vault_rate: i128, amm: Address);
}
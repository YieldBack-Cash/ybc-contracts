#![no_std]

use soroban_sdk::{contractclient, contracterror, Address, Env};

/// Codes are part of the client-facing API: append new variants, never
/// renumber existing ones.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum YieldManagerError {
    /// `set_token_contracts` has not run yet.
    NotInitialized = 1,
    /// `set_token_contracts` may only run once.
    AlreadyInitialized = 2,
    /// A non-positive amount, or a negative ceiling in `exit_expired_to_asset`.
    InvalidAmount = 3,
    /// A pre-maturity operation called at or after maturity.
    MaturityReached = 4,
    /// A post-maturity operation called before maturity.
    MaturityNotReached = 5,
    /// The vault reported a non-positive exchange rate.
    ExchangeRateZero = 6,
    /// `set_pool` may only run once.
    PoolAlreadySet = 7,
    /// A returned amount fell outside the caller's bound (`min_*` / `max_v_in`).
    SlippageExceeded = 8,
    /// `deposit_asset`'s vault deposit produced no shares.
    VaultDepositFailed = 9,
    /// A YT purchase would cost the buyer nothing or less: the pool's advance
    /// already covers the whole mint, as it does for a dust amount.
    NonPositiveYtCost = 10,
    /// A YT sale redeemed fewer shares than the pool is owed for the PT it
    /// lent.
    RedeemBelowOwed = 11,
    /// The flash callback ended holding PT it had minted for the pool.
    PtNotDelivered = 12,
}

#[contractclient(name = "YieldManagerClient")]
pub trait YieldManagerTrait {
    /// Stores the immutable market parameters and seeds the exchange rate from
    /// the vault. `set_token_contracts` must follow before any entry point
    /// works.
    fn __constructor(env: Env, admin: Address, vault: Address, maturity: u64, treasury: Address);

    fn set_token_contracts(
        env: Env,
        pt_addr: Address,
        yt_addr: Address,
    ) -> Result<(), YieldManagerError>;

    /// Registers the AMM pool trusted to drive the flash-swap callbacks.
    /// One-shot: can only be set once, by the admin.
    fn set_pool(env: Env, pool: Address) -> Result<(), YieldManagerError>;
    fn get_pool(env: Env) -> Address;
    fn get_vault(env: Env) -> Address;
    fn get_principal_token(env: Env) -> Address;
    fn get_yield_token(env: Env) -> Address;
    fn get_maturity(env: Env) -> u64;
    fn get_treasury(env: Env) -> Address;

    /// Refreshes and returns the exchange rate (assets per `SCALAR_7` = 1e7
    /// shares). Reading it commits the high-water mark and, once maturity has
    /// passed, the permanent lock.
    fn get_exchange_rate(env: Env) -> i128;
    /// Takes `shares_amount` vault shares from `from` (via allowance) and mints
    /// that many asset-units of PT and YT at the current rate. Pre-maturity.
    fn deposit(env: Env, from: Address, shares_amount: i128) -> Result<(), YieldManagerError>;
    /// Burns `amount` PT + `amount` YT from `from` and returns the vault
    /// shares they are worth at the current rate. Pre-maturity.
    fn redeem_combined(env: Env, from: Address, amount: i128) -> Result<(), YieldManagerError>;

    // ── Asset-denominated entry/exit ─────────────────────────────────────────
    //
    // These exist so a user's signed authorization never contains a number the
    // chain computes. Every argument below is caller-chosen; every measured
    // quantity (shares from a vault deposit, shares owed on a redeem) is moved
    // under the YM's own authority at execution time. The YM is already the
    // market's share custodian, so no new custody is introduced.

    /// Splits `asset_amount` of the vault's underlying straight into PT + YT:
    /// deposits into the vault with the YM itself as receiver (shares never
    /// touch the user's account), then mints both tokens to `from` at the
    /// current rate. Returns the amount minted of each; fails if below
    /// `min_tokens_out`.
    fn deposit_asset(
        env: Env,
        from: Address,
        asset_amount: i128,
        min_tokens_out: i128,
    ) -> Result<i128, YieldManagerError>;

    /// Recombines `amount` of PT + YT (burned from `from`) back into the
    /// underlying: the YM redeems the owed shares from its own custody and the
    /// vault pays the asset directly to `from`. Returns the asset delivered;
    /// fails if below `min_asset_out`. Pre-maturity only, like redeem_combined.
    fn redeem_combined_to_asset(
        env: Env,
        from: Address,
        amount: i128,
        min_asset_out: i128,
    ) -> Result<i128, YieldManagerError>;

    /// Post-maturity exit paid in the underlying with one vault redemption:
    /// burns up to `max_pt` PT at face value, pulls up to `max_shares` of the
    /// caller's loose vault shares (an LP payout, a YT claim) via allowance,
    /// and redeems the total from YM custody. One redemption, not two, because
    /// a redemption is a lending-pool submission and two of them plus an LP
    /// withdrawal exceed the per-transaction budget. Both ceilings are
    /// caller-chosen; the amounts actually taken are measured here and never
    /// enter the caller's signature.
    fn exit_expired_to_asset(
        env: Env,
        from: Address,
        max_pt: i128,
        max_shares: i128,
        min_asset_out: i128,
    ) -> Result<i128, YieldManagerError>;
    /// Callable only by the YT contract (its `claim_yield`). Pays out accrued
    /// yield to `to`. Once the rate is locked, the frozen share count is
    /// re-denominated to its locked-rate asset value at the live rate, so the
    /// payout may be fewer shares than requested. Returns the shares actually
    /// sent.
    fn distribute_yield(
        env: Env,
        to: Address,
        shares_amount: i128,
    ) -> Result<i128, YieldManagerError>;
    /// Post-maturity: burns `pt_amount` PT from `from` and pays its face value
    /// in vault shares at the live rate (floored at the locked rate).
    fn redeem_principal(env: Env, from: Address, pt_amount: i128) -> Result<(), YieldManagerError>;

    /// "You snooze you lose": sweeps accumulated protocol surplus to the
    /// treasury. Positions freeze in asset value at maturity — PT at face
    /// value, YT yield at its locked-rate value — so every post-maturity
    /// redemption or claim above the locked rate needs fewer shares than were
    /// reserved; the difference (the vault interest earned after maturity)
    /// accumulates here. Never touches shares users still have a claim on,
    /// so PT redemption and YT claims stay open forever. Permissionless (the
    /// destination is fixed); returns the amount swept (0 if none).
    fn collect_surplus(env: Env) -> Result<i128, YieldManagerError>;
}

#![no_std]

use soroban_sdk::{contractclient, contracterror, contracttype, Address, Env};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum VaultType {
    Vault4626 = 0,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum YieldManagerError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InvalidAmount = 3,
    MaturityReached = 4,
    MaturityNotReached = 5,
    ExchangeRateZero = 6,
    PoolAlreadySet = 7,
    SlippageExceeded = 8,
    VaultDepositFailed = 9,
}

#[contractclient(name = "YieldManagerClient")]
pub trait YieldManagerTrait {
    fn __constructor(
        env: Env,
        admin: Address,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
        treasury: Address,
    );

    fn set_token_contracts(env: Env, pt_addr: Address, yt_addr: Address) -> Result<(), YieldManagerError>;

    /// Registers the AMM pool trusted to drive the flash-swap callbacks.
    /// One-shot: can only be set once, by the admin.
    fn set_pool(env: Env, pool: Address) -> Result<(), YieldManagerError>;
    fn get_pool(env: Env) -> Address;
    fn get_vault(env: Env) -> Address;
    fn get_principal_token(env: Env) -> Address;
    fn get_yield_token(env: Env) -> Address;
    fn get_maturity(env: Env) -> u64;
    fn get_treasury(env: Env) -> Address;

    /// Returns the current exchange rate. The rate can only
    /// increase, and locks permanently once maturity is reached.
    fn get_exchange_rate(env: Env) -> i128;
    fn deposit(env: Env, from: Address, shares_amount: i128) -> Result<(), YieldManagerError>;
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
    fn deposit_asset(env: Env, from: Address, asset_amount: i128, min_tokens_out: i128) -> Result<i128, YieldManagerError>;

    /// Recombines `amount` of PT + YT (burned from `from`) back into the
    /// underlying: the YM redeems the owed shares from its own custody and the
    /// vault pays the asset directly to `from`. Returns the asset delivered;
    /// fails if below `min_asset_out`. Pre-maturity only, like redeem_combined.
    fn redeem_combined_to_asset(env: Env, from: Address, amount: i128, min_asset_out: i128) -> Result<i128, YieldManagerError>;

    /// Post-maturity PT redemption paid in the underlying. Burns
    /// `min(max_pt, from's PT balance)` via `burn_from`, so the caller must
    /// first grant the YM a PT allowance of `max_pt` — an approval whose
    /// arguments are all caller-chosen, which is the point: the actual burn
    /// amount may be freshly measured (e.g. PT just withdrawn from an LP
    /// position) without ever appearing in the user's signature. Same
    /// face-value / surplus accounting as redeem_principal. Returns the asset
    /// delivered; fails if below `min_asset_out`.
    fn redeem_principal_to_asset(env: Env, from: Address, max_pt: i128, min_asset_out: i128) -> Result<i128, YieldManagerError>;

    /// Post-maturity exit paid entirely in the underlying, in ONE vault
    /// redemption: burns up to `max_pt` PT at face value AND absorbs up to
    /// `max_shares` of vault shares the caller is already holding (an LP
    /// withdrawal's payout, a YT yield claim), then redeems the combined total
    /// from YM custody.
    ///
    /// The single redemption is the entire point. A vault redemption is a
    /// submission into the underlying lending pool and by far the most
    /// expensive operation in the protocol; doing the PT leg and the loose
    /// shares as two separate redemptions pushed the router's expired exit past
    /// the per-transaction budget whenever an LP position was involved. Pulling
    /// the caller's shares in with a cheap `transfer_from` first and redeeming
    /// once fits.
    ///
    /// Both ceilings are caller-chosen and consume allowances the caller granted
    /// the YM (PT, and vault shares), so the amounts actually taken may be
    /// freshly measured without ever entering the caller's signature.
    fn exit_expired_to_asset(env: Env, from: Address, max_pt: i128, max_shares: i128, min_asset_out: i128) -> Result<i128, YieldManagerError>;
    /// Pays out accrued yield to `to`. Once the rate is locked, the frozen
    /// share count is re-denominated to its locked-rate asset value at the
    /// live rate, so the payout may be fewer shares than requested. Returns
    /// the shares actually sent.
    fn distribute_yield(env: Env, to: Address, shares_amount: i128) -> Result<i128, YieldManagerError>;
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

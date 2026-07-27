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

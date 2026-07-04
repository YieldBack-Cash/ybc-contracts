#![no_std]

use soroban_sdk::{contractclient, contracterror, contracttype, Address, Env};

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum VaultType {
    Vault4626 = 0,
    VaultDefindex = 1
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
}

/// Trait defining the interface for the Yield Manager contract.
/// This trait is used to generate the YieldManagerClient for type-safe cross-contract calls.
#[contractclient(name = "YieldManagerClient")]
pub trait YieldManagerTrait {
    fn __constructor(
        env: Env,
        admin: Address,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
    );

    fn set_token_contracts(env: Env, pt_addr: Address, yt_addr: Address) -> Result<(), YieldManagerError>;
    fn get_vault(env: Env) -> Address;
    fn get_principal_token(env: Env) -> Address;
    fn get_yield_token(env: Env) -> Address;
    fn get_maturity(env: Env) -> u64;

    /// Returns the current exchange rate. Before maturity this also refreshes and
    /// persists the stored rate from the vault as a side effect (the rate can only
    /// increase, and locks permanently once maturity is reached).
    fn get_exchange_rate(env: Env) -> i128;
    fn deposit(env: Env, from: Address, shares_amount: i128) -> Result<(), YieldManagerError>;
    fn redeem(env: Env, from: Address, amount: i128) -> Result<(), YieldManagerError>;
    fn distribute_yield(env: Env, to: Address, shares_amount: i128) -> Result<(), YieldManagerError>;
    fn redeem_principal(env: Env, from: Address, pt_amount: i128) -> Result<(), YieldManagerError>;
}

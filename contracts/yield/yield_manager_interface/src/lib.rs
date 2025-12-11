#![no_std]

use soroban_sdk::{contractclient, contracterror, Address, Env};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum VaultType {
    Vault4626 = 0,
    VaultDefindex = 1
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

    fn set_token_contracts(env: Env, pt_addr: Address, yt_addr: Address);

    fn get_vault(env: Env) -> Address;
    fn get_principal_token(env: Env) -> Address;
    fn get_yield_token(env: Env) -> Address;
    fn get_maturity(env: Env) -> u64;
    fn get_exchange_rate(env: Env) -> i128;
    fn deposit(env: Env, from: Address, shares_amount: i128);
    fn distribute_yield(env: Env, to: Address, shares_amount: i128);
    fn redeem_principal(env: Env, from: Address, pt_amount: i128);
}

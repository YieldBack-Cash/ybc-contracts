use soroban_sdk::{contract, contractimpl, Address, Env, MuxedAddress, String};
use stellar_tokens::fungible::{Base, FungibleToken};
use stellar_tokens::fungible::burnable::FungibleBurnable;
use crate::storage;

pub trait MockVaultTrait {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32);

    fn set_exchange_rate(env: Env, rate: i128);
    fn get_admin(env: Env) -> Address;
    fn convert_to_assets(env: Env, shares: i128) -> i128;
    fn mint(env: &Env, to: Address, amount: i128);
}

#[contract]
pub struct MockVault;

#[contractimpl]
impl MockVaultTrait for MockVault {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32) {
        storage::set_admin(&env, &admin);
        // Initialize exchange rate to 1.0 (scaled by 1e7)
        storage::set_exchange_rate(&env, 1_000_0000);

        // Initialize OpenZeppelin token metadata
        Base::set_metadata(&env, decimals, name, symbol);
    }

    fn set_exchange_rate(env: Env, rate: i128) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_exchange_rate(&env, rate);
    }

    fn get_admin(env: Env) -> Address {
        storage::get_admin(&env)
    }

    fn convert_to_assets(env: Env, shares: i128) -> i128 {
        let exchange_rate = storage::get_exchange_rate(&env);
        shares * exchange_rate
    }

    /// Mint tokens to an address (admin only for mock purposes)
    fn mint(env: &Env, to: Address, amount: i128) {
        let admin = storage::get_admin(env);
        admin.require_auth();
        Base::mint(env, &to, amount);
    }
}

#[contractimpl]
impl FungibleToken for MockVault {
    type ContractType = Base;
    fn total_supply(e: &Env) -> i128 {
        Base::total_supply(e)
    }

    fn balance(e: &Env, account: Address) -> i128 {
        Base::balance(e, &account)
    }

    fn allowance(e: &Env, owner: Address, spender: Address) -> i128 {
        Base::allowance(e, &owner, &spender)
    }

    fn transfer(e: &Env, from: Address, to: MuxedAddress, amount: i128) {
        Base::transfer(e, &from, &to, amount)  // THIS is the missing transfer function
    }

    fn transfer_from(e: &Env, spender: Address, from: Address, to: Address, amount: i128) {
        Base::transfer_from(e, &spender, &from, &to, amount)
    }

    fn approve(e: &Env, owner: Address, spender: Address, amount: i128, live_until_ledger: u32) {
        Base::approve(e, &owner, &spender, amount, live_until_ledger)
    }

    fn decimals(e: &Env) -> u32 {
        Base::decimals(e)
    }

    fn name(e: &Env) -> String {
        Base::name(e)
    }

    fn symbol(e: &Env) -> String {
        Base::symbol(e)
    }
}

#[contractimpl]
impl FungibleBurnable for MockVault {
    fn burn(e: &Env, from: Address, amount: i128) {
        Base::burn(e, &from, amount)
    }

    fn burn_from(e: &Env, spender: Address, from: Address, amount: i128) {
        Base::burn_from(e, &spender, &from, amount)
    }
}
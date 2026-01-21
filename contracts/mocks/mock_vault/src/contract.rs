use stellar_tokens::token::fungible::{Fungible, FungibleRef};
use soroban_sdk::{contract, contractimpl, Address, Env, String};
use crate::storage;

pub trait MockVaultTrait {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32);

    fn set_exchange_rate(env: Env, rate: i128);
    fn get_admin(env: Env) -> Address;
    fn convert_to_assets(env: Env, shares: i128) -> i128;
}

#[contract]
pub struct MockVault;

#[contractimpl]
impl MockVaultTrait for MockVault {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32) {
        storage::set_admin(&env, &admin);
        // Initialize exchange rate to 1.0 (scaled by 1e7)
        storage::set_exchange_rate(&env, 1_000_0000);

        // Initialize OpenZeppelin token
        let mut token = FungibleRef::new(&env);
        token.init(name, symbol, decimals, admin);
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
        shares * exchange_rate / 1_000_0000
    }
}

#[contractimpl]
impl Fungible for MockVault {}
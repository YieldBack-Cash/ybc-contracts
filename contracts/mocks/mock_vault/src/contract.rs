use soroban_sdk::{contract, contractimpl, Address, Env};
use vault_interface::VaultTrait;
use crate::storage;

pub trait MockVaultTrait {
    fn __constructor(env: Env, admin: Address);

    fn set_exchange_rate(env: Env, rate: i128);
    fn get_admin(env: Env) -> Address;
}

#[contract]
pub struct MockVault;

#[contractimpl]
impl MockVaultTrait for MockVault {
    fn __constructor(env: Env, admin: Address) {
        storage::set_admin(&env, &admin);
        // Initialize exchange rate to 1.0 (scaled by 1e7)
        storage::set_exchange_rate(&env, 1_000_0000);
    }

    fn set_exchange_rate(env: Env, rate: i128) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::set_exchange_rate(&env, rate);
    }

    fn get_admin(env: Env) -> Address {
        storage::get_admin(&env)
    }
}
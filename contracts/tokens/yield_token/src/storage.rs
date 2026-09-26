use soroban_sdk::{contracttype, Address, Env};
use ybc_common::ttl::extend_persistent_ttl;

// Balances, allowances, total supply and metadata are OpenZeppelin's
// (`stellar_tokens::fungible::Base`) and never appear here. What is here is
// the accrual layer: per holder, the rate they last settled at and the yield
// accrued since, in vault shares.

pub use ybc_common::ttl::extend_instance_ttl;

#[contracttype]
pub enum DataKey {
    /// The yield manager: the only address that may mint, and the source of
    /// the exchange rate.
    Admin,
    /// The vault exchange rate the holder last settled at.
    UserIndex(Address),
    /// Yield accrued since, in vault shares, awaiting `claim_yield`.
    AccruedYield(Address),
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("Admin not set")
}

fn set_persistent(env: &Env, key: DataKey, value: i128) {
    env.storage().persistent().set(&key, &value);
    extend_persistent_ttl(env, &key);
}

fn get_persistent(env: &Env, key: DataKey) -> i128 {
    match env.storage().persistent().get(&key) {
        Some(value) => {
            extend_persistent_ttl(env, &key);
            value
        }
        None => 0,
    }
}

pub fn set_user_index(env: &Env, address: &Address, index: i128) {
    set_persistent(env, DataKey::UserIndex(address.clone()), index);
}

pub fn get_user_index(env: &Env, address: &Address) -> i128 {
    get_persistent(env, DataKey::UserIndex(address.clone()))
}

pub fn set_accrued_yield(env: &Env, address: &Address, amount: i128) {
    set_persistent(env, DataKey::AccruedYield(address.clone()), amount);
}

pub fn get_accrued_yield(env: &Env, address: &Address) -> i128 {
    get_persistent(env, DataKey::AccruedYield(address.clone()))
}

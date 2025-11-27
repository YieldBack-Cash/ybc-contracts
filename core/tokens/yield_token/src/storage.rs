use soroban_sdk::{contracttype, Address, Env, String};

#[contracttype]
#[derive(Clone)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Balance(Address),
    UserIndex(Address), // vault exchange rate the user last interacted at
    AccruedYield(Address),
}

// Storage keys
const ADMIN_KEY: &str = "admin";
const METADATA_KEY: &str = "metadata";
const TOTAL_SUPPLY_KEY: &str = "total_supply";

// Admin functions
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&ADMIN_KEY, admin);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&ADMIN_KEY)
        .expect("Admin not set")
}

// Token metadata
pub fn set_metadata(env: &Env, name: String, symbol: String) {
    let metadata = TokenMetadata { name, symbol };
    env.storage().instance().set(&METADATA_KEY, &metadata);
}

pub fn get_metadata(env: &Env) -> TokenMetadata {
    env.storage()
        .instance()
        .get(&METADATA_KEY)
        .expect("Metadata not set")
}

// Total supply
pub fn set_total_supply(env: &Env, supply: i128) {
    env.storage().instance().set(&TOTAL_SUPPLY_KEY, &supply);
}

pub fn get_total_supply(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&TOTAL_SUPPLY_KEY)
        .unwrap_or(0)
}

// User balance
pub fn set_balance(env: &Env, address: &Address, balance: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Balance(address.clone()), &balance);
}

pub fn get_balance(env: &Env, address: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Balance(address.clone()))
        .unwrap_or(0)
}

// User index (exchange rate at last interaction)
pub fn set_user_index(env: &Env, address: &Address, index: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::UserIndex(address.clone()), &index);
}

pub fn get_user_index(env: &Env, address: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::UserIndex(address.clone()))
        .unwrap_or(0)
}

// Accrued yield (accumulated yield not yet claimed)
pub fn set_accrued_yield(env: &Env, address: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::AccruedYield(address.clone()), &amount);
}

pub fn get_accrued_yield(env: &Env, address: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::AccruedYield(address.clone()))
        .unwrap_or(0)
}

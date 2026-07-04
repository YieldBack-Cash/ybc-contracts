use soroban_sdk::{contracttype, Address, Env, String};

#[contracttype]
#[derive(Clone)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimal: u32,
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

// Storage TTL constants
pub const DAY_IN_LEDGERS: u32 = 17280;
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

pub const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extends the instance TTL (admin, metadata, total supply). Call once per
/// entrypoint so the contract's own config doesn't expire from inactivity.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

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
pub fn set_metadata(env: &Env, name: String, symbol: String, decimal: u32) {
    let metadata = TokenMetadata { name, symbol, decimal };
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
    let key = DataKey::Balance(address.clone());
    env.storage().persistent().set(&key, &balance);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
}

pub fn get_balance(env: &Env, address: &Address) -> i128 {
    let key = DataKey::Balance(address.clone());
    if let Some(balance) = env.storage().persistent().get(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
        balance
    } else {
        0
    }
}

// User index (exchange rate at last interaction)
pub fn set_user_index(env: &Env, address: &Address, index: i128) {
    let key = DataKey::UserIndex(address.clone());
    env.storage().persistent().set(&key, &index);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
}

pub fn get_user_index(env: &Env, address: &Address) -> i128 {
    let key = DataKey::UserIndex(address.clone());
    if let Some(index) = env.storage().persistent().get(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
        index
    } else {
        0
    }
}

// Accrued yield (accumulated yield not yet claimed)
pub fn set_accrued_yield(env: &Env, address: &Address, amount: i128) {
    let key = DataKey::AccruedYield(address.clone());
    env.storage().persistent().set(&key, &amount);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
}

pub fn get_accrued_yield(env: &Env, address: &Address) -> i128 {
    let key = DataKey::AccruedYield(address.clone());
    if let Some(amount) = env.storage().persistent().get(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
        amount
    } else {
        0
    }
}
use soroban_sdk::{Address, Env};

// Storage keys
const ADMIN_KEY: &str = "admin";
const VAULT_KEY: &str = "vault";
const PRINCIPAL_TOKEN_KEY: &str = "principal_token";
const YIELD_TOKEN_KEY: &str = "yield_token";
const MATURITY_KEY: &str = "maturity";
const EXCHANGE_RATE_KEY: &str = "exchange_rate";
const RATE_LOCKED_KEY: &str = "rate_locked";
const INITIALIZED_KEY: &str = "initialized"; // TODO: redundant??

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

// Vault address (immutable after initialization)
pub fn set_vault(env: &Env, vault: &Address) {
    env.storage().instance().set(&VAULT_KEY, vault);
}

pub fn get_vault(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&VAULT_KEY)
        .expect("Vault not set")
}

// Maturity timestamp (immutable after initialization)
pub fn set_maturity(env: &Env, maturity: u64) {
    env.storage().instance().set(&MATURITY_KEY, &maturity);
}

pub fn get_maturity(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&MATURITY_KEY)
        .expect("Maturity not set")
}

// Principal Token address (immutable after initialization)
pub fn set_principal_token(env: &Env, token: &Address) {
    env.storage().instance().set(&PRINCIPAL_TOKEN_KEY, token);
}

pub fn get_principal_token(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&PRINCIPAL_TOKEN_KEY)
        .expect("Principal token not set")
}

// Yield Token address (immutable after initialization)
pub fn set_yield_token(env: &Env, token: &Address) {
    env.storage().instance().set(&YIELD_TOKEN_KEY, token);
}

pub fn get_yield_token(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&YIELD_TOKEN_KEY)
        .expect("Yield token not set")
}

// Current exchange rate (updated on every operation until maturity)
pub fn set_exchange_rate(env: &Env, rate: i128) {
    env.storage().instance().set(&EXCHANGE_RATE_KEY, &rate);
}

pub fn get_exchange_rate(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&EXCHANGE_RATE_KEY)
        .expect("Exchange rate not set")
}

// Rate locked flag (set once when rate is locked at maturity)
pub fn is_rate_locked(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&RATE_LOCKED_KEY)
        .unwrap_or(false)
}

pub fn set_rate_locked(env: &Env) {
    env.storage().instance().set(&RATE_LOCKED_KEY, &true);
}

// Initialization flag (set once when token contracts are set)
pub fn is_initialized(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&INITIALIZED_KEY)
        .unwrap_or(false)
}

pub fn set_initialized(env: &Env) {
    env.storage().instance().set(&INITIALIZED_KEY, &true);
}
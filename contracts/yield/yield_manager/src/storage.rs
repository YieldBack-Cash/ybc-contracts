use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    Admin,
    Vault,
    PrincipalToken,
    YieldToken,
    Maturity,
    ExchangeRate,
    RateLocked,
    Pool,
    Treasury,
    SurplusShares,
}

/// All YM state is instance storage. Call once per entry point: if the
/// instance expires the market is bricked.
pub use ybc_common::ttl::extend_instance_ttl;

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect("Admin not set")
}

// Vault address (immutable after initialization)
pub fn set_vault(env: &Env, vault: &Address) {
    env.storage().instance().set(&DataKey::Vault, vault);
}

pub fn get_vault(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Vault)
        .expect("Vault not set")
}

// Maturity timestamp (immutable after initialization)
pub fn set_maturity(env: &Env, maturity: u64) {
    env.storage().instance().set(&DataKey::Maturity, &maturity);
}

pub fn get_maturity(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::Maturity)
        .expect("Maturity not set")
}

// Principal Token address (immutable after initialization)
pub fn set_principal_token(env: &Env, token: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::PrincipalToken, token);
}

pub fn get_principal_token(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::PrincipalToken)
        .expect("Principal token not set")
}

// Yield Token address (immutable after initialization)
pub fn set_yield_token(env: &Env, token: &Address) {
    env.storage().instance().set(&DataKey::YieldToken, token);
}

pub fn get_yield_token(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::YieldToken)
        .expect("Yield token not set")
}

/// High-water mark of the vault rate, assets per SCALAR_7 shares. Frozen once
/// `RateLocked` is set.
pub fn set_exchange_rate(env: &Env, rate: i128) {
    env.storage().instance().set(&DataKey::ExchangeRate, &rate);
}

pub fn get_exchange_rate(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::ExchangeRate)
        .expect("Exchange rate not set")
}

// Rate locked flag (set once when rate is locked at maturity)
pub fn is_rate_locked(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::RateLocked)
        .unwrap_or(false)
}

pub fn set_rate_locked(env: &Env) {
    env.storage().instance().set(&DataKey::RateLocked, &true);
}

// Token contracts are configured once, atomically, in set_token_contracts;
// their presence in storage is the source of truth for initialization.
pub fn is_initialized(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::PrincipalToken)
        && env.storage().instance().has(&DataKey::YieldToken)
}

// Trusted AMM pool address (immutable after being set once).
pub fn set_pool(env: &Env, pool: &Address) {
    env.storage().instance().set(&DataKey::Pool, pool);
}

pub fn get_pool(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Pool)
        .expect("Pool not set")
}

pub fn is_pool_set(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Pool)
}

// Protocol fee sink for collect_surplus (immutable after construction).
pub fn set_treasury(env: &Env, treasury: &Address) {
    env.storage().instance().set(&DataKey::Treasury, treasury);
}

pub fn get_treasury(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Treasury)
        .expect("Treasury not set")
}

// Vault shares freed by post-maturity redemptions/claims above the locked
// rate — protocol surplus awaiting collection. Only ever holds shares no
// user has a claim on.
pub fn get_surplus_shares(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::SurplusShares)
        .unwrap_or(0)
}

pub fn set_surplus_shares(env: &Env, amount: i128) {
    env.storage()
        .instance()
        .set(&DataKey::SurplusShares, &amount);
}

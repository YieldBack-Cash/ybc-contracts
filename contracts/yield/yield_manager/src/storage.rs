use soroban_sdk::{contracttype, Address, Env};
use yield_manager_interface::VaultType;

#[contracttype]
pub enum DataKey {
    Admin,
    Vault,
    VaultType,
    PrincipalToken,
    YieldToken,
    Maturity,
    ExchangeRate,
    RateLocked,
    Pool,
}

// Storage TTL constants
pub const DAY_IN_LEDGERS: u32 = 17280;
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extends the instance TTL (admin, vault/token addresses, exchange rate,
/// maturity). Call once per entrypoint -- if this expires the whole protocol
/// is bricked, not just one user's data.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

// Admin functions
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

// Vault type (immutable after initialization)
pub fn set_vault_type(env: &Env, vault_type: VaultType) {
    env.storage().instance().set(&DataKey::VaultType, &vault_type);
}

pub fn get_vault_type(env: &Env) -> VaultType {
    env.storage().instance().get(&DataKey::VaultType).expect("Vault type not set")
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
    env.storage().instance().set(&DataKey::PrincipalToken, token);
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

// Current exchange rate (updated on every operation until maturity)
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

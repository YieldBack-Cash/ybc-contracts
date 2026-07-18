use crate::contract::{Market, WasmHashes};
use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
enum DataKey {
    Admin,
    WasmHashes,
    SaltCounter,
    Market(Address, u64),
}

// Storage TTL constants
pub const DAY_IN_LEDGERS: u32 = 17280;
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

pub const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extends the instance TTL (admin, wasm hashes, salt counter). Call once per
/// entrypoint -- if this expires, the factory (and with it market resolution
/// for the router) is bricked until restored.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
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

pub fn set_wasm_hashes(env: &Env, hashes: &WasmHashes) {
    env.storage().instance().set(&DataKey::WasmHashes, hashes);
}

pub fn get_wasm_hashes(env: &Env) -> WasmHashes {
    env.storage()
        .instance()
        .get(&DataKey::WasmHashes)
        .expect("WASM hashes not set")
}

pub fn get_salt_counter(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::SaltCounter)
        .unwrap_or(0)
}

pub fn set_salt_counter(env: &Env, counter: u32) {
    env.storage()
        .instance()
        .set(&DataKey::SaltCounter, &counter);
}

/// Direct lookup of a market by (vault, maturity). Each market is its own
/// PERSISTENT ledger entry keyed by the pair, so a vault can host any number of
/// markets at different maturities without them sharing (and eventually
/// overflowing) a single entry, and none of them bloat the shared contract
/// instance entry. Redeploying a pool for the same maturity is rejected by
/// create_market, so an entry, once written, is never overwritten.
///
/// There is deliberately no on-chain list of all markets or vaults: enumeration
/// is served off-chain by the indexer from MarketCreated events.
pub fn get_market(env: &Env, vault: &Address, maturity: u64) -> Option<Market> {
    let key = DataKey::Market(vault.clone(), maturity);
    let market = env.storage().persistent().get(&key);
    // Reads don't auto-extend TTL, and the router resolves every operation
    // through this lookup — renew on read so any activity keeps the market
    // entry alive.
    if market.is_some() {
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }
    market
}

pub fn set_market(env: &Env, vault: &Address, market: Market) {
    let maturity = market.maturity;
    let key = DataKey::Market(vault.clone(), maturity);
    env.storage().persistent().set(&key, &market);
    env.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );
}
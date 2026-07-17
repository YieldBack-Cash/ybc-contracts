use crate::contract::{Market, WasmHashes};
use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
enum DataKey {
    Admin,
    WasmHashes,
    SaltCounter,
    Market(Address, u64),
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
    env.storage()
        .persistent()
        .get(&DataKey::Market(vault.clone(), maturity))
}

pub fn set_market(env: &Env, vault: &Address, market: Market) {
    let maturity = market.maturity;
    env.storage()
        .persistent()
        .set(&DataKey::Market(vault.clone(), maturity), &market);
}
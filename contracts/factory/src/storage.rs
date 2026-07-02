use crate::contract::{Market, WasmHashes};
use soroban_sdk::{contracttype, Address, Env, Vec};

#[contracttype]
enum DataKey {
    Admin,
    WasmHashes,
    SaltCounter,
    Vaults,
    CurrentYm(Address),
    CurrentPt(Address),
    CurrentYt(Address),
    CurrentPool(Address),
    Markets(Address),
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

pub fn get_vaults(env: &Env) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::Vaults)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn register_vault(env: &Env, vault: &Address) {
    let mut vaults = get_vaults(env);
    if !vaults.contains(vault) {
        vaults.push_back(vault.clone());
        env.storage().persistent().set(&DataKey::Vaults, &vaults);
    }
}

pub fn set_current_yield_manager(env: &Env, vault: &Address, ym: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::CurrentYm(vault.clone()), ym);
}

pub fn get_current_yield_manager(env: &Env, vault: &Address) -> Option<Address> {
    env.storage()
        .instance()
        .get(&DataKey::CurrentYm(vault.clone()))
}

pub fn set_current_pt_token(env: &Env, vault: &Address, pt: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::CurrentPt(vault.clone()), pt);
}

pub fn get_current_pt_token(env: &Env, vault: &Address) -> Option<Address> {
    env.storage()
        .instance()
        .get(&DataKey::CurrentPt(vault.clone()))
}

pub fn set_current_yt_token(env: &Env, vault: &Address, yt: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::CurrentYt(vault.clone()), yt);
}

pub fn get_current_yt_token(env: &Env, vault: &Address) -> Option<Address> {
    env.storage()
        .instance()
        .get(&DataKey::CurrentYt(vault.clone()))
}

pub fn set_current_pool(env: &Env, vault: &Address, pool: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::CurrentPool(vault.clone()), pool);
}

pub fn get_current_pool(env: &Env, vault: &Address) -> Option<Address> {
    env.storage()
        .instance()
        .get(&DataKey::CurrentPool(vault.clone()))
}

pub fn get_markets(env: &Env, vault: &Address) -> Vec<Market> {
    env.storage()
        .persistent()
        .get(&DataKey::Markets(vault.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn push_market(env: &Env, vault: &Address, market: Market) {
    let mut markets = get_markets(env, vault);
    markets.push_back(market);
    env.storage()
        .persistent()
        .set(&DataKey::Markets(vault.clone()), &markets);
}

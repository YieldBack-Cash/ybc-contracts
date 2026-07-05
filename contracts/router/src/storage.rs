use soroban_sdk::{Address, Env};

const AMM_KEY: &str = "amm";
const YM_KEY: &str = "ym";

// Storage TTL constants
pub const DAY_IN_LEDGERS: u32 = 17280;
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extends the instance TTL (the AMM/YM addresses this router points at).
/// Call once per entrypoint so the router doesn't expire from inactivity.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

pub fn set_amm(env: &Env, amm: &Address) {
    env.storage().instance().set(&AMM_KEY, amm);
}

pub fn get_amm(env: &Env) -> Address {
    env.storage().instance().get(&AMM_KEY).unwrap()
}

pub fn set_ym(env: &Env, ym: &Address) {
    env.storage().instance().set(&YM_KEY, ym);
}

pub fn get_ym(env: &Env) -> Address {
    env.storage().instance().get(&YM_KEY).unwrap()
}
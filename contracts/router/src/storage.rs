use soroban_sdk::{Address, Env};

const FACTORY_KEY: &str = "factory";

// Storage TTL constants
pub const DAY_IN_LEDGERS: u32 = 17280;
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extends the instance TTL (the factory address this router resolves markets
/// through). Call once per entrypoint so the router doesn't expire from
/// inactivity.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

pub fn set_factory(env: &Env, factory: &Address) {
    env.storage().instance().set(&FACTORY_KEY, factory);
}

pub fn get_factory(env: &Env) -> Address {
    env.storage().instance().get(&FACTORY_KEY).unwrap()
}
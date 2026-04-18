use soroban_sdk::{Address, Env};

const ADMIN_KEY: &str = "admin";
const EXCHANGE_RATE_KEY: &str = "ex_rate";

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&ADMIN_KEY, admin);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&ADMIN_KEY)
        .expect("Admin not set")
}

pub fn set_exchange_rate(env: &Env, rate: i128) {
    env.storage().instance().set(&EXCHANGE_RATE_KEY, &rate);
}

pub fn get_exchange_rate(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&EXCHANGE_RATE_KEY)
        .unwrap_or(1_000_0000) // Default to 1.0
}

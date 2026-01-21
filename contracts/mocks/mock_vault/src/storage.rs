use soroban_sdk::{Address, Env, Map};

const ADMIN_KEY: &str = "admin";
const EXCHANGE_RATE_KEY: &str = "ex_rate";
const BALANCES_KEY: &str = "balances";

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

// Token balance functions
pub fn get_balance(env: &Env, addr: &Address) -> i128 {
    let balances: Map<Address, i128> = env.storage().instance().get(&BALANCES_KEY).unwrap_or(Map::new(env));
    balances.get(addr.clone()).unwrap_or(0)
}

pub fn set_balance(env: &Env, addr: &Address, amount: i128) {
    let mut balances: Map<Address, i128> = env.storage().instance().get(&BALANCES_KEY).unwrap_or(Map::new(env));
    balances.set(addr.clone(), amount);
    env.storage().instance().set(&BALANCES_KEY, &balances);
}

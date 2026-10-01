use soroban_sdk::{contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    Admin,
    ExchangeRate,
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

pub fn set_exchange_rate(env: &Env, rate: i128) {
    env.storage().instance().set(&DataKey::ExchangeRate, &rate);
}

pub fn get_exchange_rate(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::ExchangeRate)
        .expect("rate not set") // set in the constructor
}

use soroban_sdk::{contracttype, token, Address, Env};

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    TokenA,
    TokenB,
    TotalShares,
    ReserveA,
    ReserveB,
    Shares(Address),
}

pub fn get_token_a(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::TokenA).unwrap()
}

pub fn get_token_b(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::TokenB).unwrap()
}

pub fn get_total_shares(e: &Env) -> i128 {
    e.storage().instance().get(&DataKey::TotalShares).unwrap()
}

pub fn get_reserve_a(e: &Env) -> i128 {
    e.storage().instance().get(&DataKey::ReserveA).unwrap()
}

pub fn get_reserve_b(e: &Env) -> i128 {
    e.storage().instance().get(&DataKey::ReserveB).unwrap()
}

pub fn get_balance(e: &Env, contract: Address) -> i128 {
    token::Client::new(e, &contract).balance(&e.current_contract_address())
}

pub fn get_balance_a(e: &Env) -> i128 {
    get_balance(e, get_token_a(e))
}

pub fn get_balance_b(e: &Env) -> i128 {
    get_balance(e, get_token_b(e))
}

pub fn get_shares(e: &Env, user: &Address) -> i128 {
    e.storage()
        .persistent()
        .get(&DataKey::Shares(user.clone()))
        .unwrap_or(0)
}

pub fn put_shares(e: &Env, user: &Address, amount: i128) {
    e.storage()
        .persistent()
        .set(&DataKey::Shares(user.clone()), &amount);
}

pub fn put_token_a(e: &Env, contract: Address) {
    e.storage().instance().set(&DataKey::TokenA, &contract);
}

pub fn put_token_b(e: &Env, contract: Address) {
    e.storage().instance().set(&DataKey::TokenB, &contract);
}

pub fn put_total_shares(e: &Env, amount: i128) {
    e.storage().instance().set(&DataKey::TotalShares, &amount)
}

pub fn put_reserve_a(e: &Env, amount: i128) {
    e.storage().instance().set(&DataKey::ReserveA, &amount)
}

pub fn put_reserve_b(e: &Env, amount: i128) {
    e.storage().instance().set(&DataKey::ReserveB, &amount)
}

pub fn burn_shares(e: &Env, from: &Address, amount: i128) {
    let current_shares = get_shares(e, from);
    if current_shares < amount {
        panic!("insufficient shares");
    }
    let total = get_total_shares(e);
    put_shares(e, from, current_shares - amount);
    put_total_shares(e, total - amount);
}

pub fn mint_shares(e: &Env, to: &Address, amount: i128) {
    let current_shares = get_shares(e, to);
    let total = get_total_shares(e);
    put_shares(e, to, current_shares + amount);
    put_total_shares(e, total + amount);
}

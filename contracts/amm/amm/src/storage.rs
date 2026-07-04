use soroban_sdk::{contracttype, token, Address, Env};

#[derive(Clone)]
#[contracttype]
pub struct MarketState {
    pub token_a: Address,
    pub token_b: Address, // TODO: vault address is the same as token_b (V token)
    pub reserve_a: i128,
    pub reserve_b: i128,
    pub expiry_ts: u64,
    pub last_implied_rate: i128,
    pub scalar_root: i128,
    pub initial_anchor: i128,
    pub fee_rate_root: i128,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    TotalShares,
    Shares(Address),
    MarketState,
}

// Storage TTL constants
pub const DAY_IN_LEDGERS: u32 = 17280;
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

pub const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extends the instance TTL (market state, total shares). Call once per
/// entrypoint so the pool's own config doesn't expire from inactivity.
pub fn extend_instance_ttl(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

pub fn get_market_state(e: &Env) -> MarketState {
    e.storage().instance().get(&DataKey::MarketState).unwrap()
}

pub fn put_market_state(e: &Env, state: &MarketState) {
    e.storage().instance().set(&DataKey::MarketState, state);
}

pub fn get_token_a(e: &Env) -> Address {
    get_market_state(e).token_a
}

pub fn get_token_b(e: &Env) -> Address {
    get_market_state(e).token_b
}

pub fn get_total_shares(e: &Env) -> i128 {
    e.storage().instance().get(&DataKey::TotalShares).unwrap()
}


pub fn get_balance(e: &Env, contract: Address) -> i128 {
    token::TokenClient::new(e, &contract).balance(&e.current_contract_address())
}

pub fn get_balance_a(e: &Env) -> i128 {
    get_balance(e, get_token_a(e))
}

pub fn get_balance_b(e: &Env) -> i128 {
    get_balance(e, get_token_b(e))
}

pub fn get_shares(e: &Env, user: &Address) -> i128 {
    let key = DataKey::Shares(user.clone());
    if let Some(shares) = e.storage().persistent().get(&key) {
        e.storage()
            .persistent()
            .extend_ttl(&key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
        shares
    } else {
        0
    }
}

pub fn put_shares(e: &Env, user: &Address, amount: i128) {
    let key = DataKey::Shares(user.clone());
    e.storage().persistent().set(&key, &amount);
    e.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
}

pub fn put_total_shares(e: &Env, amount: i128) {
    e.storage().instance().set(&DataKey::TotalShares, &amount)
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
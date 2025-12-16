use soroban_sdk::{contracttype, Address, Env, String};

// Storage TTL constants
pub const DAY_IN_LEDGERS: u32 = 17280;
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

pub const BALANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
pub const BALANCE_LIFETIME_THRESHOLD: u32 = BALANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

#[contracttype]
#[derive(Clone)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Allowance(Address, Address),
    Balance(Address),
    Admin,
    Metadata,
    TotalSupply,
}

// Admin functions
pub fn read_administrator(e: &Env) -> Address {
    let key = DataKey::Admin;
    e.storage().instance().get(&key).unwrap()
}

pub fn write_administrator(e: &Env, id: &Address) {
    let key = DataKey::Admin;
    e.storage().instance().set(&key, id);
}

// Metadata functions
pub fn read_metadata(e: &Env) -> TokenMetadata {
    let key = DataKey::Metadata;
    e.storage().instance().get(&key).unwrap()
}

pub fn write_metadata(e: &Env, metadata: TokenMetadata) {
    let key = DataKey::Metadata;
    e.storage().instance().set(&key, &metadata);
}

pub fn read_decimal(e: &Env) -> u32 {
    read_metadata(e).decimals
}

pub fn read_name(e: &Env) -> String {
    read_metadata(e).name
}

pub fn read_symbol(e: &Env) -> String {
    read_metadata(e).symbol
}

// Balance functions
pub fn read_balance(e: &Env, addr: &Address) -> i128 {
    let key = DataKey::Balance(addr.clone());
    if let Some(balance) = e.storage().persistent().get::<DataKey, i128>(&key) {
        e.storage()
            .persistent()
            .extend_ttl(&key, BALANCE_LIFETIME_THRESHOLD, BALANCE_BUMP_AMOUNT);
        balance
    } else {
        0
    }
}

fn write_balance(e: &Env, addr: &Address, amount: i128) {
    let key = DataKey::Balance(addr.clone());
    e.storage().persistent().set(&key, &amount);
    e.storage()
        .persistent()
        .extend_ttl(&key, BALANCE_LIFETIME_THRESHOLD, BALANCE_BUMP_AMOUNT);
}

pub fn receive_balance(e: &Env, addr: &Address, amount: i128) {
    let balance = read_balance(e, addr);
    write_balance(e, addr, balance + amount);
}

pub fn spend_balance(e: &Env, addr: &Address, amount: i128) {
    let balance = read_balance(e, addr);
    if balance < amount {
        panic!("insufficient balance");
    }
    write_balance(e, addr, balance - amount);
}

// Allowance functions
pub fn read_allowance(e: &Env, from: &Address, spender: &Address) -> i128 {
    let key = DataKey::Allowance(from.clone(), spender.clone());
    e.storage().temporary().get(&key).unwrap_or(0)
}

pub fn write_allowance(
    e: &Env,
    from: &Address,
    spender: &Address,
    amount: i128,
    expiration_ledger: u32,
) {
    let key = DataKey::Allowance(from.clone(), spender.clone());
    e.storage().temporary().set(&key, &amount);

    if expiration_ledger > 0 {
        let ledger = e.ledger().sequence();
        let live_for = expiration_ledger.saturating_sub(ledger);
        e.storage().temporary().extend_ttl(&key, live_for, live_for);
    }
}

pub fn spend_allowance(e: &Env, from: &Address, spender: &Address, amount: i128) {
    let allowance = read_allowance(e, from, spender);
    if allowance < amount {
        panic!("insufficient allowance");
    }
    write_allowance(e, from, spender, allowance - amount, 0);
}

// Total supply functions
pub fn read_total_supply(e: &Env) -> i128 {
    let key = DataKey::TotalSupply;
    e.storage().instance().get(&key).unwrap_or(0)
}

pub fn write_total_supply(e: &Env, amount: i128) {
    let key = DataKey::TotalSupply;
    e.storage().instance().set(&key, &amount);
}

pub fn increase_total_supply(e: &Env, amount: i128) {
    let total_supply = read_total_supply(e);
    write_total_supply(e, total_supply + amount);
}

pub fn decrease_total_supply(e: &Env, amount: i128) {
    let total_supply = read_total_supply(e);
    write_total_supply(e, total_supply - amount);
}

#![no_std]
use soroban_sdk::{contractclient, Address, Env, String};

#[contractclient(name = "YieldTokenClient")]
pub trait YieldTokenTrait {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String);
    fn mint(env: Env, to: Address, amount: i128, exchange_rate: i128);
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
    fn burn(env: Env, from: Address, amount: i128);
    fn balance(env: Env, address: Address) -> i128;
    fn user_index(env: Env, address: Address) -> i128;
    fn accrued_yield(env: Env, address: Address) -> i128;
    fn total_supply(env: Env) -> i128;
    fn name(env: Env) -> String;
    fn symbol(env: Env) -> String;
    fn claim_yield(env: Env, user: Address) -> i128;
}

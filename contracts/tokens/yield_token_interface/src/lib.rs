#![no_std]
use soroban_sdk::{contractclient, Address, Env, String};
use soroban_sdk::token::TokenInterface;

pub use soroban_sdk::token::TokenInterface as YieldTokenInterface;

#[contractclient(name = "YieldTokenClient")]
pub trait YieldTokenTrait: TokenInterface {
    fn __constructor(env: Env, admin: Address, decimals: u32, name: String, symbol: String);
    fn mint(env: Env, to: Address, amount: i128, exchange_rate: i128);
    fn user_index(env: Env, address: Address) -> i128;
    fn accrued_yield(env: Env, address: Address) -> i128;
    fn claim_yield(env: Env, user: Address) -> i128;
}
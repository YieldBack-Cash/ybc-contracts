#![no_std]
use soroban_sdk::{contractclient, Address, Env, String};
use soroban_sdk::token::TokenInterface;

#[contractclient(name = "PrincipalTokenClient")]
pub trait PrincipalTokenTrait: TokenInterface {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32);

    // Custom mint function for yield manager control
    fn mint(env: Env, to: Address, amount: i128);
}

#![no_std]

use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "AmmClient")]
pub trait AmmInterface {
    fn swap_v_for_pt(env: Env, to: Address, pt_out: i128, v_in_max: i128);
    fn swap_pt_for_v(env: Env, to: Address, pt_in: i128, min_v_out: i128);
    fn deposit(env: Env, to: Address, desired_a: i128, min_a: i128, desired_b: i128, min_b: i128);
    fn withdraw(env: Env, to: Address, share_amount: i128, min_a: i128, min_b: i128) -> (i128, i128);
    fn get_reserves(env: Env) -> (i128, i128);
    fn balance_shares(env: Env, user: Address) -> i128;
}
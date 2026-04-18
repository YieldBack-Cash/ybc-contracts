use amm_interface::AmmClient;
use soroban_sdk::{contract, contractclient, contractimpl, Address, Env};

#[contractclient(name = "RouterClient")]
pub trait RouterInterface {
    fn get_amm(env: Env) -> Address;
    fn swap_v_for_pt(env: Env, to: Address, pt_out: i128, v_in_max: i128);
    fn swap_pt_for_v(env: Env, to: Address, pt_in: i128, min_v_out: i128);
    fn deposit(env: Env, to: Address, desired_a: i128, min_a: i128, desired_b: i128, min_b: i128);
    fn withdraw(env: Env, to: Address, share_amount: i128, min_a: i128, min_b: i128) -> (i128, i128);
    fn get_reserves(env: Env) -> (i128, i128);
    fn balance_shares(env: Env, user: Address) -> i128;
}

const AMM_KEY: &str = "amm";

fn get_amm(e: &Env) -> Address {
    e.storage().instance().get(&AMM_KEY).unwrap()
}

#[contract]
pub struct RouterContract;

#[contractimpl]
impl RouterContract {
    pub fn __constructor(e: Env, amm: Address) {
        e.storage().instance().set(&AMM_KEY, &amm);
    }
}

#[contractimpl]
impl RouterInterface for RouterContract {
    fn get_amm(e: Env) -> Address {
        get_amm(&e)
    }

    fn swap_v_for_pt(e: Env, to: Address, pt_out: i128, v_in_max: i128) {
        to.require_auth();
        AmmClient::new(&e, &get_amm(&e)).swap_v_for_pt(&to, &pt_out, &v_in_max);
    }

    fn swap_pt_for_v(e: Env, to: Address, pt_in: i128, min_v_out: i128) {
        to.require_auth();
        AmmClient::new(&e, &get_amm(&e)).swap_pt_for_v(&to, &pt_in, &min_v_out);
    }

    fn deposit(e: Env, to: Address, desired_a: i128, min_a: i128, desired_b: i128, min_b: i128) {
        to.require_auth();
        AmmClient::new(&e, &get_amm(&e)).deposit(&to, &desired_a, &min_a, &desired_b, &min_b);
    }

    fn withdraw(e: Env, to: Address, share_amount: i128, min_a: i128, min_b: i128) -> (i128, i128) {
        to.require_auth();
        AmmClient::new(&e, &get_amm(&e)).withdraw(&to, &share_amount, &min_a, &min_b)
    }

    fn get_reserves(e: Env) -> (i128, i128) {
        AmmClient::new(&e, &get_amm(&e)).get_reserves()
    }

    fn balance_shares(e: Env, user: Address) -> i128 {
        AmmClient::new(&e, &get_amm(&e)).balance_shares(&user)
    }
}
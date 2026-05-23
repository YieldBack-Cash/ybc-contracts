use amm_interface::AmmClient;
use yield_manager_interface::YieldManagerClient;
use soroban_sdk::{contract, contractclient, contractimpl, Address, Env};

#[contractclient(name = "RouterClient")]
pub trait RouterInterface {
    fn get_amm(env: Env) -> Address;
    fn swap_v_for_pt(env: Env, to: Address, pt_out: i128, v_in_max: i128);
    fn swap_pt_for_v(env: Env, to: Address, pt_in: i128, min_v_out: i128);
    fn swap_v_for_yt(env: Env, to: Address, v_in: i128, min_yt_out: i128);
    fn swap_yt_for_v(env: Env, to: Address, yt_in: i128, min_v_out: i128);
    fn deposit(env: Env, to: Address, desired_a: i128, min_a: i128, desired_b: i128, min_b: i128);
    fn withdraw(env: Env, to: Address, share_amount: i128, min_a: i128, min_b: i128) -> (i128, i128);
    fn get_reserves(env: Env) -> (i128, i128);
    fn balance_shares(env: Env, user: Address) -> i128;
}

const AMM_KEY: &str = "amm";
const YM_KEY: &str = "ym";

fn get_amm(e: &Env) -> Address {
    e.storage().instance().get(&AMM_KEY).unwrap()
}

fn get_ym(e: &Env) -> Address {
    e.storage().instance().get(&YM_KEY).unwrap()
}

#[contract]
pub struct RouterContract;

#[contractimpl]
impl RouterContract {
    pub fn __constructor(e: Env, amm: Address, ym: Address) {
        e.storage().instance().set(&AMM_KEY, &amm);
        e.storage().instance().set(&YM_KEY, &ym);
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

    fn swap_v_for_yt(e: Env, to: Address, v_in: i128, min_yt_out: i128) {
        to.require_auth();
        assert!(v_in > 0, "v_in must be positive");
        assert!(min_yt_out > 0, "min_yt_out must be positive");

        let ym_client = YieldManagerClient::new(&e, &get_ym(&e));
        let exchange_rate = ym_client.get_exchange_rate();
        let pt_to_borrow = v_in * exchange_rate / 10_000_000;

        AmmClient::new(&e, &get_amm(&e))
            .flash_swap_pt(&get_ym(&e), &pt_to_borrow, &to, &v_in, &min_yt_out);
    }

    fn swap_yt_for_v(e: Env, to: Address, yt_in: i128, min_v_out: i128) {
        to.require_auth();
        assert!(yt_in > 0, "yt_in must be positive");
        assert!(min_v_out > 0, "min_v_out must be positive");

        // Borrow exactly `yt_in` PT so it pairs 1:1 with the user's YT in the YM redeem.
        // The YM is the callback receiver — it pulls YT from the user, burns PT+YT, and repays the AMM.
        AmmClient::new(&e, &get_amm(&e))
            .flash_swap_v(&get_ym(&e), &yt_in, &to, &min_v_out);
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
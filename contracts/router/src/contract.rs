use amm_interface::{AmmClient, FlashSwapReceiver, FlashSwapVReceiver};
use yield_manager_interface::YieldManagerClient;
use soroban_sdk::{contract, contractclient, contractimpl, token, Address, Env};

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
            .flash_swap_pt(&e.current_contract_address(), &pt_to_borrow, &to, &v_in, &min_yt_out);
    }

    fn swap_yt_for_v(e: Env, to: Address, yt_in: i128, min_v_out: i128) {
        to.require_auth();
        assert!(yt_in > 0, "yt_in must be positive");
        assert!(min_v_out > 0, "min_v_out must be positive");

        // Borrow exactly `yt_in` PT so it pairs 1:1 with the user's YT in the YM redeem.
        AmmClient::new(&e, &get_amm(&e))
            .flash_swap_v(&e.current_contract_address(), &yt_in, &to, &min_v_out);
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

#[contractimpl]
impl FlashSwapReceiver for RouterContract {
    /// Called by the AMM during a flash swap. Receives borrowed PT, deposits user V into the
    /// YM to mint PT + YT, sends YT to the user, then repays the AMM with all PT held.
    fn on_flash_receive(e: Env, pt_borrowed: i128, user: Address, v_in: i128, min_yt_out: i128) {
        // Only the AMM may invoke this callback.
        get_amm(&e).require_auth();

        let ym = get_ym(&e);
        let ym_client = YieldManagerClient::new(&e, &ym);
        let v_token = ym_client.get_vault();
        let pt_addr = ym_client.get_principal_token();
        let yt_addr = ym_client.get_yield_token();

        let v_client = token::Client::new(&e, &v_token);
        let pt_client = token::Client::new(&e, &pt_addr);
        let yt_client = token::Client::new(&e, &yt_addr);

        // Pull V from user into router.
        v_client.transfer(&user, &e.current_contract_address(), &v_in);

        // Deposit V → YM mints equal amounts of PT and YT to the router.
        ym_client.deposit(&e.current_contract_address(), &v_in);

        let exchange_rate = ym_client.get_exchange_rate();
        let yt_minted = v_in * exchange_rate / 10_000_000;

        assert!(yt_minted >= min_yt_out, "yt below minimum");

        // Send YT to user.
        yt_client.transfer(&e.current_contract_address(), &user, &yt_minted);

        // Repay AMM: return the borrowed PT plus all newly minted PT.
        let amm = get_amm(&e);
        pt_client.transfer(&e.current_contract_address(), &amm, &(pt_borrowed + yt_minted));

        // Post-conditions: router must hold no residual tokens.
        assert!(pt_client.balance(&e.current_contract_address()) == 0, "router leaked PT");
        assert!(yt_client.balance(&e.current_contract_address()) == 0, "router leaked YT");
        assert!(v_client.balance(&e.current_contract_address()) == 0, "router leaked V");
    }
}

#[contractimpl]
impl FlashSwapVReceiver for RouterContract {
    /// Called by the AMM during `flash_swap_v`. Receives `pt_borrowed` PT, pulls the user's
    /// matching YT, redeems both for vault shares via the YM, repays the AMM `v_owed` shares,
    /// and forwards the remainder to the user.
    fn on_flash_receive_v(e: Env, pt_borrowed: i128, v_owed: i128, user: Address, min_v_out: i128) {
        // Only the AMM may invoke this callback.
        get_amm(&e).require_auth();

        let router = e.current_contract_address();

        let ym = get_ym(&e);
        let ym_client = YieldManagerClient::new(&e, &ym);
        let v_token = ym_client.get_vault();
        let pt_addr = ym_client.get_principal_token();
        let yt_addr = ym_client.get_yield_token();

        let v_client = token::Client::new(&e, &v_token);
        let pt_client = token::Client::new(&e, &pt_addr);
        let yt_client = token::Client::new(&e, &yt_addr);

        // Pull the user's YT into the router; together with the borrowed PT this is a 1:1 pair.
        yt_client.transfer(&user, &router, &pt_borrowed);

        // Redeem PT + YT → vault shares. The YM burns `pt_borrowed` of each from the router.
        let v_before = v_client.balance(&router);
        ym_client.redeem(&router, &pt_borrowed);
        let v_total = v_client
            .balance(&router)
            .checked_sub(v_before)
            .expect("V balance went backwards");

        let v_to_user = v_total
            .checked_sub(v_owed)
            .expect("redeem yielded less V than owed to pool");
        assert!(v_to_user >= min_v_out, "v out below minimum");

        // Repay the AMM, then forward the remainder to the user.
        let amm = get_amm(&e);
        v_client.transfer(&router, &amm, &v_owed);
        v_client.transfer(&router, &user, &v_to_user);

        // Post-conditions: router must hold no residual tokens.
        assert!(pt_client.balance(&router) == 0, "router leaked PT");
        assert!(yt_client.balance(&router) == 0, "router leaked YT");
        assert!(v_client.balance(&router) == 0, "router leaked V");
    }
}
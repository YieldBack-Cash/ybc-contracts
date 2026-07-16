use amm_interface::AmmClient;
use soroban_sdk::{contract, contractclient, contractimpl, token, Address, Env};
use yield_manager_interface::YieldManagerClient;

use crate::events::{RoutedYtBuy, RoutedYtSell};
use crate::storage::{extend_instance_ttl, get_factory, set_factory};

/// Minimal view of the factory: just enough to resolve a vault's current market.
#[contractclient(name = "FactoryViewClient")]
pub trait FactoryView {
    fn get_current_pool(env: Env, vault: Address) -> Option<Address>;
    fn get_current_yield_manager(env: Env, vault: Address) -> Option<Address>;
}

#[contractclient(name = "RouterClient")]
pub trait RouterInterface {
    fn get_amm(env: Env, vault: Address) -> Address;
    fn swap_v_for_pt(env: Env, vault: Address, to: Address, pt_out: i128, v_in_max: i128);
    fn swap_pt_for_v(env: Env, vault: Address, to: Address, pt_in: i128, min_v_out: i128);
    fn swap_v_for_yt(env: Env, vault: Address, to: Address, yt_out: i128, max_v_in: i128);
    fn swap_yt_for_v(env: Env, vault: Address, to: Address, yt_in: i128, min_v_out: i128);
    fn deposit(
        env: Env,
        vault: Address,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    );
    fn withdraw(
        env: Env,
        vault: Address,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> (i128, i128);
    fn get_reserves(env: Env, vault: Address) -> (i128, i128);
    fn balance_shares(env: Env, vault: Address, user: Address) -> i128;
}

#[contract]
pub struct RouterContract;

/// Resolves the vault's current market through the factory, so callers can't
/// point the router at a pool the factory didn't deploy.
fn resolve_market(e: &Env, vault: &Address) -> (Address, Address) {
    let factory = FactoryViewClient::new(e, &get_factory(e));
    let amm = factory.get_current_pool(vault).expect("no pool for vault");
    let ym = factory
        .get_current_yield_manager(vault)
        .expect("no yield manager for vault");
    (amm, ym)
}

fn resolve_amm(e: &Env, vault: &Address) -> Address {
    FactoryViewClient::new(e, &get_factory(e))
        .get_current_pool(vault)
        .expect("no pool for vault")
}

#[contractimpl]
impl RouterContract {
    pub fn __constructor(e: Env, factory: Address) {
        set_factory(&e, &factory);
    }
}

#[contractimpl]
impl RouterInterface for RouterContract {
    fn get_amm(e: Env, vault: Address) -> Address {
        extend_instance_ttl(&e);
        resolve_amm(&e, &vault)
    }

    fn swap_v_for_pt(e: Env, vault: Address, to: Address, pt_out: i128, v_in_max: i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        AmmClient::new(&e, &resolve_amm(&e, &vault)).swap_v_for_pt(&to, &pt_out, &v_in_max);
    }

    fn swap_pt_for_v(e: Env, vault: Address, to: Address, pt_in: i128, min_v_out: i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        AmmClient::new(&e, &resolve_amm(&e, &vault)).swap_pt_for_v(&to, &pt_in, &min_v_out);
    }

    fn swap_v_for_yt(e: Env, vault: Address, to: Address, yt_out: i128, max_v_in: i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_out > 0, "yt_out must be positive");
        assert!(max_v_in > 0, "max_v_in must be positive");

        let (amm, ym) = resolve_market(&e, &vault);

        // Flash swap: the YM mints yt_out PT+YT, keeps the PT for the pool, and gives
        // the user the YT. The user pays only the YT price, bounded by max_v_in.
        AmmClient::new(&e, &amm).flash_swap_pt(&ym, &yt_out, &to, &max_v_in);

        RoutedYtBuy {
            vault,
            to,
            yt_out,
            max_v_in,
        }
        .publish(&e);
    }

    fn swap_yt_for_v(e: Env, vault: Address, to: Address, yt_in: i128, min_v_out: i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_in > 0, "yt_in must be positive");
        assert!(min_v_out > 0, "min_v_out must be positive");

        let (amm, ym) = resolve_market(&e, &vault);

        // Transfer the YT before the flash swap so the user's signed auth entry is a
        // plain transfer with fixed args — nothing that drifts with pool state.
        let yt = YieldManagerClient::new(&e, &ym).get_yield_token();
        token::TokenClient::new(&e, &yt).transfer(&to, &ym, &yt_in);

        // Borrow exactly `yt_in` PT so it pairs 1:1 with the YT now held by the YM.
        // The YM is the callback receiver — it burns PT+YT and repays the AMM.
        AmmClient::new(&e, &amm).flash_swap_v(&ym, &yt_in, &to, &min_v_out);

        RoutedYtSell {
            vault,
            to,
            yt_in,
            min_v_out,
        }
        .publish(&e);
    }

    fn deposit(
        e: Env,
        vault: Address,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        AmmClient::new(&e, &resolve_amm(&e, &vault)).deposit(
            &to, &desired_a, &min_a, &desired_b, &min_b,
        );
    }

    fn withdraw(
        e: Env,
        vault: Address,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> (i128, i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        AmmClient::new(&e, &resolve_amm(&e, &vault)).withdraw(&to, &share_amount, &min_a, &min_b)
    }

    fn get_reserves(e: Env, vault: Address) -> (i128, i128) {
        extend_instance_ttl(&e);
        AmmClient::new(&e, &resolve_amm(&e, &vault)).get_reserves()
    }

    fn balance_shares(e: Env, vault: Address, user: Address) -> i128 {
        extend_instance_ttl(&e);
        AmmClient::new(&e, &resolve_amm(&e, &vault)).balance_shares(&user)
    }
}
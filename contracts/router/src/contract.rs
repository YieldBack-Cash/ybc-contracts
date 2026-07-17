use amm_interface::AmmClient;
use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, token, Address, Env,
};
use yield_manager_interface::YieldManagerClient;
use yield_token_interface::YieldTokenClient;

use crate::events::{ExitedExpired, RoutedYtBuy, RoutedYtSell};
use crate::storage::{extend_instance_ttl, get_factory, set_factory};

/// Mirror of the factory's `Market` record. Field names and types must match
/// the factory's struct exactly so its return value decodes into this one.
#[contracttype]
#[derive(Clone)]
pub struct Market {
    pub ym: Address,
    pub pt: Address,
    pub yt: Address,
    pub pool: Address,
    pub maturity: u64,
    pub vault: Address,
}

/// Minimal view of the factory: just enough to resolve a single market.
/// The trait itself is never implemented or called — it exists only as the
/// source for the generated FactoryViewClient, which dead_code can't see.
#[allow(dead_code)]
#[contractclient(name = "FactoryViewClient")]
pub trait FactoryView {
    fn get_market(env: Env, vault: Address, maturity: u64) -> Option<Market>;
}

#[contractclient(name = "RouterClient")]
pub trait RouterInterface {
    fn get_amm(env: Env, vault: Address, maturity: u64) -> Address;
    fn swap_v_for_pt(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_out: i128,
        v_in_max: i128,
    );
    fn swap_pt_for_v(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_in: i128,
        min_v_out: i128,
    );
    fn swap_v_for_yt(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_out: i128,
        max_v_in: i128,
    );
    fn swap_yt_for_v(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_in: i128,
        min_v_out: i128,
    );
    fn deposit(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    );
    fn withdraw(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> (i128, i128);
    fn exit_expired(
        env: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        min_shares_out: i128,
    ) -> i128;
    fn get_reserves(env: Env, vault: Address, maturity: u64) -> (i128, i128);
    fn balance_shares(env: Env, vault: Address, maturity: u64, user: Address) -> i128;
}

#[contract]
pub struct RouterContract;

/// Resolves a market by (vault, maturity) through the factory, so callers can't
/// point the router at a pool the factory didn't deploy. The factory keys each
/// market directly by (vault, maturity) and forbids overwriting it, so this is a
/// single O(1) lookup rather than a scan of the vault's whole market history.
fn resolve_market(e: &Env, vault: &Address, maturity: u64) -> Market {
    FactoryViewClient::new(e, &get_factory(e))
        .get_market(vault, &maturity)
        .expect("no market for vault and maturity")
}

#[contractimpl]
impl RouterContract {
    pub fn __constructor(e: Env, factory: Address) {
        set_factory(&e, &factory);
    }
}

#[contractimpl]
impl RouterInterface for RouterContract {
    fn get_amm(e: Env, vault: Address, maturity: u64) -> Address {
        extend_instance_ttl(&e);
        resolve_market(&e, &vault, maturity).pool
    }

    fn swap_v_for_pt(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_out: i128,
        v_in_max: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).swap_v_for_pt(&to, &pt_out, &v_in_max);
    }

    fn swap_pt_for_v(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        pt_in: i128,
        min_v_out: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).swap_pt_for_v(&to, &pt_in, &min_v_out);
    }

    fn swap_v_for_yt(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_out: i128,
        max_v_in: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_out > 0, "yt_out must be positive");
        assert!(max_v_in > 0, "max_v_in must be positive");

        let market = resolve_market(&e, &vault, maturity);

        // Flash swap: the YM mints yt_out PT+YT, keeps the PT for the pool, and gives
        // the user the YT. The user pays only the YT price, bounded by max_v_in.
        AmmClient::new(&e, &market.pool).flash_swap_pt(&market.ym, &yt_out, &to, &max_v_in);

        RoutedYtBuy {
            vault,
            to,
            maturity,
            yt_out,
            max_v_in,
        }
        .publish(&e);
    }

    fn swap_yt_for_v(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        yt_in: i128,
        min_v_out: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(yt_in > 0, "yt_in must be positive");
        assert!(min_v_out > 0, "min_v_out must be positive");

        let market = resolve_market(&e, &vault, maturity);

        // Transfer the YT before the flash swap so the user's signed auth entry is a
        // plain transfer with fixed args — nothing that drifts with pool state.
        token::TokenClient::new(&e, &market.yt).transfer(&to, &market.ym, &yt_in);

        // Borrow exactly `yt_in` PT so it pairs 1:1 with the YT now held by the YM.
        // The YM is the callback receiver — it burns PT+YT and repays the AMM.
        AmmClient::new(&e, &market.pool).flash_swap_v(&market.ym, &yt_in, &to, &min_v_out);

        RoutedYtSell {
            vault,
            to,
            maturity,
            yt_in,
            min_v_out,
        }
        .publish(&e);
    }

    fn deposit(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    ) {
        to.require_auth();
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).deposit(&to, &desired_a, &min_a, &desired_b, &min_b);
    }

    fn withdraw(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> (i128, i128) {
        to.require_auth();
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).withdraw(&to, &share_amount, &min_a, &min_b)
    }

    /// One-call exit from an expired market: burns the user's LP position,
    /// redeems their entire PT balance at the YM's locked post-maturity rate,
    /// and sweeps yield the YT accrued before maturity — everything lands as
    /// vault shares. `min_shares_out` bounds the total delivered.
    fn exit_expired(
        e: Env,
        vault: Address,
        maturity: u64,
        to: Address,
        lp_shares: i128,
        min_shares_out: i128,
    ) -> i128 {
        to.require_auth();
        extend_instance_ttl(&e);
        assert!(lp_shares >= 0, "lp_shares must be non-negative");

        let market = resolve_market(&e, &vault, maturity);
        assert!(
            e.ledger().timestamp() >= market.maturity,
            "market not expired"
        );

        let vault_token = token::TokenClient::new(&e, &vault);
        let v_before = vault_token.balance(&to);

        // The pool pays out both legs (PT + vault shares) directly to the user.
        // Per-leg mins stay 0: PT converts at the YM's fixed rate below, so the
        // aggregate min_shares_out check at the end is the real slippage bound.
        if lp_shares > 0 {
            AmmClient::new(&e, &market.pool).withdraw(&to, &lp_shares, &0, &0);
        }

        // Redeem the full PT balance — the LP-withdrawn PT plus any the user
        // already held. Post-maturity PT has no other use.
        let pt_balance = token::TokenClient::new(&e, &market.pt).balance(&to);
        if pt_balance > 0 {
            YieldManagerClient::new(&e, &market.ym).redeem_principal(&to, &pt_balance);
        }

        YieldTokenClient::new(&e, &market.yt).claim_yield(&to);

        let shares_out = vault_token.balance(&to) - v_before;
        assert!(shares_out >= min_shares_out, "min_shares_out not satisfied");

        ExitedExpired {
            vault,
            to,
            maturity,
            lp_shares,
            pt_redeemed: pt_balance,
            shares_out,
        }
        .publish(&e);

        shares_out
    }

    fn get_reserves(e: Env, vault: Address, maturity: u64) -> (i128, i128) {
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).get_reserves()
    }

    fn balance_shares(e: Env, vault: Address, maturity: u64, user: Address) -> i128 {
        extend_instance_ttl(&e);
        let market = resolve_market(&e, &vault, maturity);
        AmmClient::new(&e, &market.pool).balance_shares(&user)
    }
}
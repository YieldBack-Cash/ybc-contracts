//! The global router serves every market by resolving the vault's *current*
//! market through the factory on each call. These tests pin the three claims
//! that design makes:
//!
//! 1. Swaps on one market never touch another market's pool.
//! 2. After a rollover, the router routes to the new market and leaves the
//!    expired one untouched.
//! 3. A vault the factory never deployed for reverts instead of routing.

use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, Symbol};

use amm::LiquidityPoolClient;

use super::fixture::{IntegrationFixture, ONE_YEAR_SECS};

const POOL_PT: i128 = 50_000_000;
const POOL_V: i128 = 50_000_000;
const YM_DEPOSIT: i128 = 100_000_000;

/// Primary market seeded exactly like the router_swaps suite.
fn seeded<'a>(env: &'a Env) -> IntegrationFixture<'a> {
    let f = IntegrationFixture::new(env);
    f.vault.set_exchange_rate(&10_000_000);
    f.ym_deposit(&f.user, YM_DEPOSIT);
    f.amm_deposit(&f.user, POOL_PT, POOL_V);
    f
}

#[test]
fn test_router_isolates_markets() {
    let env = Env::default();
    let f = seeded(&env);

    // Second vault with its own market, seeded identically.
    let (vault_b, market_b) = f.create_market_for_new_vault("MVB");
    f.ym_deposit_to(&vault_b, &market_b.ym, &f.user, YM_DEPOSIT);
    f.amm_deposit_to(&vault_b, &market_b.pt, &market_b.pool, &f.user, POOL_PT, POOL_V);

    let pool_b = LiquidityPoolClient::new(&env, &market_b.pool);
    assert_ne!(f.pool.address, market_b.pool, "each market gets its own pool");

    // Swap on market A: only A's reserves move.
    let a_before = f.pool.get_reserves();
    let b_before = pool_b.get_reserves();
    f.router_swap_v_for_yt(&f.user, 1_000_000, 1_000_000);
    assert_ne!(f.pool.get_reserves(), a_before, "market A pool traded");
    assert_eq!(pool_b.get_reserves(), b_before, "market B pool untouched by market A swap");

    // Swap on market B: only B's reserves move.
    let a_mid = f.pool.get_reserves();
    f.router_swap_v_for_yt_on(&vault_b, &f.user, 1_000_000, 1_000_000);
    assert_ne!(pool_b.get_reserves(), b_before, "market B pool traded");
    assert_eq!(f.pool.get_reserves(), a_mid, "market A pool untouched by market B swap");

    // The YT landed in each market's own token.
    let yt_b_balance = env.invoke_contract::<i128>(
        &market_b.yt,
        &Symbol::new(&env, "balance"),
        (&f.user,).into_val(&env),
    );
    assert_eq!(yt_b_balance, YM_DEPOSIT + 1_000_000, "market B YT credited from market B swap");
}

#[test]
fn test_router_follows_rollover() {
    let env = Env::default();
    let f = seeded(&env);
    let old_pool_addr = f.pool.address.clone();

    // Expire the market and roll it over.
    f.advance_time(ONE_YEAR_SECS + 1);
    let new_maturity = env.ledger().timestamp() + ONE_YEAR_SECS;
    assert!(f.rollover(&f.vault.address, new_maturity), "rollover must run");

    // The router now resolves the vault to the *new* pool.
    let resolved = env.invoke_contract::<Address>(
        &f.router,
        &Symbol::new(&env, "get_amm"),
        (&f.vault.address,).into_val(&env),
    );
    assert_ne!(resolved, old_pool_addr, "router no longer routes to the expired pool");
    assert_eq!(
        resolved,
        f.factory.get_current_pool(&f.vault.address).unwrap(),
        "router routes to the factory's current pool"
    );

    // Seed the new market and swap through the router: the new pool trades,
    // the expired pool's reserves never move.
    let new_ym = f.factory.get_current_yield_manager(&f.vault.address).unwrap();
    let new_pt = f.factory.get_current_pt_token(&f.vault.address).unwrap();
    f.ym_deposit_to(&f.vault.address, &new_ym, &f.user, YM_DEPOSIT);
    f.amm_deposit_to(&f.vault.address, &new_pt, &resolved, &f.user, POOL_PT, POOL_V);

    let old_reserves = f.pool.get_reserves();
    let new_pool = LiquidityPoolClient::new(&env, &resolved);
    let new_before = new_pool.get_reserves();

    f.router_swap_v_for_yt(&f.user, 1_000_000, 1_000_000);

    assert_ne!(new_pool.get_reserves(), new_before, "post-rollover swap trades the new pool");
    assert_eq!(f.pool.get_reserves(), old_reserves, "expired pool untouched after rollover");
}

#[test]
#[should_panic]
fn test_router_unknown_vault_reverts() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    // An address the factory never deployed a market for.
    let stranger = Address::generate(&env);
    env.invoke_contract::<Address>(
        &f.router,
        &Symbol::new(&env, "get_amm"),
        (&stranger,).into_val(&env),
    );
}
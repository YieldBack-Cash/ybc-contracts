//! The global router serves every market by resolving (vault, maturity)
//! against the factory on each call. These tests pin the claims that design
//! makes:
//!
//! 1. Swaps on one market never touch another market's pool.
//! 2. A single vault can host several markets at different maturities at once,
//!    and each stays addressable by its maturity — including after one expires.
//! 3. A (vault, maturity) pair the factory never deployed for reverts
//!    instead of routing.

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
    f.router_swap_v_for_yt_on(&vault_b, market_b.maturity, &f.user, 1_000_000, 1_000_000);
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
fn test_markets_stay_addressable_by_maturity_after_one_expires() {
    let env = Env::default();
    let f = seeded(&env);
    let old_pool_addr = f.pool.address.clone();

    // A second, longer-dated market on the same vault, created while the first
    // is still active.
    let later = f.maturity + ONE_YEAR_SECS;
    let market2 = f.create_market_on_vault(&f.vault.address, later);
    f.ym_deposit_to(&f.vault.address, &market2.ym, &f.user, YM_DEPOSIT);
    f.amm_deposit_to(&f.vault.address, &market2.pt, &market2.pool, &f.user, POOL_PT, POOL_V);

    // Let the first market expire; the second is still active.
    f.advance_time(ONE_YEAR_SECS + 1);

    // Both remain addressable through the router by their maturities.
    let resolved_old = env.invoke_contract::<Address>(
        &f.router,
        &Symbol::new(&env, "get_amm"),
        (&f.vault.address, f.maturity).into_val(&env),
    );
    assert_eq!(resolved_old, old_pool_addr, "expired market still resolves by its maturity");

    let resolved_new = env.invoke_contract::<Address>(
        &f.router,
        &Symbol::new(&env, "get_amm"),
        (&f.vault.address, later).into_val(&env),
    );
    assert_eq!(resolved_new, market2.pool, "later maturity resolves to its own pool");

    // Trade the still-active market; the expired pool's reserves never move.
    let old_reserves = f.pool.get_reserves();
    let new_pool = LiquidityPoolClient::new(&env, &market2.pool);
    let new_before = new_pool.get_reserves();

    f.router_swap_v_for_yt_on(&f.vault.address, later, &f.user, 1_000_000, 1_000_000);

    assert_ne!(new_pool.get_reserves(), new_before, "swap trades the active market");
    assert_eq!(f.pool.get_reserves(), old_reserves, "expired pool untouched");
}

#[test]
fn test_two_concurrent_markets_same_vault_different_maturities() {
    let env = Env::default();
    let f = seeded(&env); // primary market at f.maturity, already seeded

    // A second market on the SAME vault at a later maturity. Both are unexpired,
    // so they are active at the same time.
    let later = f.maturity + ONE_YEAR_SECS;
    let market2 = f.create_market_on_vault(&f.vault.address, later);
    assert_ne!(f.pool.address, market2.pool, "second market gets its own pool");
    assert_ne!(f.yield_manager, market2.ym, "second market gets its own YM");

    // Seed the second market too.
    f.ym_deposit_to(&f.vault.address, &market2.ym, &f.user, YM_DEPOSIT);
    f.amm_deposit_to(&f.vault.address, &market2.pt, &market2.pool, &f.user, POOL_PT, POOL_V);

    let pool2 = LiquidityPoolClient::new(&env, &market2.pool);
    let a_before = f.pool.get_reserves();
    let b_before = pool2.get_reserves();

    // Trade the first market via the router: only its pool moves.
    f.router_swap_v_for_yt(&f.user, 1_000_000, 1_000_000);
    assert_ne!(f.pool.get_reserves(), a_before, "first market traded");
    assert_eq!(pool2.get_reserves(), b_before, "second market untouched by first-market swap");

    // Trade the second market via the router: only its pool moves.
    let a_mid = f.pool.get_reserves();
    f.router_swap_v_for_yt_on(&f.vault.address, later, &f.user, 1_000_000, 1_000_000);
    assert_ne!(pool2.get_reserves(), b_before, "second market traded");
    assert_eq!(f.pool.get_reserves(), a_mid, "first market untouched by second-market swap");
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
        (&stranger, f.maturity).into_val(&env),
    );
}

#[test]
#[should_panic]
fn test_router_unknown_maturity_reverts() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    // A real vault, but no market ever existed at this maturity.
    env.invoke_contract::<Address>(
        &f.router,
        &Symbol::new(&env, "get_amm"),
        (&f.vault.address, f.maturity + 1).into_val(&env),
    );
}
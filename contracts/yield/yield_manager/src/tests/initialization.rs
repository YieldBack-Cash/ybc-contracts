use soroban_sdk::{testutils::Address as _, Address, IntoVal, Symbol};

use super::fixture::YieldManagerTest;

#[test]
fn test_initialization() {
    let test = YieldManagerTest::setup();

    let vault_addr: Address = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_vault"),
        ().into_val(&test.env),
    );
    assert_eq!(vault_addr, test.vault_addr);

    let maturity: u64 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_maturity"),
        ().into_val(&test.env),
    );
    assert_eq!(maturity, test.maturity);
}

// ── the one-shot setters stay one-shot ───────────────────────────────────────
//
// The rate round-trip in the flash callbacks is safe *because* the pool can
// never be re-pointed (SECURITY.md §3 trust table; threat model §4.3): the figure
// the pool hands back is this contract's own only while `set_pool` cannot be
// called a second time. Likewise the token pair. These tests call each setter
// again, as the admin, and expect the typed refusal; remove either guard and
// its test fails.

#[test]
#[should_panic(expected = "Error(Contract, #7)")] // PoolAlreadySet
fn set_pool_cannot_be_called_twice() {
    let test = YieldManagerTest::setup();
    let another_pool = Address::generate(&test.env);

    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "set_pool"),
        (&another_pool,).into_val(&test.env),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")] // AlreadyInitialized
fn set_token_contracts_cannot_be_called_twice() {
    let test = YieldManagerTest::setup();
    let pt = Address::generate(&test.env);
    let yt = Address::generate(&test.env);

    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "set_token_contracts"),
        (&pt, &yt).into_val(&test.env),
    );
}

/// And the pool the fixture registered is the one every callback trusts.
#[test]
fn the_registered_pool_is_the_only_trusted_receiver() {
    let test = YieldManagerTest::setup();
    let pool: Address = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_pool"),
        ().into_val(&test.env),
    );
    assert_eq!(pool, test.pool);
}

use soroban_sdk::{IntoVal, Symbol};

use super::fixture::YieldManagerTest;

#[test]
fn test_exchange_rate_increases_over_time() {
    let test = YieldManagerTest::setup();

    let initial_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    test.set_vault_exchange_rate(1_200_0000); // 1.0 → 1.2

    let new_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    assert!(new_rate > initial_rate);
}

#[test]
fn test_exchange_rate_high_water_mark() {
    let test = YieldManagerTest::setup();

    let initial_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    test.set_vault_exchange_rate(1_500_0000); // raise to 1.5

    let higher_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    assert!(higher_rate > initial_rate);

    test.set_vault_exchange_rate(1_200_0000); // drop to 1.2 (simulated loss)

    let rate_after_decrease: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Should be locked at high water mark (1.5), not the decreased rate (1.2)
    assert_eq!(rate_after_decrease, higher_rate);
    assert!(rate_after_decrease > 1_200_0000);
}

#[test]
fn test_exchange_rate_locks_at_maturity() {
    let test = YieldManagerTest::setup();

    test.set_vault_exchange_rate(1_200_0000); // 1.2x before maturity

    test.advance_time(500); // halfway to maturity

    let rate_before_maturity: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    test.advance_time(600); // past maturity (500 + 600 > 1000)

    let rate_at_maturity: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    assert_eq!(rate_at_maturity, rate_before_maturity);

    test.set_vault_exchange_rate(1_500_0000); // additional yield after maturity
    test.advance_time(1000);

    let rate_after_maturity: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Should still be locked at 1.2x, not updated to 1.5x
    assert_eq!(rate_after_maturity, rate_at_maturity);
}
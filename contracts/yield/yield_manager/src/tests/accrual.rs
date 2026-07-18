use soroban_sdk::{IntoVal, Symbol};

use super::fixture::YieldManagerTest;

#[test]
fn test_yt_accrues_yield_over_time() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    let initial_accrued: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "accrued_yield"),
        (&test.user1,).into_val(&test.env),
    );
    assert_eq!(initial_accrued, 0);

    test.set_vault_exchange_rate(1_200_0000); // 1.0 → 1.2

    let claimed: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "claim_yield"),
        (&test.user1,).into_val(&test.env),
    );

    assert!(claimed > 0);

    let user_vault_balance = test.vault_balance(&test.user1);
    assert_eq!(user_vault_balance, claimed);
}

#[test]
fn test_yield_distribution_proportional() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;

    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    test.mint_vault_shares(&test.user2, shares);
    test.deposit(&test.user2.clone(), shares);

    test.set_vault_exchange_rate(1_200_0000);

    let claimed1: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "claim_yield"),
        (&test.user1,).into_val(&test.env),
    );

    let claimed2: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "claim_yield"),
        (&test.user2,).into_val(&test.env),
    );

    // Equal deposits → equal yield (within 1% tolerance)
    let diff = if claimed1 > claimed2 { claimed1 - claimed2 } else { claimed2 - claimed1 };
    assert!(diff < claimed1 / 100);
}
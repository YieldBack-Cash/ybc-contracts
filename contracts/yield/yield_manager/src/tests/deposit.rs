use soroban_sdk::{IntoVal, Symbol};
use yield_manager_interface::YieldManagerError;

use super::fixture::YieldManagerTest;

#[test]
fn test_deposit_mints_pt_and_yt() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    let pt_balance = test.get_pt_balance(&test.user1);
    let yt_balance = test.get_yt_balance(&test.user1);

    // mint_amount = shares * exchange_rate(1.0) = shares
    assert_eq!(pt_balance, shares);
    assert_eq!(yt_balance, shares);

    let ym_vault_balance = test.vault_balance(&test.yield_manager);
    assert_eq!(ym_vault_balance, shares);
}

#[test]
fn test_multiple_users_deposit() {
    let test = YieldManagerTest::setup();

    let shares1 = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares1);
    test.deposit(&test.user1.clone(), shares1);

    let shares2 = 2_000_0000i128;
    test.mint_vault_shares(&test.user2, shares2);
    test.deposit(&test.user2.clone(), shares2);

    let pt1 = test.get_pt_balance(&test.user1);
    let pt2 = test.get_pt_balance(&test.user2);

    assert!(pt2 > pt1);
    assert!(pt2 >= pt1 * 2 - 100); // Allow some rounding
}

#[test]
fn test_deposit_after_maturity_reverts() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);

    test.advance_time(1100); // past maturity (maturity = 1000s from start)

    let result = test.env.try_invoke_contract::<(), YieldManagerError>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares).into_val(&test.env),
    );

    assert_eq!(result, Err(Ok(YieldManagerError::MaturityReached)));
}
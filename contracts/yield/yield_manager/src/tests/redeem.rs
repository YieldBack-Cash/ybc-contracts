use soroban_sdk::{IntoVal, Symbol};
use yield_manager_interface::YieldManagerError;

use super::fixture::YieldManagerTest;

#[test]
fn test_cannot_redeem_principal_before_maturity() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    let pt_balance = test.get_pt_balance(&test.user1);

    let result = test.env.try_invoke_contract::<(), YieldManagerError>(
        &test.yield_manager,
        &Symbol::new(&test.env, "redeem_principal"),
        (&test.user1, pt_balance).into_val(&test.env),
    );

    assert_eq!(result, Err(Ok(YieldManagerError::MaturityNotReached)));
}

#[test]
fn test_redeem_principal_after_maturity() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    let pt_balance = test.get_pt_balance(&test.user1);

    test.advance_time(1100); // past maturity (maturity = 1000s from start)

    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "redeem_principal"),
        (&test.user1, pt_balance).into_val(&test.env),
    );

    assert_eq!(test.get_pt_balance(&test.user1), 0);
    assert!(test.vault_balance(&test.user1) > 0);
}
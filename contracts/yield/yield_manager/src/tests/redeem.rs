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

#[test]
fn test_late_redeem_principal_pays_face_value_only() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares); // rate 1.0 -> pt = 1_000_0000

    // Vault doubles by maturity; lock the rate at 2.0.
    test.set_vault_exchange_rate(2_000_0000);
    test.advance_time(1100);
    test.env.invoke_contract::<i128>(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Vault keeps appreciating after the lock: 2.0 -> 4.0.
    test.set_vault_exchange_rate(4_000_0000);

    let pt_balance = test.get_pt_balance(&test.user1);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "redeem_principal"),
        (&test.user1, pt_balance).into_val(&test.env),
    );

    // Face value at the live rate: 1_000_0000 * 1e7 / 4_000_0000 shares.
    // At the locked rate the user would have gotten 500_0000 shares; the
    // post-maturity appreciation stays in the YM as protocol surplus.
    assert_eq!(test.vault_balance(&test.user1), 250_0000);
    assert_eq!(test.vault_balance(&test.yield_manager), shares - 250_0000);
}

#[test]
fn test_redeem_principal_floors_at_locked_rate_on_vault_dip() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares); // rate 1.0 -> pt = 1_000_0000

    // Lock the rate at 2.0.
    test.set_vault_exchange_rate(2_000_0000);
    test.advance_time(1100);
    test.env.invoke_contract::<i128>(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Vault suffers a loss after the lock: 2.0 -> 1.0. The divisor must
    // floor at the locked rate, not pay out extra shares at the dipped rate.
    test.set_vault_exchange_rate(1_000_0000);

    let pt_balance = test.get_pt_balance(&test.user1);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "redeem_principal"),
        (&test.user1, pt_balance).into_val(&test.env),
    );

    // 1_000_0000 * 1e7 / 2_000_0000 (locked), not / 1_000_0000 (live).
    assert_eq!(test.vault_balance(&test.user1), 500_0000);
}
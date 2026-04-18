use soroban_sdk::{IntoVal, Symbol};

use super::fixture::YieldManagerTest;

#[test]
fn test_pt_transferable() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    let pt_balance = test.get_pt_balance(&test.user1);
    let transfer_amount = pt_balance / 2;

    test.env.invoke_contract::<()>(
        &test.pt,
        &Symbol::new(&test.env, "transfer"),
        (&test.user1, &test.user2, transfer_amount).into_val(&test.env),
    );

    assert_eq!(test.get_pt_balance(&test.user1), pt_balance - transfer_amount);
    assert_eq!(test.get_pt_balance(&test.user2), transfer_amount);
}

#[test]
fn test_yt_transferable() {
    let test = YieldManagerTest::setup();

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    let yt_balance = test.get_yt_balance(&test.user1);
    let transfer_amount = yt_balance / 2;

    test.env.invoke_contract::<()>(
        &test.yt,
        &Symbol::new(&test.env, "transfer"),
        (&test.user1, &test.user2, transfer_amount).into_val(&test.env),
    );

    assert_eq!(test.get_yt_balance(&test.user1), yt_balance - transfer_amount);
    assert_eq!(test.get_yt_balance(&test.user2), transfer_amount);
}
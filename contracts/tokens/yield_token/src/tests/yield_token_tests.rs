use super::YieldTokenTest;
use soroban_sdk::{
    testutils::{MockAuth, MockAuthInvoke},
    IntoVal, String,
};

#[test]
fn test_initialization() {
    let test = YieldTokenTest::setup();

    let name = test.get_name();
    assert_eq!(name, String::from_str(&test.env, "Yield Token"));

    let symbol = test.get_symbol();
    assert_eq!(symbol, String::from_str(&test.env, "YT"));

    let decimals = test.get_decimals();
    assert_eq!(decimals, 7u32);
}

#[test]
fn test_mint_sets_initial_index() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000i128;
    let exchange_rate = 1_000_000i128;

    test.mint_yt(&test.user1, mint_amount, exchange_rate);

    let balance = test.get_balance(&test.user1);
    assert_eq!(balance, mint_amount);

    let user_index = test.get_user_index(&test.user1);
    assert_eq!(user_index, exchange_rate);
}

#[test]
fn test_yield_accrues_when_exchange_rate_increases() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128; // 1M tokens scaled by 1e6
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    let initial_accrued = test.get_accrued_yield(&test.user1);
    assert_eq!(initial_accrued, 0);

    let new_rate = initial_rate + 100_0000; // Increase by 0.01 (scaled by 1e7)
    test.set_vault_exchange_rate(new_rate);

    let fetched_rate = test.get_exchange_rate();
    assert!(fetched_rate > initial_rate, "Exchange rate should increase");

    let claimed = test.claim_yield(&test.user1);

    assert!(claimed > 0, "Should have claimed some yield");

    let vault_balance = test.vault_client.balance(&test.user1);
    assert_eq!(vault_balance, claimed);
}

#[test]
fn test_user_index_updates_after_accrual() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    let initial_index = test.get_user_index(&test.user1);
    assert_eq!(initial_index, initial_rate);

    let new_rate = initial_rate + 200_0000; // Increase by 0.02 (scaled by 1e7)
    test.set_vault_exchange_rate(new_rate);

    test.claim_yield(&test.user1);

    let updated_index = test.get_user_index(&test.user1);
    assert_eq!(updated_index, new_rate);
}

#[test]
fn test_multiple_claims_accumulate_yield() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    let rate1 = initial_rate + 100_0000;
    test.set_vault_exchange_rate(rate1);
    let claimed1 = test.claim_yield(&test.user1);
    assert!(claimed1 > 0);

    let rate2 = rate1 + 100_0000;
    test.set_vault_exchange_rate(rate2);
    let claimed2 = test.claim_yield(&test.user1);
    assert!(claimed2 > 0);

    let total_vault_balance = test.vault_client.balance(&test.user1);
    assert_eq!(total_vault_balance, claimed1 + claimed2);
}

#[test]
fn test_transfer_accrues_yield_for_both_parties() {
    let test = YieldTokenTest::setup();

    let mint_amount = 2_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    let new_rate = initial_rate + 100_0000;
    test.set_vault_exchange_rate(new_rate);

    let transfer_amount = 1_000_000_000_000i128;
    test.transfer(&test.user1, &test.user2, transfer_amount);

    let accrued1 = test.get_accrued_yield(&test.user1);
    assert!(accrued1 > 0);

    let user2_index = test.get_user_index(&test.user2);
    assert_eq!(user2_index, new_rate);

    assert_eq!(test.get_balance(&test.user1), mint_amount - transfer_amount);
    assert_eq!(test.get_balance(&test.user2), transfer_amount);
}

/// A holder must be able to move YT with only their own signature -- unlike
/// transfer_with_rate/burn_with_rate, plain transfer never trusts a
/// caller-supplied rate (it always fetches the real rate from the yield
/// manager), so it must not require the yield manager's auth too.
#[test]
fn test_transfer_needs_only_sender_auth_and_still_accrues_yield() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    let new_rate = initial_rate + 100_0000;
    test.set_vault_exchange_rate(new_rate);

    // Restrict auth to exactly what a real user transfer provides: the
    // sender's own signature. The yield manager never signs a plain transfer.
    let transfer_amount = 500_000_000_000i128;
    test.env.mock_auths(&[MockAuth {
        address: &test.user1,
        invoke: &MockAuthInvoke {
            contract: &test.yield_token,
            fn_name: "transfer",
            args: (&test.user1, &test.user2, transfer_amount).into_val(&test.env),
            sub_invokes: &[],
        },
    }]);

    test.transfer(&test.user1, &test.user2, transfer_amount);

    assert_eq!(test.get_balance(&test.user1), mint_amount - transfer_amount);
    assert_eq!(test.get_balance(&test.user2), transfer_amount);

    assert!(test.get_accrued_yield(&test.user1) > 0);
    assert_eq!(test.get_user_index(&test.user1), new_rate);
    assert_eq!(test.get_user_index(&test.user2), new_rate);
}

#[test]
fn test_transfer_to_existing_user_preserves_index() {
    let test = YieldTokenTest::setup();

    let initial_rate = test.get_exchange_rate();

    test.mint_yt(&test.user1, 1_000_000_000_000i128, initial_rate);
    test.mint_yt(&test.user2, 1_000_000_000_000i128, initial_rate);

    let new_rate = initial_rate + 100_0000;
    test.set_vault_exchange_rate(new_rate);

    test.transfer(&test.user1, &test.user2, 500_000_000_000i128);

    let accrued1 = test.get_accrued_yield(&test.user1);
    let accrued2 = test.get_accrued_yield(&test.user2);

    assert!(accrued1 > 0);
    assert!(accrued2 > 0);

    assert_eq!(test.get_user_index(&test.user1), new_rate);
    assert_eq!(test.get_user_index(&test.user2), new_rate);
}

#[test]
fn test_burn_accrues_yield_before_burning() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    let new_rate = initial_rate + 100_0000;
    test.set_vault_exchange_rate(new_rate);

    let burn_amount = 500_000_000_000i128;
    test.burn(&test.user1, burn_amount);

    let accrued = test.get_accrued_yield(&test.user1);
    assert!(accrued > 0);

    let balance = test.get_balance(&test.user1);
    assert_eq!(balance, mint_amount - burn_amount);

    let total_supply = test.get_total_supply();
    assert_eq!(total_supply, mint_amount - burn_amount);
}

#[test]
fn test_no_yield_if_rate_unchanged() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    let claimed = test.claim_yield(&test.user1);

    assert_eq!(claimed, 0);

    let vault_balance = test.vault_client.balance(&test.user1);
    assert_eq!(vault_balance, 0);
}

#[test]
fn test_proportional_yield_distribution() {
    let test = YieldTokenTest::setup();

    let initial_rate = test.get_exchange_rate();

    test.mint_yt(&test.user1, 2_000_000_000_000i128, initial_rate);
    test.mint_yt(&test.user2, 1_000_000_000_000i128, initial_rate);

    let new_rate = initial_rate + 100_0000;
    test.set_vault_exchange_rate(new_rate);

    let claimed1 = test.claim_yield(&test.user1);
    let claimed2 = test.claim_yield(&test.user2);

    assert!(claimed1 > 0);
    assert!(claimed2 > 0);

    // Allow 1% tolerance for rounding
    let ratio = claimed1 * 100 / claimed2;
    assert!(ratio >= 190 && ratio <= 210, "Ratio should be ~200, got {}", ratio);
}

#[test]
fn test_mint_to_existing_user_preserves_high_water_mark() {
    let test = YieldTokenTest::setup();

    let initial_rate = test.get_exchange_rate();

    test.mint_yt(&test.user1, 1_000_000_000_000i128, initial_rate);

    let new_rate = initial_rate + 100_0000;
    test.set_vault_exchange_rate(new_rate);

    test.mint_yt(&test.user1, 1_000_000_000_000i128, new_rate);

    let user_index = test.get_user_index(&test.user1);
    assert_eq!(user_index, new_rate);

    let accrued = test.get_accrued_yield(&test.user1);
    assert!(accrued > 0);
}

#[test]
fn test_sep41_balance_function() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000i128;
    let exchange_rate = 1_000_000i128;

    test.mint_yt(&test.user1, mint_amount, exchange_rate);

    let balance = test.get_balance(&test.user1);
    assert_eq!(balance, mint_amount);
}

#[test]
fn test_total_supply_tracking() {
    let test = YieldTokenTest::setup();

    let initial_supply = test.get_total_supply();
    assert_eq!(initial_supply, 0);

    let mint_amount = 1_000_000i128;
    let exchange_rate = 1_000_000i128;

    test.mint_yt(&test.user1, mint_amount, exchange_rate);
    assert_eq!(test.get_total_supply(), mint_amount);

    test.mint_yt(&test.user2, mint_amount, exchange_rate);
    assert_eq!(test.get_total_supply(), mint_amount * 2);
}

#[test]
#[should_panic(expected = "Insufficient balance")]
fn test_transfer_insufficient_balance() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000i128;
    let exchange_rate = 1_000_000i128;
    test.mint_yt(&test.user1, mint_amount, exchange_rate);

    test.transfer(&test.user1, &test.user2, mint_amount + 1);
}

#[test]
#[should_panic(expected = "Insufficient balance")]
fn test_burn_insufficient_balance() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000i128;
    let exchange_rate = 1_000_000i128;
    test.mint_yt(&test.user1, mint_amount, exchange_rate);

    test.burn(&test.user1, mint_amount + 1);
}

#[test]
fn test_zero_balance_user_can_claim() {
    let test = YieldTokenTest::setup();

    let claimed = test.claim_yield(&test.user1);
    assert_eq!(claimed, 0);
}

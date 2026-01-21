use super::YieldTokenTest;
use soroban_sdk::String;

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

    // Mint YT at current rate
    let mint_amount = 1_000_000_000_000i128; // 1M tokens scaled by 1e6
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // Check no yield initially
    let initial_accrued = test.get_accrued_yield(&test.user1);
    assert_eq!(initial_accrued, 0);

    // Advance time to increase exchange rate
    // The exact time depends on your vault's yield rate
    test.advance_time(100);

    // Verify exchange rate increased
    let new_rate = test.get_exchange_rate();
    assert!(new_rate > initial_rate, "Exchange rate should increase");

    // Trigger yield accrual by claiming
    let claimed = test.claim_yield(&test.user1);

    // User should have received some yield
    assert!(claimed > 0, "Should have claimed some yield");

    // User should have vault shares now
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

    // Increase rate by advancing time
    test.advance_time(200);
    let new_rate = test.get_exchange_rate();

    // Claim to trigger accrual
    test.claim_yield(&test.user1);

    // User index should be updated
    let updated_index = test.get_user_index(&test.user1);
    assert_eq!(updated_index, new_rate);
}

#[test]
fn test_multiple_claims_accumulate_yield() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // First increase
    test.advance_time(100);
    let claimed1 = test.claim_yield(&test.user1);
    assert!(claimed1 > 0);

    // Second increase
    test.advance_time(100);
    let claimed2 = test.claim_yield(&test.user1);
    assert!(claimed2 > 0);

    // Total vault shares received
    let total_vault_balance = test.vault_client.balance(&test.user1);
    assert_eq!(total_vault_balance, claimed1 + claimed2);
}

#[test]
fn test_transfer_accrues_yield_for_both_parties() {
    let test = YieldTokenTest::setup();

    // User1 gets YT
    let mint_amount = 2_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // Increase rate before transfer
    test.advance_time(100);
    let new_rate = test.get_exchange_rate();

    // Transfer to user2
    let transfer_amount = 1_000_000_000_000i128;
    test.transfer(&test.user1, &test.user2, transfer_amount);

    // Both users should have yield accrued from the rate increase
    let accrued1 = test.get_accrued_yield(&test.user1);

    // User1 should have accrued yield on their full balance before transfer
    assert!(accrued1 > 0);

    // User2 is new, so their index should be set to current rate
    let user2_index = test.get_user_index(&test.user2);
    assert_eq!(user2_index, new_rate);

    // Balances should be correct
    assert_eq!(test.get_balance(&test.user1), mint_amount - transfer_amount);
    assert_eq!(test.get_balance(&test.user2), transfer_amount);
}

#[test]
fn test_transfer_to_existing_user_preserves_index() {
    let test = YieldTokenTest::setup();

    let initial_rate = test.get_exchange_rate();

    // Both users get YT
    test.mint_yt(&test.user1, 1_000_000_000_000i128, initial_rate);
    test.mint_yt(&test.user2, 1_000_000_000_000i128, initial_rate);

    // Increase rate
    test.advance_time(100);
    let new_rate = test.get_exchange_rate();

    // Transfer from user1 to user2
    test.transfer(&test.user1, &test.user2, 500_000_000_000i128);

    // Both should have accrued yield
    let accrued1 = test.get_accrued_yield(&test.user1);
    let accrued2 = test.get_accrued_yield(&test.user2);

    assert!(accrued1 > 0);
    assert!(accrued2 > 0);

    // Both indices should be updated to new rate
    assert_eq!(test.get_user_index(&test.user1), new_rate);
    assert_eq!(test.get_user_index(&test.user2), new_rate);
}

#[test]
fn test_burn_accrues_yield_before_burning() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // Increase rate
    test.advance_time(100);

    // Burn tokens
    let burn_amount = 500_000_000_000i128;
    test.burn(&test.user1, burn_amount);

    // Check yield was accrued before burn
    let accrued = test.get_accrued_yield(&test.user1);
    assert!(accrued > 0);

    // Balance should be reduced
    let balance = test.get_balance(&test.user1);
    assert_eq!(balance, mint_amount - burn_amount);

    // Total supply should be reduced
    let total_supply = test.get_total_supply();
    assert_eq!(total_supply, mint_amount - burn_amount);
}

#[test]
fn test_no_yield_if_rate_unchanged() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // Claim without changing rate (don't advance time)
    let claimed = test.claim_yield(&test.user1);

    // Should be zero
    assert_eq!(claimed, 0);

    let vault_balance = test.vault_client.balance(&test.user1);
    assert_eq!(vault_balance, 0);
}

#[test]
fn test_proportional_yield_distribution() {
    let test = YieldTokenTest::setup();

    let initial_rate = test.get_exchange_rate();

    // User1 gets 2x as much as user2
    test.mint_yt(&test.user1, 2_000_000_000_000i128, initial_rate);
    test.mint_yt(&test.user2, 1_000_000_000_000i128, initial_rate);

    // Increase rate
    test.advance_time(100);

    // Both claim
    let claimed1 = test.claim_yield(&test.user1);
    let claimed2 = test.claim_yield(&test.user2);

    // User1 should get approximately 2x the yield of user2
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

    // User gets initial YT
    test.mint_yt(&test.user1, 1_000_000_000_000i128, initial_rate);

    // Rate increases
    test.advance_time(100);
    let new_rate = test.get_exchange_rate();

    // User gets more YT (should preserve their high water mark at new rate)
    test.mint_yt(&test.user1, 1_000_000_000_000i128, new_rate);

    // User index should be at new rate (not reset to initial)
    let user_index = test.get_user_index(&test.user1);
    assert_eq!(user_index, new_rate);

    // Should have accrued yield from the first mint
    let accrued = test.get_accrued_yield(&test.user1);
    assert!(accrued > 0);
}

#[test]
fn test_sep41_balance_function() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000i128;
    let exchange_rate = 1_000_000i128;

    test.mint_yt(&test.user1, mint_amount, exchange_rate);

    // Test that balance function works (SEP-41 standard)
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

    // Try to transfer more than balance
    test.transfer(&test.user1, &test.user2, mint_amount + 1);
}

#[test]
#[should_panic(expected = "Insufficient balance")]
fn test_burn_insufficient_balance() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000i128;
    let exchange_rate = 1_000_000i128;
    test.mint_yt(&test.user1, mint_amount, exchange_rate);

    // Try to burn more than balance
    test.burn(&test.user1, mint_amount + 1);
}

#[test]
fn test_zero_balance_user_can_claim() {
    let test = YieldTokenTest::setup();

    // User with no balance should be able to call claim_yield without panic
    let claimed = test.claim_yield(&test.user1);
    assert_eq!(claimed, 0);
}

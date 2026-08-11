// ── "You snooze you lose" surplus tests ──────────────────────────────────────
//
// Positions freeze in ASSET value at maturity: PT pays face value, YT yield
// pays its locked-rate value, whenever redeemed/claimed. Vault interest earned
// after maturity is freed share-by-share as users exit and is swept to the
// treasury by collect_surplus.

use soroban_sdk::token::TokenClient;
use yield_manager_interface::YieldManagerClient;
use yield_token_interface::YieldTokenClient;

use super::fixture::YieldManagerTest;

const SCALAR_7: i128 = 1_0000000;

fn ym_client<'a>(test: &YieldManagerTest) -> YieldManagerClient<'a> {
    YieldManagerClient::new(&test.env, &test.yield_manager)
}

#[test]
fn test_get_treasury_returns_constructor_value() {
    let test = YieldManagerTest::setup();
    assert_eq!(ym_client(&test).get_treasury(), test.treasury);
}

#[test]
fn test_collect_with_nothing_accrued_returns_zero() {
    let test = YieldManagerTest::setup();
    assert_eq!(ym_client(&test).collect_surplus(), 0);
    assert_eq!(test.vault_balance(&test.treasury), 0);
}

#[test]
fn test_redeem_at_locked_rate_accrues_no_surplus() {
    let test = YieldManagerTest::setup();
    let ym = ym_client(&test);

    let shares = 1000 * SCALAR_7;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    // Mature and lock with the rate unchanged at 1.0; redeem immediately.
    test.advance_time(1100);
    ym.get_exchange_rate();
    ym.redeem_principal(&test.user1, &test.get_pt_balance(&test.user1));

    assert_eq!(ym.collect_surplus(), 0);
    assert_eq!(test.vault_balance(&test.treasury), 0);
}

/// The core conservation scenario. Deposit 1000 shares at rate 1.0 (mints
/// 1000 PT + 1000 YT); the vault grows to 1.25 by maturity (locks) and keeps
/// growing to 2.5 afterward. Then:
///   - PT redeems for 400 shares (= exactly 1000 assets, face value),
///     freeing 400 of its 800-share locked-rate backing;
///   - the YT claim's frozen 200 shares pay out as 100 shares (= exactly its
///     250-asset locked-rate value), freeing the other 100;
///   - collect_surplus sweeps the 500 freed shares — all post-maturity
///     interest — and the YM ends exactly empty.
#[test]
fn test_pt_and_yt_exits_free_post_maturity_interest() {
    let test = YieldManagerTest::setup();
    let ym = ym_client(&test);
    let yt = YieldTokenClient::new(&test.env, &test.yt);

    let shares = 1000 * SCALAR_7;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    // Vault grows to 1.25 by maturity; lock there.
    test.set_vault_exchange_rate(12_500_000);
    test.advance_time(1100);
    assert_eq!(ym.get_exchange_rate(), 12_500_000);

    // Vault keeps growing after maturity — none of this belongs to users.
    test.set_vault_exchange_rate(25_000_000);

    let user_shares_before = test.vault_balance(&test.user1);
    ym.redeem_principal(&test.user1, &(1000 * SCALAR_7));
    let pt_payout = test.vault_balance(&test.user1) - user_shares_before;
    assert_eq!(pt_payout, 400 * SCALAR_7, "PT must pay face value at the live rate");

    let yt_payout = yt.claim_yield(&test.user1);
    assert_eq!(
        yt_payout,
        100 * SCALAR_7,
        "YT claim must pay its locked-rate asset value at the live rate"
    );

    assert_eq!(ym.collect_surplus(), 500 * SCALAR_7);
    assert_eq!(test.vault_balance(&test.treasury), 500 * SCALAR_7);

    // Full conservation: everyone paid, protocol swept, nothing stranded.
    assert_eq!(test.vault_balance(&test.yield_manager), 0);
}

#[test]
fn test_collect_twice_second_returns_zero() {
    let test = YieldManagerTest::setup();
    let ym = ym_client(&test);

    let shares = 1000 * SCALAR_7;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    test.advance_time(1100);
    ym.get_exchange_rate(); // lock at 1.0
    test.set_vault_exchange_rate(20_000_000);
    ym.redeem_principal(&test.user1, &(1000 * SCALAR_7));

    assert_eq!(ym.collect_surplus(), 500 * SCALAR_7);
    assert_eq!(ym.collect_surplus(), 0);
    assert_eq!(test.vault_balance(&test.treasury), 500 * SCALAR_7);
}

#[test]
fn test_collect_surplus_is_permissionless() {
    let test = YieldManagerTest::setup();
    let ym = ym_client(&test);

    let shares = 1000 * SCALAR_7;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    test.advance_time(1100);
    ym.get_exchange_rate(); // lock at 1.0
    test.set_vault_exchange_rate(20_000_000);
    ym.redeem_principal(&test.user1, &(1000 * SCALAR_7));

    // Strip every auth: collect_surplus must still succeed — its destination
    // is fixed, so no signature is required to trigger it.
    test.env.set_auths(&[]);
    assert_eq!(ym.collect_surplus(), 500 * SCALAR_7);
}

/// A late YT claimer receives the same asset value as a punctual one — the
/// extra shares their frozen claim would have paid go to the protocol.
#[test]
fn test_late_yt_claim_gets_locked_value_not_appreciation() {
    let test = YieldManagerTest::setup();
    let yt = YieldTokenClient::new(&test.env, &test.yt);
    let ym = ym_client(&test);

    let shares = 1000 * SCALAR_7;
    test.mint_vault_shares(&test.user1, shares);
    test.deposit(&test.user1.clone(), shares);

    test.set_vault_exchange_rate(12_500_000);
    test.advance_time(1100);
    ym.get_exchange_rate(); // lock at 1.25; frozen YT claim = 200 shares

    // Claimed punctually this would pay 200 shares; claimed after the vault
    // doubles again it pays 100 — worth the same 250 assets either way.
    test.set_vault_exchange_rate(25_000_000);
    assert_eq!(yt.claim_yield(&test.user1), 100 * SCALAR_7);

    let vault_token = TokenClient::new(&test.env, &test.vault_addr);
    assert_eq!(vault_token.balance(&test.user1), 100 * SCALAR_7);
}
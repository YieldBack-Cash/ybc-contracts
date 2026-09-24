//! YT yield against a real vault rate: the yield manager's high-water mark
//! driven by the backing protocol's own accrual rather than a mock setter.

use soroban_sdk::{testutils::Address as _, Address};

use super::fixture::{VaultStack, ONE_DAY_SECS, ONE_YEAR_SECS};

/// No yield before time passes or any interest accrues.
fn no_yield_before_interest_accrues(f: &VaultStack) {
    f.setup_yt_position(&f.user.clone(), 1_000_0000000);
    assert_eq!(f.claim_yield(&f.user.clone()), 0);
}

/// After time passes and the protocol accrues, YT holders earn yield.
fn yield_accrues_after_time_passes(f: &VaultStack) {
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    assert!(f.claim_yield(&user) > 0, "yield should accrue over 30 days");
}

/// Claimed yield arrives as vault shares transferred to the user.
fn claim_yield_transfers_vault_shares_to_user(f: &VaultStack) {
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    let before = f.vault_shares(&user);
    let claimed = f.claim_yield(&user);

    assert!(claimed > 0);
    assert_eq!(f.vault_shares(&user), before + claimed);
}

/// Claiming twice at the same rate returns 0 the second time.
fn double_claim_returns_zero_second_time(f: &VaultStack) {
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    assert!(f.claim_yield(&user) > 0);
    assert_eq!(f.claim_yield(&user), 0);
}

/// Each new accrual period yields an additional positive amount.
fn multiple_periods_accumulate_yield(f: &VaultStack) {
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();
    let first = f.claim_yield(&user);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();
    let second = f.claim_yield(&user);

    assert!(first > 0 && second > 0, "both periods should produce yield");
}

/// Yield accumulates if not claimed between periods; one claim collects it all.
fn unclaimed_yield_accumulates_and_is_claimed_at_once(f: &VaultStack) {
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();
    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    let before = f.vault_shares(&user);
    let claimed = f.claim_yield(&user);

    assert!(claimed > 0);
    assert_eq!(f.vault_shares(&user), before + claimed);
}

/// Two users earn yield proportional to their YT balance.
fn two_users_earn_proportional_yield(f: &VaultStack) {
    let user_a = f.user.clone();
    let user_b = Address::generate(&f.env);

    f.setup_yt_position(&user_a, 2_000_0000000);
    f.setup_yt_position(&user_b, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    let claimed_a = f.claim_yield(&user_a);
    let claimed_b = f.claim_yield(&user_b);

    assert!(claimed_a > 0 && claimed_b > 0);
    let ratio = claimed_a as f64 / claimed_b as f64;
    assert!(ratio > 1.95 && ratio < 2.05, "expected ~2x, got {ratio}");
}

/// After the exchange rate is sealed at maturity, no further yield accrues.
fn no_new_yield_accrues_after_maturity_is_sealed(f: &VaultStack) {
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(ONE_YEAR_SECS + ONE_DAY_SECS);
    f.accrue_interest();

    // First claim at/after maturity seals the rate.
    let _ = f.claim_yield(&user);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    assert_eq!(f.claim_yield(&user), 0, "rate is locked after maturity");
}

/// Yield that accrued before maturity remains claimable after it passes.
fn pre_maturity_yield_claimable_after_maturity(f: &VaultStack) {
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    f.advance_time(ONE_YEAR_SECS + 1);

    let before = f.vault_shares(&user);
    let claimed = f.claim_yield(&user);

    assert!(claimed > 0);
    assert_eq!(f.vault_shares(&user), before + claimed);
}

on_every_vault!(
    no_yield_before_interest_accrues,
    yield_accrues_after_time_passes,
    claim_yield_transfers_vault_shares_to_user,
    double_claim_returns_zero_second_time,
    multiple_periods_accumulate_yield,
    unclaimed_yield_accumulates_and_is_claimed_at_once,
    two_users_earn_proportional_yield,
    no_new_yield_accrues_after_maturity_is_sealed,
    pre_maturity_yield_claimable_after_maturity,
);

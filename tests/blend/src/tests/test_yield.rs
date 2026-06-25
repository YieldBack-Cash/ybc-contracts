use soroban_sdk::{testutils::Address as _, Env};

use super::real_fixture::{RealBlendFixture, ONE_DAY_SECS, ONE_YEAR_SECS};

/// No yield before time passes or any interest accrues.
#[test]
fn no_yield_before_interest_accrues() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);
    f.setup_yt_position(&f.user.clone(), 1_000_0000000);

    let claimed = f.claim_yield(&f.user.clone());
    assert_eq!(claimed, 0, "no yield before time passes");
}

/// After time passes and interest is accrued in the pool, YT holders earn yield.
#[test]
fn yield_accrues_after_time_passes() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    let claimed = f.claim_yield(&user);
    assert!(claimed > 0, "yield should accrue with active borrowers over 30 days");
}

/// Claimed yield arrives as fee vault shares transferred to the user.
#[test]
fn claim_yield_transfers_vault_shares_to_user() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    let shares_before = f.vault_shares(&user);
    let claimed = f.claim_yield(&user);

    assert!(claimed > 0, "expected positive yield");
    assert_eq!(
        f.vault_shares(&user),
        shares_before + claimed,
        "user must receive exactly claimed vault shares"
    );
}

/// Claiming twice at the same rate returns 0 the second time.
#[test]
fn double_claim_returns_zero_second_time() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    let first = f.claim_yield(&user);
    assert!(first > 0);

    let second = f.claim_yield(&user);
    assert_eq!(second, 0, "no yield to claim at same rate");
}

/// Each new interest period yields an additional positive amount.
#[test]
fn multiple_periods_accumulate_yield() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);
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

/// Yield accumulates if not claimed between periods; one claim collects all of it.
#[test]
fn unclaimed_yield_accumulates_and_is_claimed_at_once() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();
    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    let shares_before = f.vault_shares(&user);
    let claimed = f.claim_yield(&user);

    assert!(claimed > 0, "accumulated yield should be positive");
    assert_eq!(
        f.vault_shares(&user),
        shares_before + claimed,
        "all accumulated yield must arrive as vault shares"
    );
}

/// Two users earn yield proportional to their YT balance.
#[test]
fn two_users_earn_proportional_yield() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);

    let user_a = f.user.clone();
    let user_b = soroban_sdk::Address::generate(&env);

    // user A deposits 2× more than user B.
    f.setup_yt_position(&user_a, 2_000_0000000);
    f.setup_yt_position(&user_b, 1_000_0000000);

    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    let claimed_a = f.claim_yield(&user_a);
    let claimed_b = f.claim_yield(&user_b);

    assert!(claimed_a > 0 && claimed_b > 0, "both users should earn yield");
    let ratio = claimed_a as f64 / claimed_b as f64;
    assert!(
        ratio > 1.95 && ratio < 2.05,
        "user A (2× balance) should earn ~2× the yield of user B, got ratio {ratio}"
    );
}

/// After maturity no new yield accrues, regardless of ongoing pool interest.
#[test]
fn no_new_yield_accrues_after_maturity() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    // Claim any pre-maturity yield so the slate is clean.
    f.advance_time(ONE_DAY_SECS);
    f.accrue_interest();
    let _ = f.claim_yield(&user);

    // Advance well past maturity and accrue more interest.
    f.advance_time(ONE_YEAR_SECS + 1);
    f.accrue_interest();

    let claimed = f.claim_yield(&user);
    assert_eq!(claimed, 0, "no new yield should accrue after maturity");
}

/// Yield that accrued before maturity remains claimable after maturity passes.
#[test]
fn pre_maturity_yield_claimable_after_maturity() {
    let env = Env::default();
    let f = RealBlendFixture::new(&env);
    let user = f.user.clone();
    f.setup_yt_position(&user, 1_000_0000000);

    // Accrue interest pre-maturity but do not claim.
    f.advance_time(30 * ONE_DAY_SECS);
    f.accrue_interest();

    // Advance past maturity without claiming.
    f.advance_time(ONE_YEAR_SECS + 1);

    let shares_before = f.vault_shares(&user);
    let claimed = f.claim_yield(&user);

    assert!(claimed > 0, "pre-maturity yield must be claimable after maturity");
    assert_eq!(
        f.vault_shares(&user),
        shares_before + claimed,
        "claimed yield must arrive as vault shares"
    );
}
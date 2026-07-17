use soroban_sdk::Env;

use super::fixture::{AmmFixture, ONE_YEAR_SECS};

#[test]
fn test_first_deposit_mints_shares_and_burns_minimum_liquidity() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    let pt_in = 10_000_000i128;
    let v_in = 10_000_000i128;
    f.deposit(&f.admin, pt_in, v_in);

    let (res_pt, res_v) = f.pool.get_reserves();
    assert_eq!(res_pt, pt_in);
    assert_eq!(res_v, v_in);

    // User shares = sqrt(pt * v) - MINIMUM_LIQUIDITY
    let expected_shares = (pt_in * v_in).isqrt() - 100;
    assert_eq!(f.pool.balance_shares(&f.admin), expected_shares);
}

#[test]
#[should_panic]
fn test_deposit_expired_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let now = env.ledger().timestamp();
    f.set_time(now + ONE_YEAR_SECS + 1);

    f.deposit(&f.user, 1_000_000, 1_000_000);
}

#[test]
fn test_second_deposit_proportional() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares_before = f.pool.balance_shares(&f.user);
    f.deposit(&f.user, 5_000_000, 5_000_000);
    let shares_after = f.pool.balance_shares(&f.user);

    assert!(shares_after > shares_before);
}

#[test]
#[should_panic]
fn test_deposit_zero_amount_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 0, 10_000_000);
}

/// Guard #1 — first-deposit underflow.
///
/// The initial deposit mints `sqrt(a*b)` shares and burns `MINIMUM_LIQUIDITY`
/// (100) of them to the dead address. If `sqrt(a*b) <= 100` the depositor would
/// receive zero (or, via underflow, negative) shares for real tokens. Deposit
/// must revert instead.
#[test]
#[should_panic]
fn test_initial_deposit_below_minimum_liquidity_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    // sqrt(50 * 50) = 50 <= 100 → would leave the depositor with nothing.
    f.deposit(&f.admin, 50, 50);
}

/// The smallest initial deposit that clears the dead-burn still works: the guard
/// rejects only genuinely-too-small deposits, not legitimate ones.
#[test]
fn test_initial_deposit_just_above_minimum_liquidity_ok() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    // sqrt(101 * 101) = 101 > 100 → depositor keeps 1 share after the burn.
    f.deposit(&f.admin, 101, 101);
    assert_eq!(f.pool.balance_shares(&f.admin), 1);
}

/// Skews the pool into the low-shares / high-reserve state described by the
/// share-rounding bug: round-trip swaps leave `reserve_a` pinned at the share
/// count while fees inflate `reserve_b` well above it. Returns the fixture.
fn skewed_pool(env: &Env) -> AmmFixture<'_> {
    let f = AmmFixture::new(env);

    let p = 100_000_000i128;
    f.deposit(&f.admin, p, p);

    // Exact-PT round trips return reserve_a to `p` each time while swap fees
    // accrue in reserve_b, driving reserve_b far above total_shares (= p).
    let c = p / 10;
    for _ in 0..40 {
        f.swap_pt_for_v(&f.admin, c, 1);
        f.swap_v_for_pt(&f.admin, c, i128::MAX / 4);
    }

    let (ra, rb) = f.pool.get_reserves();
    let s = f.total_shares_admin_only();
    // Precondition for the bug: reserve_b has grown past the share count while
    // reserve_a is unchanged. A tiny deposit can now round its shares to zero.
    assert!(rb > s && ra == s, "unexpected skew: ra={} rb={} s={}", ra, rb, s);
    f
}

/// Guard #2 — subsequent-deposit rounds to zero shares.
///
/// In the skewed state, depositing `desired_a = 1` yields `amount_b = 2`, and
/// `floor((reserve_b + 2) * total_shares / reserve_b) == total_shares` — i.e.
/// zero shares minted. The depositor would hand over real tokens for nothing;
/// deposit must revert instead.
#[test]
#[should_panic]
fn test_deposit_rounding_to_zero_shares_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = skewed_pool(&env);

    // desired_b is generous; get_deposit_amounts binds on desired_a = 1.
    f.pt.approve(&f.user, &f.pool.address, &1, &(env.ledger().sequence() + 1000));
    f.vault.approve(&f.user, &f.pool.address, &1_000, &(env.ledger().sequence() + 1000));
    f.pool.deposit(&f.user, &1, &0, &1_000, &0);
}

/// The zero-share guard must not block a legitimately-sized deposit made against
/// the same skewed pool.
#[test]
fn test_normal_deposit_after_skew_still_mints() {
    let env = Env::default();
    env.mock_all_auths();
    let f = skewed_pool(&env);

    let before = f.pool.balance_shares(&f.user);
    // Large enough that neither side rounds down to zero.
    f.deposit(&f.user, 10_000_000, 100_000_000);
    assert!(f.pool.balance_shares(&f.user) > before);
}
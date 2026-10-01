//! Removing liquidity: pro-rata payout, minimums, and share accounting.

use amm_interface::AmmError;
use soroban_sdk::Env;

use super::fixture::AmmFixture;

/// A non-positive share_amount must be rejected at the entrypoint. Without the
/// check it passes `current_shares < share_amount` and reaches burn_shares,
/// where subtracting a negative mints LP shares; the only thing that stopped it
/// was a token leg refusing the resulting transfer, which PT does not.
#[test]
fn test_withdraw_rejects_non_positive_share_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 1_000);
    let total_before = f.pool.get_total_shares();

    for bad in [-1i128, 0, -1_000_000] {
        let res = f.pool.try_withdraw(&f.user, &bad, &-1_000_000, &-1_000_000);
        assert!(
            matches!(res, Err(Ok(AmmError::InvalidAmount))),
            "share_amount {bad} should be InvalidAmount, got {res:?}"
        );
    }

    assert_eq!(f.pool.balance_shares(&f.user), 0);
    assert_eq!(f.pool.get_total_shares(), total_before);
}

/// Negative minimums are meaningless and would let a bad out_a/out_b pass the
/// slippage check; reject them with the same error.
#[test]
fn test_withdraw_rejects_negative_minimums() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);

    assert!(matches!(
        f.pool.try_withdraw(&f.admin, &shares, &-1, &0),
        Err(Ok(AmmError::InvalidAmount))
    ));
    assert!(matches!(
        f.pool.try_withdraw(&f.admin, &shares, &0, &-1),
        Err(Ok(AmmError::InvalidAmount))
    ));
    assert_eq!(f.pool.balance_shares(&f.admin), shares);
}

#[test]
fn test_withdraw_returns_proportional_tokens() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);
    assert!(shares > 0);

    let pt_before = f.pt.balance(&f.admin);
    let v_before = f.vault.balance(&f.admin);

    f.pool.withdraw(&f.admin, &shares, &0, &0);

    let pt_after = f.pt.balance(&f.admin);
    let v_after = f.vault.balance(&f.admin);

    assert!(pt_after > pt_before, "should receive PT back");
    assert!(v_after > v_before, "should receive V back");
    assert_eq!(f.pool.balance_shares(&f.admin), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_withdraw_insufficient_shares_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);
    f.pool.withdraw(&f.admin, &(shares + 1), &0, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #23)")]
fn test_withdraw_min_not_satisfied_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);
    f.pool.withdraw(&f.admin, &shares, &999_999_999, &0);
}

use soroban_sdk::Env;

use super::fixture::AmmFixture;

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
#[should_panic]
fn test_withdraw_insufficient_shares_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);
    // Try to withdraw more than owned
    f.pool.withdraw(&f.admin, &(shares + 1), &0, &0);
}

#[test]
#[should_panic]
fn test_withdraw_min_not_satisfied_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);
    // min_a set absurdly high
    f.pool.withdraw(&f.admin, &shares, &999_999_999, &0);
}
use soroban_sdk::Env;

use super::fixture::AmmFixture;

#[test]
fn test_first_deposit_mints_shares_and_burns_minimum_liquidity() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    let pt_in = 10_000_000i128;
    let v_in = 10_000_000i128;
    f.deposit(&f.admin, pt_in, v_in);

    let (res_pt, res_v) = f.pool.get_rsrvs();
    assert_eq!(res_pt, pt_in);
    assert_eq!(res_v, v_in);

    // User shares = sqrt(pt * v) - MINIMUM_LIQUIDITY
    let expected_shares = (pt_in * v_in).isqrt() - 100;
    assert_eq!(f.pool.balance_shares(&f.admin), expected_shares);
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
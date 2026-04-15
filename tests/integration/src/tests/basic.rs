use soroban_sdk::Env;

use super::fixture::IntegrationFixture;

/// Depositing vault shares into the yield manager issues PT and YT 1:1.
#[test]
fn test_deposit_issues_pt_and_yt() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    let shares = 100_000_000i128;
    f.ym_deposit(&f.user.clone(), shares);

    assert_eq!(f.pt_balance(&f.user), shares);
    assert_eq!(f.yt_balance(&f.user), shares);
}

/// PT received from yield_manager can be deposited into the AMM alongside
/// vault shares to provide liquidity.
#[test]
fn test_pt_from_yield_manager_works_in_amm() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    // Split user's vault shares: half to yield_manager, half kept for AMM
    let shares = 100_000_000i128;
    let half = shares / 2;

    f.ym_deposit(&f.user.clone(), half); // → half PT + half YT

    // Provide liquidity: PT from yield_manager + vault shares
    f.amm_deposit(&f.user.clone(), half, half);

    // User should hold LP shares in the pool
    assert!(f.pool.balance_shares(&f.user) > 0);
}
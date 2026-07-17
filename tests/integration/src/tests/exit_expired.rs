//! Pins the router's one-call exit from an expired market: LP shares are
//! burned, the user's entire PT balance is redeemed at the YM's locked
//! post-maturity rate, accrued YT yield is claimed, and everything lands as
//! vault shares.

use super::fixture::{IntegrationFixture, ONE_YEAR_SECS};
use soroban_sdk::Env;

const YM_DEPOSIT: i128 = 100_000_000;
const POOL_PT: i128 = 50_000_000;
const POOL_V: i128 = 50_000_000;

/// User mints PT+YT and LPs into the pool, exactly like the router_swaps seeding.
fn seeded<'a>(env: &'a Env) -> IntegrationFixture<'a> {
    let f = IntegrationFixture::new(env);
    f.vault.set_exchange_rate(&10_000_000);
    f.ym_deposit(&f.user, YM_DEPOSIT);
    f.amm_deposit(&f.user, POOL_PT, POOL_V);
    f
}

#[test]
fn test_exit_expired_returns_all_value_as_vault_shares() {
    let env = Env::default();
    let f = seeded(&env);

    let lp = f.pool.balance_shares(&f.user);
    assert!(lp > 0, "user must hold LP shares");
    let v_before = f.vault.balance(&f.user);

    // Expire the market; the exit path operates on it by (vault, maturity).
    f.advance_time(ONE_YEAR_SECS + 1);

    let shares_out = f.router_exit_expired(&f.vault.address, f.maturity, &f.user, lp, 1);

    // Position fully unwound: no LP shares, no PT, and the post-maturity
    // yield claim burned the now-worthless YT.
    assert_eq!(f.pool.balance_shares(&f.user), 0, "LP position burned");
    assert_eq!(f.pt_balance(&f.user), 0, "all PT redeemed");
    assert_eq!(f.yt_balance(&f.user), 0, "expired YT burned");
    assert_eq!(
        f.vault.balance(&f.user),
        v_before + shares_out,
        "return value matches shares delivered"
    );

    // With rate 1.0 and no trades, the user recovers their full deposit: the
    // LP legs (POOL_PT + POOL_V) plus the wallet PT (YM_DEPOSIT - POOL_PT),
    // all as vault shares. First-deposit share rounding may shave dust.
    let expected = POOL_V + YM_DEPOSIT;
    assert!(
        shares_out <= expected && shares_out >= expected - 1000,
        "expected ~{} shares out, got {}",
        expected,
        shares_out
    );
}

#[test]
fn test_exit_expired_with_no_lp_redeems_wallet_pt() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    f.vault.set_exchange_rate(&10_000_000);
    f.ym_deposit(&f.user, YM_DEPOSIT);

    let v_before = f.vault.balance(&f.user);
    f.advance_time(ONE_YEAR_SECS + 1);

    let shares_out = f.router_exit_expired(&f.vault.address, f.maturity, &f.user, 0, 1);

    assert_eq!(f.pt_balance(&f.user), 0, "wallet PT redeemed");
    assert_eq!(f.yt_balance(&f.user), 0, "expired YT burned");
    assert_eq!(shares_out, YM_DEPOSIT, "PT redeems 1:1 at rate 1.0");
    assert_eq!(f.vault.balance(&f.user), v_before + shares_out);
}

#[test]
#[should_panic(expected = "market not expired")]
fn test_exit_expired_before_maturity_reverts() {
    let env = Env::default();
    let f = seeded(&env);

    let lp = f.pool.balance_shares(&f.user);
    f.router_exit_expired(&f.vault.address, f.maturity, &f.user, lp, 1);
}

#[test]
#[should_panic(expected = "min_shares_out not satisfied")]
fn test_exit_expired_min_shares_out_reverts() {
    let env = Env::default();
    let f = seeded(&env);

    let lp = f.pool.balance_shares(&f.user);
    f.advance_time(ONE_YEAR_SECS + 1);

    // Demand more than the position can possibly return.
    f.router_exit_expired(&f.vault.address, f.maturity, &f.user, lp, i128::MAX);
}
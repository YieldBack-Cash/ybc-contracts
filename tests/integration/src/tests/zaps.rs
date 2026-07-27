//! Base-asset zaps, exercised against a real SEP-56 vault.
//!
//! The invariant every test here shares is that a zap leaves the user's vault
//! share balance exactly where it found it. Shares are an implementation detail
//! the user never asked to hold — if a zap ever strands one, the abstraction has
//! leaked. See `zap_fixture.rs` for why these run against OpenZeppelin's vault
//! rather than `mock_vault`.

use soroban_sdk::Env;

use super::zap_fixture::ZapFixture;

#[test]
fn zap_asset_for_pt_buys_exact_pt_and_strands_no_shares() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let asset_before = f.balance(&f.asset);
    let pt_before = f.balance(&f.pt);
    let shares_before = f.balance(&f.vault);

    let spent = f
        .router
        .zap_asset_for_pt(&f.vault, &f.maturity, &f.user, &100_000_000, &300_000_000);

    assert_eq!(f.balance(&f.pt) - pt_before, 100_000_000, "exact PT out");
    assert_eq!(
        f.balance(&f.vault),
        shares_before,
        "zap must leave no vault shares behind"
    );
    assert_eq!(
        asset_before - f.balance(&f.asset),
        spent,
        "return value is the asset actually spent"
    );
    assert!(spent > 0 && spent <= 300_000_000, "spend within bound: {spent}");
}

#[test]
fn zap_pt_for_asset_round_trips() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let spent = f
        .router
        .zap_asset_for_pt(&f.vault, &f.maturity, &f.user, &100_000_000, &300_000_000);

    let shares_before = f.balance(&f.vault);
    let asset_before = f.balance(&f.asset);
    let received = f
        .router
        .zap_pt_for_asset(&f.vault, &f.maturity, &f.user, &100_000_000, &1);

    assert_eq!(f.balance(&f.asset) - asset_before, received);
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
    // Round-tripping costs the spread plus two lots of fees, so the user gets
    // back less than they put in — but the same order of magnitude, not dust.
    assert!(received < spent, "round trip must not be profitable");
    assert!(
        received > spent / 2,
        "round trip lost too much: {received} vs {spent}"
    );
}

#[test]
fn zap_asset_for_split_mints_pt_and_yt_together() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let pt_before = f.balance(&f.pt);
    let yt_before = f.balance(&f.yt);
    let shares_before = f.balance(&f.vault);

    let minted = f
        .router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &1);

    assert_eq!(f.balance(&f.pt) - pt_before, minted);
    assert_eq!(
        f.balance(&f.yt) - yt_before,
        minted,
        "PT and YT mint in equal measure"
    );
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
}

#[test]
fn zap_split_for_asset_returns_the_underlying() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let minted = f
        .router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &1);

    let asset_before = f.balance(&f.asset);
    let shares_before = f.balance(&f.vault);
    let returned = f
        .router
        .zap_split_for_asset(&f.vault, &f.maturity, &f.user, &minted, &1);

    assert_eq!(f.balance(&f.asset) - asset_before, returned);
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
    // Splitting and recombining touches no AMM and charges no fee, so the only
    // loss is rounding.
    assert!(
        returned >= 499_999_000 && returned <= 500_000_000,
        "split round trip should be near-lossless, got {returned}"
    );
}

#[test]
fn zap_asset_for_yt_and_back() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let yt_before = f.balance(&f.yt);
    let shares_before = f.balance(&f.vault);

    let spent = f
        .router
        .zap_asset_for_yt(&f.vault, &f.maturity, &f.user, &100_000_000, &300_000_000);

    assert_eq!(f.balance(&f.yt) - yt_before, 100_000_000, "exact YT out");
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
    // YT costs only the yield portion, far less than its face amount.
    assert!(spent > 0 && spent < 100_000_000, "YT should be cheap: {spent}");

    let asset_before = f.balance(&f.asset);
    let received = f
        .router
        .zap_yt_for_asset(&f.vault, &f.maturity, &f.user, &100_000_000, &1);

    assert_eq!(f.balance(&f.asset) - asset_before, received);
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
    assert!(received > 0);
}

#[test]
fn zap_lp_round_trip() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let lp_before = f.pool.balance_shares(&f.user);
    let shares_before = f.balance(&f.vault);

    // Pool sits at 1:1, so roughly half the deposit should become PT.
    let lp_out = f.router.zap_asset_for_lp(
        &f.vault,
        &f.maturity,
        &f.user,
        &400_000_000,
        &200_000_000,
        &1,
    );

    assert_eq!(f.pool.balance_shares(&f.user) - lp_before, lp_out);
    assert!(lp_out > 0);
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");

    let asset_before = f.balance(&f.asset);
    let returned = f
        .router
        .zap_lp_for_asset(&f.vault, &f.maturity, &f.user, &lp_out, &1);

    assert_eq!(f.balance(&f.asset) - asset_before, returned);
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
    assert!(returned > 0);
}

#[test]
fn exit_expired_to_asset_unwinds_everything() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let lp = f.pool.balance_shares(&f.user);
    f.accrue_yield(200_000_000);
    f.advance_past_maturity();

    let asset_before = f.balance(&f.asset);
    let shares_before = f.balance(&f.vault);
    let returned = f
        .router
        .exit_expired_to_asset(&f.vault, &f.maturity, &f.user, &lp, &1);

    assert_eq!(f.balance(&f.asset) - asset_before, returned);
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
    assert_eq!(f.balance(&f.pt), 0, "all PT redeemed");
    assert!(returned > 0);
}

#[test]
fn yield_accrual_raises_what_a_zap_returns() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let minted = f
        .router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &1);

    // Same PT+YT position, but the vault is now worth more per share. Holding
    // both legs means holding the yield too, so the exit must be bigger.
    f.accrue_yield(1_000_000_000);

    let asset_before = f.balance(&f.asset);
    f.router
        .zap_split_for_asset(&f.vault, &f.maturity, &f.user, &minted, &1);
    let returned = f.balance(&f.asset) - asset_before;

    assert!(
        returned > 500_000_000,
        "accrued yield should raise the payout above the 500_000_000 deposited, got {returned}"
    );
}

#[test]
#[should_panic(expected = "min_asset_out not satisfied")]
fn zap_out_respects_min_asset_out() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    f.router
        .zap_asset_for_pt(&f.vault, &f.maturity, &f.user, &100_000_000, &300_000_000);
    // Demand far more than 1e8 PT could possibly fetch.
    f.router
        .zap_pt_for_asset(&f.vault, &f.maturity, &f.user, &100_000_000, &999_000_000);
}

#[test]
#[should_panic]
fn zap_in_reverts_when_the_asset_budget_is_too_small() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    // 1_000 of the asset buys nowhere near 1e8 PT. The AMM's own `v_in_max`
    // bound rejects the swap, which unwinds the deposit with it — so the user
    // is not left holding the vault shares the first leg produced. No `expected`
    // string: the panic originates inside the AMM's wasm and surfaces as an
    // opaque trap, and pinning that text would test the host, not the router.
    f.router
        .zap_asset_for_pt(&f.vault, &f.maturity, &f.user, &100_000_000, &1_000);
}

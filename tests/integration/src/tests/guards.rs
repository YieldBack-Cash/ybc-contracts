//! Every floor and ceiling the router settles a zap against, tripped on
//! purpose.
//!
//! The router's slippage protection is one comparison at the end of each
//! path, against a measured balance delta. Nothing else in the code depends on
//! that comparison, so a refactor that moved, loosened or dropped it would
//! compile, run and pass every happy-path test. These tests ask for more than a
//! trade can produce and expect the typed refusal; remove a guard and its test
//! passes a transaction it should have refused, and fails. docs/SECURITY.md
//! invariant 17 and the threat model's §4.5 rest on these.
//!
//! Two paths have no floor of their own to trip: `zap_asset_for_pt` and
//! `zap_asset_for_yt` bound the price through `max_v_in`, which the pool
//! (`MaxVInExceeded`) and the yield manager (`SlippageExceeded`) enforce;
//! `zaps.rs` covers the pool side and this file the yield manager side. Their
//! trailing `AssetSpentOverMax` check cannot trip by construction, since the
//! whole budget is deposited and the remainder swept back.

use soroban_sdk::Env;

use super::zap_fixture::{ZapFixture, SWEEP};

/// More than any position in the fixture could return.
const IMPOSSIBLE: i128 = i128::MAX / 4;

// ── router floors: RouterError::MinAssetOutNotMet (7) ────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn zap_pt_for_asset_refuses_a_fill_below_the_floor() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    f.router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &1);

    f.router.zap_pt_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &IMPOSSIBLE,
        &SWEEP,
        &f.expiry(),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn zap_yt_for_asset_refuses_a_fill_below_the_floor() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    f.router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &1);

    f.router.zap_yt_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &IMPOSSIBLE,
        &SWEEP,
        &f.expiry(),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn zap_lp_for_asset_refuses_a_fill_below_the_floor() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let lp_out = f.router.zap_asset_for_lp(
        &f.vault,
        &f.maturity,
        &f.user,
        &400_000_000,
        &200_000_000,
        &250_000_000,
        &150_000_000,
        &1,
        &SWEEP,
        &f.expiry(),
    );

    f.router.zap_lp_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &lp_out,
        &f.balance(&f.pt),
        &IMPOSSIBLE,
        &SWEEP,
        &f.expiry(),
    );
}

// ── router LP floor: RouterError::MinLpOutNotMet (9) ─────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn zap_asset_for_lp_refuses_fewer_lp_shares_than_the_floor() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    f.router.zap_asset_for_lp(
        &f.vault,
        &f.maturity,
        &f.user,
        &400_000_000,
        &200_000_000,
        &250_000_000,
        &150_000_000,
        &IMPOSSIBLE,
        &SWEEP,
        &f.expiry(),
    );
}

// ── yield-manager floors reached through the router: SlippageExceeded (8) ───

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn zap_asset_for_split_refuses_fewer_tokens_than_the_floor() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    f.router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &IMPOSSIBLE);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn zap_split_for_asset_refuses_a_fill_below_the_floor() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let minted = f
        .router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &1);

    f.router
        .zap_split_for_asset(&f.vault, &f.maturity, &f.user, &minted, &IMPOSSIBLE);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn zap_asset_for_yt_refuses_a_cost_above_max_v_in() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    // The deposit funds `max_v_in` comfortably; the YT then costs more than the
    // one share allowed, and the yield manager's callback refuses.
    f.router.zap_asset_for_yt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &1,
        &SWEEP,
        &f.expiry(),
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn exit_expired_to_asset_refuses_a_fill_below_the_floor() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    f.router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &500_000_000, &1);
    f.advance_past_maturity();

    f.router.exit_expired_to_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &0,
        &f.balance(&f.pt),
        &f.expiry(),
        &IMPOSSIBLE,
        &SWEEP,
        &f.expiry(),
    );
}

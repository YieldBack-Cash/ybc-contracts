//! Base-asset zaps, exercised against a real SEP-56 vault.
//!
//! Two invariants run through these. First, a zap leaves the user's vault share
//! balance exactly where it found it — shares are an implementation detail the
//! user never asked to hold, so stranding one means a sweep was missed. Second,
//! every argument a zap takes is caller-chosen, which is what makes the
//! resulting authorization signable; `zap_auth_entries.rs` pins that property
//! under real signature-matching rules.
//!
//! See `zap_fixture.rs` for why these run against OpenZeppelin's vault rather
//! than `mock_vault`.

use soroban_sdk::Env;

use super::zap_fixture::{ZapFixture, SWEEP};

#[test]
fn zap_asset_for_pt_buys_exact_pt_and_strands_no_shares() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    let asset_before = f.balance(&f.asset);
    let pt_before = f.balance(&f.pt);
    let shares_before = f.balance(&f.vault);

    let spent = f.router.zap_asset_for_pt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &200_000_000,
        &SWEEP,
        &f.expiry(),
    );

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

    let spent = f.router.zap_asset_for_pt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &200_000_000,
        &SWEEP,
        &f.expiry(),
    );

    let shares_before = f.balance(&f.vault);
    let asset_before = f.balance(&f.asset);
    let received = f.router.zap_pt_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &1,
        &SWEEP,
        &f.expiry(),
    );

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
    // The YM deposits with itself as receiver, so shares never pass through the
    // user's account at all — not swept afterwards, never held.
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

    let spent = f.router.zap_asset_for_yt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &200_000_000,
        &SWEEP,
        &f.expiry(),
    );

    assert_eq!(f.balance(&f.yt) - yt_before, 100_000_000, "exact YT out");
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");
    // YT costs only the yield portion, far less than its face amount.
    assert!(spent > 0 && spent < 100_000_000, "YT should be cheap: {spent}");

    let asset_before = f.balance(&f.asset);
    let received = f.router.zap_yt_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &1,
        &SWEEP,
        &f.expiry(),
    );

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

    // Pool sits at 1:1, so roughly half the deposit becomes PT and the rest is
    // offered as the share leg.
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

    assert_eq!(f.pool.balance_shares(&f.user) - lp_before, lp_out);
    assert!(lp_out > 0);
    assert_eq!(f.balance(&f.vault), shares_before, "no shares stranded");

    let pt_held = f.balance(&f.pt);
    let asset_before = f.balance(&f.asset);
    let returned = f.router.zap_lp_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &lp_out,
        &pt_held,
        &1,
        &SWEEP,
        &f.expiry(),
    );

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
    let returned = f.router.exit_expired_to_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &lp,
        &10_000_000_000,
        &f.expiry(),
        &1,
        &SWEEP,
        &f.expiry(),
    );

    assert_eq!(f.balance(&f.asset) - asset_before, returned);
    // Unlike the other zaps this converts the actor's WHOLE share balance up to
    // `sweep_allowance`, not merely what the call produced — the yield manager
    // authenticates the holder, so the figure it takes has to be a ceiling the
    // user signed rather than something measured mid-call. With a generous
    // ceiling that means everything goes.
    assert!(shares_before > 0, "fixture should leave shares to absorb");
    assert_eq!(f.balance(&f.vault), 0, "every share converted to the asset");
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

// ── resource usage ───────────────────────────────────────────────────────────
//
// Zaps are the longest call chains in the system, so they are the likeliest to
// exceed a transaction's budget. On-chain each transaction gets its own budget,
// so each test zeroes the meter immediately before the call.
//
// These numbers UNDERSTATE the real cost: the market contracts run as real WASM
// here, but the router and the vault are registered natively, and those are
// exactly the two a zap adds work to. Judge by headroom, not by the assert.

/// Soroban per-transaction network limits.
const NETWORK_TX_CPU_LIMIT: u64 = 100_000_000;
const NETWORK_TX_MEM_LIMIT: u64 = 40 * 1024 * 1024;

fn assert_within_tx_budget(env: &Env, label: &str) {
    let budget = env.cost_estimate().budget();
    let (cpu, mem) = (budget.cpu_instruction_cost(), budget.memory_bytes_cost());
    std::eprintln!(
        "{label}: {cpu} CPU insns ({}% of limit), {mem} bytes",
        cpu * 100 / NETWORK_TX_CPU_LIMIT
    );
    assert!(cpu < NETWORK_TX_CPU_LIMIT, "{label} used {cpu} CPU insns, over the {NETWORK_TX_CPU_LIMIT} per-tx limit");
    assert!(mem < NETWORK_TX_MEM_LIMIT, "{label} used {mem} bytes, over the {NETWORK_TX_MEM_LIMIT} per-tx limit");
}

#[test]
fn zap_asset_for_yt_fits_network_tx_budget() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    env.cost_estimate().budget().reset_tracker();
    f.router.zap_asset_for_yt(
        &f.vault,
        &f.maturity,
        &f.user,
        &1_000_000,
        &3_000_000,
        &2_000_000,
        &SWEEP,
        &f.expiry(),
    );
    assert_within_tx_budget(&env, "zap_asset_for_yt");
}

#[test]
fn zap_asset_for_split_fits_network_tx_budget() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    env.cost_estimate().budget().reset_tracker();
    f.router
        .zap_asset_for_split(&f.vault, &f.maturity, &f.user, &100_000_000, &1);
    assert_within_tx_budget(&env, "zap_asset_for_split");
}

/// The heaviest path in the protocol: LP withdrawal, a full PT redemption, a YT
/// yield claim and a vault redeem, all in one transaction.
#[test]
fn exit_expired_to_asset_fits_network_tx_budget() {
    let env = Env::default();
    let f = ZapFixture::new(&env);
    let lp = f.pool.balance_shares(&f.user);
    f.accrue_yield(200_000_000);
    f.advance_past_maturity();

    env.cost_estimate().budget().reset_tracker();
    f.router.exit_expired_to_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &lp,
        &10_000_000_000,
        &f.expiry(),
        &1,
        &SWEEP,
        &f.expiry(),
    );
    assert_within_tx_budget(&env, "exit_expired_to_asset");
}

#[test]
#[should_panic(expected = "min_asset_out not satisfied")]
fn zap_out_respects_min_asset_out() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    f.router.zap_asset_for_pt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &200_000_000,
        &SWEEP,
        &f.expiry(),
    );
    // Demand far more than 1e8 PT could possibly fetch.
    f.router.zap_pt_for_asset(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &999_000_000,
        &SWEEP,
        &f.expiry(),
    );
}

#[test]
#[should_panic(expected = "sweep_allowance below the shares this zap produced")]
fn sweep_allowance_is_enforced() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    // A ceiling far below the leftovers this trade will produce. Failing here
    // rather than silently stranding shares is the point of the parameter.
    f.router.zap_asset_for_pt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &300_000_000,
        &200_000_000,
        &1,
        &f.expiry(),
    );
}

#[test]
#[should_panic]
fn zap_in_reverts_when_the_asset_budget_is_too_small() {
    let env = Env::default();
    let f = ZapFixture::new(&env);

    // 1_000 of the asset buys nowhere near 1e8 PT. The deposit cannot fund the
    // pool bound, so the whole zap unwinds and the user is not left holding the
    // shares the first leg produced.
    f.router.zap_asset_for_pt(
        &f.vault,
        &f.maturity,
        &f.user,
        &100_000_000,
        &1_000,
        &1_000,
        &SWEEP,
        &f.expiry(),
    );
}

//! Property-based tests (proptest layer of the fuzzing strategy).
//!
//! Fast, CI-runnable counterpart to the cargo-fuzz target in `fuzz/`:
//! the stateful test drives the same harness (`fuzz_harness::run_steps`),
//! and the math properties pin down curve behaviors that example-based
//! tests only check at hand-picked points.

use proptest::prelude::*;

use soroban_sdk::testutils::EnvTestConfig;
use soroban_sdk::{token, Env};

use crate::curve::calc_trade;
use crate::fuzz_harness::{run_steps, Harness, Step, HOLDER_FUNDS, POOL_SEED};
use crate::math::FP_SCALE;

/// Test env that skips writing a snapshot JSON per proptest case.
fn quiet_env() -> Env {
    Env::new_with_config(EnvTestConfig { capture_snapshot_at_drop: false })
}

/// Token amounts: mostly plausible trade sizes, sometimes zero/negative/huge so
/// the contract's rejection paths get exercised too.
fn amount() -> impl Strategy<Value = i128> {
    prop_oneof![
        8 => 1i128..=5_000_000_0000000i128,          // up to 5M units
        1 => Just(0i128),
        1 => any::<i64>().prop_map(|x| x as i128),   // extremes, incl. negatives
    ]
}

fn step() -> impl Strategy<Value = Step> {
    let actor = any::<u8>();
    prop_oneof![
        (actor.clone(), amount(), amount())
            .prop_map(|(actor, pt, v)| Step::Deposit { actor, pt, v }),
        (actor.clone(), amount()).prop_map(|(actor, shares)| Step::Withdraw { actor, shares }),
        (actor.clone(), amount(), amount())
            .prop_map(|(actor, pt_out, v_in_max)| Step::SwapVForPt { actor, pt_out, v_in_max }),
        (actor, amount(), amount())
            .prop_map(|(actor, pt_in, min_v_out)| Step::SwapPtForV { actor, pt_in, min_v_out }),
        (0u64..=200 * 24 * 3600).prop_map(|secs| Step::AdvanceTime { secs }),
        (500_000i128..=200_000_000i128).prop_map(|rate| Step::SetVaultRate { rate }),
    ]
}

/// Curve inputs guaranteed to sit inside the documented valid domain
/// (proportion within bounds, trade small enough not to leave it), so any
/// panic inside `calc_trade` is a genuine finding rather than a rejection.
///
/// Yields `(reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, pt_trade)`.
fn curve_domain() -> impl Strategy<Value = (i128, i128, i128, i128, i128, i128)> {
    (
        1_000_0000000i128..=100_000_000_0000000i128, // total reserves: 1k – 100M units
        15i128..=85i128,                             // PT share of pool, percent
        25i128..=2_500i128,                          // rate_scalar, whole units
        // Floor of 1.1 keeps the pre/post-trade exchange rate above 1.0 for the
        // whole proportion range (|ln(p/(1-p))|/scalar peaks at ~0.074 here),
        // clearing the below-one guard in get_exchange_rate_from_trade.
        11_000_000i128..=15_000_000i128,             // rate_anchor: 1.1 – 1.5
        FP_SCALE..=12_000_000i128,                   // fee_factor: 1.0 – 1.2
    )
        .prop_flat_map(|(total, pt_pct, scalar_units, anchor, fee)| {
            let reserve_pt = total * pt_pct / 100;
            let reserve_v = total - reserve_pt;
            // Trades capped at 10% of the PT reserve keep the post-trade
            // proportion within [MIN_PROPORTION, MAX_PROPORTION].
            (
                Just(reserve_pt),
                Just(reserve_v),
                Just(scalar_units * FP_SCALE),
                Just(anchor),
                Just(fee),
                1i128..=reserve_pt / 10,
            )
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn stateful_invariants_hold(steps in proptest::collection::vec(step(), 1..25)) {
        run_steps(&steps);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Depositing liquidity and immediately withdrawing the minted shares never
    /// returns more of EITHER token than was put in: LP entry/exit accounting
    /// can only lose rounding dust to the pool, never extract from it.
    #[test]
    fn lp_deposit_withdraw_round_trip_never_profits(
        pt in 1_0000000i128..=HOLDER_FUNDS,
        v in 1_0000000i128..=HOLDER_FUNDS,
    ) {
        let env = quiet_env();
        let harness = Harness::new(&env);
        let user = &harness.actors[1];

        let pt_token = token::TokenClient::new(&env, &harness.pt.address);
        let v_token = token::TokenClient::new(&env, &harness.vault.address);
        let pt_before = pt_token.balance(user);
        let v_before = v_token.balance(user);

        prop_assume!(harness.pool.try_deposit(user, &pt, &0, &v, &0).is_ok());
        let minted = harness.pool.balance_shares(user);
        prop_assert!(minted > 0, "accepted deposit minted no shares");

        harness.pool.withdraw(user, &minted, &0, &0);

        let net_pt = pt_token.balance(user) - pt_before;
        let net_v = v_token.balance(user) - v_before;
        prop_assert!(net_pt <= 0, "LP round trip minted {} PT from nothing", net_pt);
        prop_assert!(net_v <= 0, "LP round trip minted {} V from nothing", net_v);
    }

    /// A buy-then-sell-back swap round trip leaves the pool with the same PT,
    /// no less V, and an unchanged share supply — so the fees the trader paid
    /// land in the LPs' redeemable value rather than being skimmed or lost.
    #[test]
    fn swap_fees_accrue_to_lp_redeemable_value(x in 1_0000000i128..=POOL_SEED / 2) {
        let env = quiet_env();
        let harness = Harness::new(&env);
        let admin = &harness.actors[0];
        let trader = &harness.actors[1];

        let (pt_before, v_before) = harness.pool.get_reserves();
        let total_before = harness.pool.get_total_shares();
        let lp_shares = harness.pool.balance_shares(admin);
        let quote_pt_before = pt_before * lp_shares / total_before;
        let quote_v_before = v_before * lp_shares / total_before;

        prop_assume!(harness.pool.try_swap_v_for_pt(trader, &x, &HOLDER_FUNDS).is_ok());
        prop_assume!(harness.pool.try_swap_pt_for_v(trader, &x, &1).is_ok());

        let (pt_after, v_after) = harness.pool.get_reserves();
        let total_after = harness.pool.get_total_shares();
        prop_assert_eq!(pt_after, pt_before, "round trip changed the PT reserve");
        prop_assert!(v_after >= v_before, "round trip drained {} V from the pool", v_before - v_after);
        prop_assert_eq!(total_after, total_before, "swaps must not mint or burn LP shares");

        prop_assert!(
            pt_after * lp_shares / total_after >= quote_pt_before,
            "LP redeemable PT decreased across a fee-paying round trip"
        );
        prop_assert!(
            v_after * lp_shares / total_after >= quote_v_before,
            "LP redeemable V decreased across a fee-paying round trip"
        );
    }
}

proptest! {
    /// Buying PT and immediately selling it back can never profit the user:
    /// fees plus pool-favoring rounding must make round trips strictly lossy.
    #[test]
    fn round_trip_never_profits(
        (reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, x) in curve_domain()
    ) {
        // Buy: x PT out of the pool; user pays v_in.
        let (net_v, _, _) =
            calc_trade(reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, 0, x);
        prop_assert!(net_v < 0, "buying PT must cost V");
        let v_in = -net_v;

        // Sell the same x back at the post-trade reserves.
        let (net_v_back, _, _) = calc_trade(
            reserve_pt - x,
            reserve_v + v_in,
            rate_scalar,
            rate_anchor,
            fee_factor,
            0,
            -x,
        );
        prop_assert!(net_v_back > 0, "selling PT must return V");

        prop_assert!(
            net_v_back <= v_in,
            "round trip profited: paid {} V, got back {} V",
            v_in,
            net_v_back
        );
    }

    /// Paying for more PT never costs less: v_in is monotone in trade size.
    #[test]
    fn buy_cost_is_monotone(
        (reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, x) in curve_domain()
    ) {
        prop_assume!(x >= 2);
        let smaller = x / 2;

        let (net_small, _, _) =
            calc_trade(reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, 0, smaller);
        let (net_large, _, _) =
            calc_trade(reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, 0, x);

        prop_assert!(
            -net_large >= -net_small,
            "buying {} PT cost {} V but buying {} PT cost {} V",
            x, -net_large, smaller, -net_small
        );
    }

    /// The fee is never negative, in either trade direction.
    #[test]
    fn fee_is_non_negative(
        (reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, x) in curve_domain(),
        sell in any::<bool>()
    ) {
        let net_pt = if sell { -x } else { x };
        let (_, fee, _) =
            calc_trade(reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, 0, net_pt);
        prop_assert!(fee >= 0, "negative fee: {}", fee);
    }
}

/// Regression for the round-trip profit `round_trip_never_profits` first found.
///
/// Near expiry the fee factor shrinks toward 1.0, so the fee no longer masks
/// pricing errors. With the proportion denominator tracking the post-trade PT
/// reserve, buy and sell legs priced against different totals and this exact
/// input let a buy→sell round trip profit by 6 V units. Kept as an explicit
/// case in addition to the proptest, which only replays it via its regression
/// file.
#[test]
fn round_trip_does_not_profit_near_expiry() {
    let reserve_pt = 7_801_411_003i128;
    let reserve_v = 2_200_397_976i128;
    let rate_scalar = 11_140_000_000i128;
    let rate_anchor = 12_864_631i128;
    let fee_factor = 10_000_243i128; // ~1.0000243 — near-zero fee close to expiry
    let x = 344_975_319i128;

    // Buy x PT out; user pays v_in.
    let (net_v, _, _) =
        calc_trade(reserve_pt, reserve_v, rate_scalar, rate_anchor, fee_factor, 0, x);
    let v_in = -net_v;

    // Sell the same x back at the post-trade reserves.
    let (net_v_back, _, _) = calc_trade(
        reserve_pt - x, reserve_v + v_in, rate_scalar, rate_anchor, fee_factor, 0, -x,
    );

    assert!(
        net_v_back <= v_in,
        "round trip profited: paid {v_in} V, got back {net_v_back} V",
    );
}

proptest! {
    /// ln is strictly monotone over its useful range.
    #[test]
    fn ln_fp_is_monotone(a in 1_000i128..=1_000 * FP_SCALE, b in 1_000i128..=1_000 * FP_SCALE) {
        prop_assume!(a < b);
        prop_assert!(
            crate::math::ln_fp(a, FP_SCALE) <= crate::math::ln_fp(b, FP_SCALE),
            "ln_fp not monotone between {} and {}", a, b
        );
    }

    /// exp(ln(x)) round-trips within 0.1% over the range where both are
    /// accurate (x in [1.0, 7.0]; exp_fp is rated to inputs of ~2.0 ≈ ln 7.4).
    /// The artanh-series ln_fp is good to ~1e-5, so truncation noise dominates.
    #[test]
    fn exp_ln_round_trip(x in FP_SCALE..=7 * FP_SCALE) {
        let ln_x = crate::math::ln_fp(x, FP_SCALE);
        prop_assume!(ln_x >= 0); // exp_fp domain
        let round_trip = crate::math::exp_fp(ln_x);
        let error = (round_trip - x).abs();
        prop_assert!(
            error <= x / 1000,
            "exp(ln({})) = {} — error {} exceeds 0.1%", x, round_trip, error
        );
    }
}
//! Stateful property tests for the base-asset zaps.
//!
//! Sibling of `proptests.rs`, which drives the share-denominated router against
//! `mock_vault`. This one drives the *asset*-denominated entrypoints against a
//! real SEP-56 vault, so it covers the two boundaries the other harness cannot
//! reach: the vault deposit/redeem round trip, and the refund paths where a zap
//! hands back whatever a leg did not consume.
//!
//! Failing calls are discarded (`try_` semantics) — random amounts are rejected
//! constantly and that is fine; only invariant violations and unexpected panics
//! count. But a harness where everything is rejected proves nothing, so the
//! strategies below are shaped to keep the success rate meaningful; see the note
//! above `sell_pct()`.

use proptest::prelude::*;
use std::vec;
use std::vec::Vec;

use soroban_sdk::testutils::EnvTestConfig;
use soroban_sdk::{Address, Env, IntoVal, Symbol};

use super::zap_fixture::{ZapFixture, USER_ASSET};

/// Test env that skips writing a snapshot JSON per proptest case.
fn quiet_env() -> Env {
    Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    })
}

/// Fixture user plus two more, all arriving holding only the base asset.
const NUM_ACTORS: usize = 3;
/// At most ~400 days per step — enough to jump past the 1-year maturity, so the
/// post-maturity paths are reachable.
const MAX_TIME_STEP: u64 = 400 * 24 * 3600;
/// Cap on simulated yield, so the vault rate cannot run away into overflow
/// territory across a long sequence.
const MAX_TOTAL_YIELD: i128 = 20_000_000_000;

#[derive(Clone, Copy, Debug)]
enum Step {
    // Buy side: the actor pays in the base asset, of which they always have
    // plenty, so absolute amounts work.
    ZapAssetForPt { actor: u8, pt_out: i128, max_asset_in: i128 },
    ZapAssetForYt { actor: u8, yt_out: i128, max_asset_in: i128 },
    ZapAssetForSplit { actor: u8, asset_in: i128, min_tokens_out: i128 },
    ZapAssetForLp { actor: u8, asset_in: i128, pt_pct: u8, min_lp_out: i128 },

    // Sell side: sized as a percentage of what the actor actually holds. See
    // `sell_pct()` for why.
    ZapPtForAsset { actor: u8, pct: u8, min_asset_out: i128 },
    ZapYtForAsset { actor: u8, pct: u8, min_asset_out: i128 },
    ZapSplitForAsset { actor: u8, pct: u8, min_asset_out: i128 },
    ZapLpForAsset { actor: u8, pct: u8, min_asset_out: i128 },
    /// Post-maturity only; unwinds LP + PT + YT straight to the base asset.
    ExitExpiredToAsset { actor: u8, pct: u8, min_asset_out: i128 },

    AdvanceTime { secs: u64 },
    /// The only way this vault's rate can move: more underlying arrives.
    AccrueYield { amount: i128 },
}

impl Step {
    /// The acting address, when there is one. Also marks the step as a zap, so
    /// the no-stranded-shares invariant applies across it.
    fn actor_idx(&self) -> Option<u8> {
        match *self {
            Step::ZapAssetForPt { actor, .. }
            | Step::ZapAssetForYt { actor, .. }
            | Step::ZapAssetForSplit { actor, .. }
            | Step::ZapAssetForLp { actor, .. }
            | Step::ZapPtForAsset { actor, .. }
            | Step::ZapYtForAsset { actor, .. }
            | Step::ZapSplitForAsset { actor, .. }
            | Step::ZapLpForAsset { actor, .. }
            | Step::ExitExpiredToAsset { actor, .. } => Some(actor),
            Step::AdvanceTime { .. } | Step::AccrueYield { .. } => None,
        }
    }
}

/// `pct` of `balance`. Percentages above 100 are kept rather than clamped, so
/// over-asking (and its rejection) stays reachable.
fn portion(balance: i128, pct: u8) -> i128 {
    balance * (pct as i128) / 100
}

struct ZapHarness<'a> {
    f: ZapFixture<'a>,
    actors: Vec<Address>,
    /// Every unit of the base asset the harness has created, so conservation can
    /// be checked against a known total.
    asset_minted: i128,
    yield_accrued: i128,
    /// PT and YT move in pairs until a maturity-only path burns one alone
    /// (`redeem_principal` or a post-maturity `claim_yield`, both reachable
    /// through `exit_expired_to_asset`). After that no relation holds.
    supplies_decoupled: bool,
}

impl<'a> ZapHarness<'a> {
    fn new(env: &'a Env) -> Self {
        let f = ZapFixture::new(env);

        let mut actors = vec![f.user.clone()];
        for _ in 1..NUM_ACTORS {
            actors.push(f.add_actor());
        }

        ZapHarness {
            f,
            actors,
            asset_minted: NUM_ACTORS as i128 * USER_ASSET,
            yield_accrued: 0,
            supplies_decoupled: false,
        }
    }

    fn actor(&self, idx: u8) -> Address {
        self.actors[idx as usize % NUM_ACTORS].clone()
    }

    /// Invokes a router entrypoint, swallowing failures.
    fn try_router(&self, func: &str, args: soroban_sdk::Vec<soroban_sdk::Val>) -> bool {
        let e = &self.f.env;
        e.try_invoke_contract::<i128, soroban_sdk::Error>(
            &self.f.router.address,
            &Symbol::new(e, func),
            args,
        )
        .is_ok()
    }

    fn apply(&mut self, step: Step) {
        let e = &self.f.env;
        let (vault, maturity) = (self.f.vault.clone(), self.f.maturity);
        let f = &self.f;

        match step {
            Step::ZapAssetForPt { actor, pt_out, max_asset_in } => {
                let who = self.actor(actor);
                self.try_router(
                    "zap_asset_for_pt",
                    (&vault, maturity, &who, pt_out, max_asset_in).into_val(e),
                );
            }
            Step::ZapAssetForYt { actor, yt_out, max_asset_in } => {
                let who = self.actor(actor);
                self.try_router(
                    "zap_asset_for_yt",
                    (&vault, maturity, &who, yt_out, max_asset_in).into_val(e),
                );
            }
            Step::ZapAssetForSplit { actor, asset_in, min_tokens_out } => {
                let who = self.actor(actor);
                self.try_router(
                    "zap_asset_for_split",
                    (&vault, maturity, &who, asset_in, min_tokens_out).into_val(e),
                );
            }
            Step::ZapAssetForLp { actor, asset_in, pt_pct, min_lp_out } => {
                let who = self.actor(actor);
                // Spending roughly half the deposit on PT is what lands near the
                // pool's ratio; an absolute amount almost never does, so the
                // whole entrypoint would go untested.
                let pt_to_buy = portion(asset_in, pt_pct.min(90)) / 2;
                self.try_router(
                    "zap_asset_for_lp",
                    (&vault, maturity, &who, asset_in, pt_to_buy, min_lp_out).into_val(e),
                );
            }
            Step::ZapPtForAsset { actor, pct, min_asset_out } => {
                let who = self.actor(actor);
                let pt_in = portion(f.balance_of(&f.pt, &who), pct);
                self.try_router(
                    "zap_pt_for_asset",
                    (&vault, maturity, &who, pt_in, min_asset_out).into_val(e),
                );
            }
            Step::ZapYtForAsset { actor, pct, min_asset_out } => {
                let who = self.actor(actor);
                let yt_in = portion(f.balance_of(&f.yt, &who), pct);
                self.try_router(
                    "zap_yt_for_asset",
                    (&vault, maturity, &who, yt_in, min_asset_out).into_val(e),
                );
            }
            Step::ZapSplitForAsset { actor, pct, min_asset_out } => {
                let who = self.actor(actor);
                // Recombining burns PT and YT together, so the position is
                // whichever leg is smaller.
                let pair = f
                    .balance_of(&f.pt, &who)
                    .min(f.balance_of(&f.yt, &who));
                let amount = portion(pair, pct);
                self.try_router(
                    "zap_split_for_asset",
                    (&vault, maturity, &who, amount, min_asset_out).into_val(e),
                );
            }
            Step::ZapLpForAsset { actor, pct, min_asset_out } => {
                let who = self.actor(actor);
                let lp_shares = portion(f.pool.balance_shares(&who), pct);
                self.try_router(
                    "zap_lp_for_asset",
                    (&vault, maturity, &who, lp_shares, min_asset_out).into_val(e),
                );
            }
            Step::ExitExpiredToAsset { actor, pct, min_asset_out } => {
                let who = self.actor(actor);
                let lp_shares = portion(f.pool.balance_shares(&who), pct);
                // The exit redeems PT without a paired YT burn and claims yield
                // without a paired PT burn — either decouples the supplies.
                let touches_supply = f.balance_of(&f.pt, &who) > 0
                    || f.balance_of(&f.yt, &who) > 0
                    || (lp_shares > 0 && f.pool.balance_shares(&who) >= lp_shares);
                let ok = self.try_router(
                    "exit_expired_to_asset",
                    (&vault, maturity, &who, lp_shares, min_asset_out).into_val(e),
                );
                if ok && touches_supply {
                    self.supplies_decoupled = true;
                }
            }
            Step::AdvanceTime { secs } => {
                self.f.advance_time(secs % (MAX_TIME_STEP + 1));
            }
            Step::AccrueYield { amount } => {
                let room = MAX_TOTAL_YIELD - self.yield_accrued;
                let amount = amount.clamp(0, room.max(0));
                if amount > 0 {
                    self.f.accrue_yield(amount);
                    self.yield_accrued += amount;
                    self.asset_minted += amount;
                }
            }
        }
    }

    /// System-wide invariants, checked after every step.
    fn assert_invariants(&self) {
        let f = &self.f;

        // 1. The router is a pure conduit. It never takes custody, so it must
        //    end every operation holding none of the five things that pass
        //    through it — the base asset most of all, since that is the one the
        //    zaps introduce.
        assert_eq!(f.balance_of(&f.asset, &f.router.address), 0, "router retained asset");
        assert_eq!(f.balance_of(&f.vault, &f.router.address), 0, "router retained V");
        assert_eq!(f.balance_of(&f.pt, &f.router.address), 0, "router retained PT");
        assert_eq!(f.balance_of(&f.yt, &f.router.address), 0, "router retained YT");
        assert_eq!(f.pool.balance_shares(&f.router.address), 0, "router retained LP shares");

        // 2. Base-asset conservation. Only the harness mints, so the total
        //    across every holder — including the vault, which custodies the
        //    underlying behind its shares — must equal what was minted. A leak
        //    anywhere in a zap shows up here.
        let mut asset_total = f.balance_of(&f.asset, &f.vault)
            + f.balance_of(&f.asset, &f.router.address)
            + f.balance_of(&f.asset, &f.pool.address)
            + f.balance_of(&f.asset, &f.ym)
            + f.balance_of(&f.asset, &f.admin);
        for actor in &self.actors {
            asset_total += f.balance_of(&f.asset, actor);
        }
        assert_eq!(asset_total, self.asset_minted, "base asset not conserved");

        // 3. Reserves match balances, including across flash swaps.
        let (reserve_pt, reserve_v) = f.pool.get_reserves();
        assert_eq!(reserve_pt, f.balance_of(&f.pt, &f.pool.address), "PT reserve diverged");
        assert_eq!(reserve_v, f.balance_of(&f.vault, &f.pool.address), "V reserve diverged");
        assert!(reserve_pt > 0 && reserve_v > 0, "pool drained");

        // 4. PT and YT mint and burn in pairs until a post-maturity path burns
        //    one alone.
        if !self.supplies_decoupled {
            assert_eq!(
                f.total_supply(&f.pt),
                f.total_supply(&f.yt),
                "PT and YT supplies diverged"
            );
        }

        // 5. YM solvency: its shares, valued at the current rate, must cover the
        //    principal owed to every outstanding PT. Slack absorbs per-operation
        //    floor rounding.
        const SOLVENCY_SLACK: i128 = 100;
        let rate = f.env.invoke_contract::<i128>(
            &f.ym,
            &Symbol::new(&f.env, "get_exchange_rate"),
            soroban_sdk::Vec::new(&f.env),
        );
        let ym_assets = f.balance_of(&f.vault, &f.ym) * rate / 10_000_000;
        let pt_owed = f.total_supply(&f.pt);
        assert!(
            ym_assets + SOLVENCY_SLACK >= pt_owed,
            "YM insolvent: {ym_assets} asset backing cannot cover {pt_owed} PT principal",
        );
    }

    /// The headline property of the whole feature: a zap must leave the acting
    /// address holding exactly the vault shares it started with. Shares are an
    /// implementation detail the user never asked for, so stranding even one
    /// means a refund path was missed.
    fn assert_no_shares_stranded(&self, step: Step, before: Option<i128>) {
        if let (Some(idx), Some(before)) = (step.actor_idx(), before) {
            let who = self.actor(idx);
            assert_eq!(
                self.f.balance_of(&self.f.vault, &who),
                before,
                "{step:?} left vault shares stranded with the actor",
            );
        }
    }

    fn actor_shares(&self, step: Step) -> Option<i128> {
        step.actor_idx()
            .map(|idx| self.f.balance_of(&self.f.vault, &self.actor(idx)))
    }
}

// ── Strategies ───────────────────────────────────────────────────────────────

/// Buy-side sizes against the seeded pool; sometimes zero, negative, or huge to
/// exercise the rejection paths.
fn amount() -> impl Strategy<Value = i128> {
    prop_oneof![
        8 => 1i128..=400_000_000i128,
        1 => Just(0i128),
        1 => any::<i64>().prop_map(|x| x as i128),
    ]
}

/// Sell-side sizing, as a percentage of what the actor actually holds.
///
/// Absolute amounts do not work here. An actor's PT/YT/LP balance is whatever
/// earlier steps happened to give them, so a random absolute amount almost never
/// lands under it — measured on an earlier revision, `zap_yt_for_asset` and
/// `exit_expired_to_asset` succeeded ZERO times in 33 attempts each, making
/// those entrypoints effectively untested. Percentages track the balance
/// wherever the sequence has taken it. Values above 100 are kept so over-asking
/// stays reachable.
///
/// With this shaping every entrypoint lands somewhere between roughly 10% and
/// 85% success across a run — `exit_expired_to_asset` is the rarest, since it
/// additionally needs the sequence to have crossed maturity. If you change these
/// strategies, re-measure before trusting the result: a harness where everything
/// is rejected still passes.
fn sell_pct() -> impl Strategy<Value = u8> {
    prop_oneof![
        8 => 1u8..=100u8,
        1 => Just(0u8),
        1 => 101u8..=200u8,
    ]
}

/// Slippage floors: mostly permissive, so an operation is exercised rather than
/// rejected on its bound, but sometimes hostile to cover the rejection path.
fn min_bound() -> impl Strategy<Value = i128> {
    prop_oneof![
        5 => Just(1i128),
        1 => amount(),
    ]
}

/// Spending ceilings: mostly generous enough to let the trade through.
fn max_bound() -> impl Strategy<Value = i128> {
    prop_oneof![
        5 => 200_000_000i128..=2_000_000_000i128,
        1 => amount(),
    ]
}

/// Mostly short hops; often a jump big enough to cross the 1-year maturity,
/// since otherwise the post-maturity exit is never reached in a short sequence.
fn time_step() -> impl Strategy<Value = u64> {
    prop_oneof![
        2 => 0u64..=40 * 24 * 3600,
        1 => 300 * 24 * 3600..=MAX_TIME_STEP,
    ]
}

fn step() -> impl Strategy<Value = Step> {
    let actor = any::<u8>();
    prop_oneof![
        (actor.clone(), amount(), max_bound()).prop_map(|(actor, pt_out, max_asset_in)| {
            Step::ZapAssetForPt { actor, pt_out, max_asset_in }
        }),
        (actor.clone(), amount(), max_bound()).prop_map(|(actor, yt_out, max_asset_in)| {
            Step::ZapAssetForYt { actor, yt_out, max_asset_in }
        }),
        (actor.clone(), amount(), min_bound()).prop_map(|(actor, asset_in, min_tokens_out)| {
            Step::ZapAssetForSplit { actor, asset_in, min_tokens_out }
        }),
        (actor.clone(), amount(), sell_pct(), min_bound()).prop_map(
            |(actor, asset_in, pt_pct, min_lp_out)| Step::ZapAssetForLp {
                actor,
                asset_in,
                pt_pct,
                min_lp_out
            }
        ),
        (actor.clone(), sell_pct(), min_bound()).prop_map(|(actor, pct, min_asset_out)| {
            Step::ZapPtForAsset { actor, pct, min_asset_out }
        }),
        (actor.clone(), sell_pct(), min_bound()).prop_map(|(actor, pct, min_asset_out)| {
            Step::ZapYtForAsset { actor, pct, min_asset_out }
        }),
        (actor.clone(), sell_pct(), min_bound()).prop_map(|(actor, pct, min_asset_out)| {
            Step::ZapSplitForAsset { actor, pct, min_asset_out }
        }),
        (actor.clone(), sell_pct(), min_bound()).prop_map(|(actor, pct, min_asset_out)| {
            Step::ZapLpForAsset { actor, pct, min_asset_out }
        }),
        (actor.clone(), sell_pct(), min_bound()).prop_map(|(actor, pct, min_asset_out)| {
            Step::ExitExpiredToAsset { actor, pct, min_asset_out }
        }),
        time_step().prop_map(|secs| Step::AdvanceTime { secs }),
        (0i128..=2_000_000_000i128).prop_map(|amount| Step::AccrueYield { amount }),
    ]
}

// ── Properties ───────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn zap_stateful_invariants_hold(steps in proptest::collection::vec(step(), 1..16)) {
        let env = quiet_env();
        let mut harness = ZapHarness::new(&env);
        harness.assert_invariants();

        for step in steps {
            let shares_before = harness.actor_shares(step);
            harness.apply(step);
            harness.assert_no_shares_stranded(step, shares_before);
            harness.assert_invariants();
        }
    }

    /// Zapping into PT and straight back out again must never leave the user
    /// with more of the base asset than they started with. This is the whole
    /// round trip — vault deposit, AMM swap, vault redeem — so a rounding error
    /// that favoured the user anywhere along it would surface here.
    #[test]
    fn pt_zap_round_trip_never_profits(pt in 1_000i128..=200_000_000i128) {
        let env = quiet_env();
        let harness = ZapHarness::new(&env);
        let f = &harness.f;

        let asset_before = f.balance(&f.asset);

        let bought = f.env.try_invoke_contract::<i128, soroban_sdk::Error>(
            &f.router.address,
            &Symbol::new(&f.env, "zap_asset_for_pt"),
            (&f.vault, f.maturity, &f.user, pt, asset_before).into_val(&f.env),
        );
        prop_assume!(bought.is_ok());

        let sold = f.env.try_invoke_contract::<i128, soroban_sdk::Error>(
            &f.router.address,
            &Symbol::new(&f.env, "zap_pt_for_asset"),
            (&f.vault, f.maturity, &f.user, pt, 1i128).into_val(&f.env),
        );
        prop_assume!(sold.is_ok());

        let net = f.balance(&f.asset) - asset_before;
        prop_assert!(net <= 0, "PT zap round trip profited the user by {} asset", net);
        harness.assert_invariants();
    }

    /// Same for the YT flash-swap path, which never touches the AMM's spot
    /// reserves and so exercises a different set of conversions.
    #[test]
    fn yt_zap_round_trip_never_profits(yt in 1_000i128..=100_000_000i128) {
        let env = quiet_env();
        let harness = ZapHarness::new(&env);
        let f = &harness.f;

        let asset_before = f.balance(&f.asset);

        let bought = f.env.try_invoke_contract::<i128, soroban_sdk::Error>(
            &f.router.address,
            &Symbol::new(&f.env, "zap_asset_for_yt"),
            (&f.vault, f.maturity, &f.user, yt, asset_before).into_val(&f.env),
        );
        prop_assume!(bought.is_ok());

        let sold = f.env.try_invoke_contract::<i128, soroban_sdk::Error>(
            &f.router.address,
            &Symbol::new(&f.env, "zap_yt_for_asset"),
            (&f.vault, f.maturity, &f.user, yt, 1i128).into_val(&f.env),
        );
        prop_assume!(sold.is_ok());

        let net = f.balance(&f.asset) - asset_before;
        prop_assert!(net <= 0, "YT zap round trip profited the user by {} asset", net);
        harness.assert_invariants();
    }
}

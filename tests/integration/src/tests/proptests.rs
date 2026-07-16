//! Router-level stateful property tests.
//!
//! Integration counterpart to the AMM crate's fuzz harness: drives the FULL
//! system (router → AMM ↔ yield manager ↔ PT/YT/vault) with random operation
//! sequences, covering the YT flash-swap paths the AMM-only harness cannot
//! reach. Failing calls are discarded (`try_` semantics) — only invariant
//! violations and unexpected panics count.

use proptest::prelude::*;
use std::vec;
use std::vec::Vec;

use soroban_sdk::testutils::EnvTestConfig;
use soroban_sdk::{Env, IntoVal, Symbol};

use super::fixture::IntegrationFixture;

/// Test env that skips writing a snapshot JSON per proptest case.
fn quiet_env() -> Env {
    Env::new_with_config(EnvTestConfig { capture_snapshot_at_drop: false })
}

// Seeds mirror router_swaps.rs.
const POOL_PT: i128 = 50_000_000;
const POOL_V: i128 = 50_000_000;
const YM_DEPOSIT: i128 = 100_000_000;
/// Total V minted to the user by the fixture; nothing else mints V.
const USER_FUNDS: i128 = 1_000_000_000;

/// Vault rate only ratchets up (like a real yield vault), capped at 10x.
const MAX_VAULT_RATE: i128 = 100_000_000;
/// At most ~400 days per time step — enough to jump past the 1-year maturity,
/// making post-maturity paths (redeem_principal, expired-market rejects) reachable.
const MAX_TIME_STEP: u64 = 400 * 24 * 3600;

/// Funded actors: fixture user plus two more, all holding USER_FUNDS V.
const NUM_ACTORS: usize = 3;

#[derive(Clone, Copy, Debug)]
enum Step {
    BuyYt { actor: u8, yt_out: i128, max_v_in: i128 },
    SellYt { actor: u8, yt_in: i128, min_v_out: i128 },
    SwapVForPt { actor: u8, pt_out: i128, v_in_max: i128 },
    SwapPtForV { actor: u8, pt_in: i128, min_v_out: i128 },
    YmDeposit { actor: u8, shares: i128 },
    /// Burns PT+YT in pairs for V; pre-maturity only.
    RedeemCombined { actor: u8, amount: i128 },
    /// Burns PT alone for V; post-maturity only.
    RedeemPrincipal { actor: u8, pt_amount: i128 },
    AmmDeposit { actor: u8, pt: i128, v: i128 },
    AmmWithdraw { actor: u8, shares: i128 },
    /// One-call exit through the router; post-maturity only. Burns LP, redeems
    /// the actor's whole PT balance, claims YT yield.
    ExitExpired { actor: u8, lp_shares: i128, min_shares_out: i128 },
    AdvanceTime { secs: u64 },
    RaiseVaultRate { pct: i128 },
}

struct RouterHarness<'a> {
    f: IntegrationFixture<'a>,
    actors: Vec<soroban_sdk::Address>,
    vault_rate: i128,
    /// PT and YT are minted/burned in pairs until a maturity-only path runs:
    /// redeem_principal burns PT alone, and a post-maturity claim_yield burns
    /// the claimer's YT alone. Once either succeeds the supplies decouple in
    /// both directions and the PT==YT invariant no longer applies.
    supplies_decoupled: bool,
}

impl<'a> RouterHarness<'a> {
    /// Full-system deploy seeded like router_swaps.rs: vault rate 1.0, the
    /// fixture user holding PT+YT from a YM deposit, pool funded with balanced
    /// liquidity, plus two extra actors holding only V.
    fn new(env: &'a Env) -> Self {
        use soroban_sdk::testutils::Address as _;

        env.cost_estimate().budget().reset_unlimited();
        let f = IntegrationFixture::new(env);
        f.vault.set_exchange_rate(&10_000_000);
        f.ym_deposit(&f.user, YM_DEPOSIT);
        f.amm_deposit(&f.user, POOL_PT, POOL_V);

        let mut actors = std::vec![f.user.clone()];
        for _ in 1..NUM_ACTORS {
            let actor = soroban_sdk::Address::generate(env);
            f.vault.mint(&actor, &USER_FUNDS);
            actors.push(actor);
        }

        RouterHarness { f, actors, vault_rate: 10_000_000, supplies_decoupled: false }
    }

    fn actor(&self, idx: u8) -> soroban_sdk::Address {
        self.actors[idx as usize % NUM_ACTORS].clone()
    }

    fn try_router(&self, func: &str, args: soroban_sdk::Vec<soroban_sdk::Val>) {
        let e = &self.f.env;
        let _ = e.try_invoke_contract::<(), soroban_sdk::Error>(
            &self.f.router,
            &Symbol::new(e, func),
            args,
        );
    }

    fn apply(&mut self, step: Step) {
        let e = &self.f.env;
        match step {
            Step::BuyYt { actor, yt_out, max_v_in } => {
                let who = self.actor(actor);
                self.try_router("swap_v_for_yt", (&self.f.vault.address, self.f.maturity, &who, yt_out, max_v_in).into_val(e));
            }
            Step::SellYt { actor, yt_in, min_v_out } => {
                let who = self.actor(actor);
                self.try_router("swap_yt_for_v", (&self.f.vault.address, self.f.maturity, &who, yt_in, min_v_out).into_val(e));
            }
            Step::SwapVForPt { actor, pt_out, v_in_max } => {
                let _ = self.f.pool.try_swap_v_for_pt(&self.actor(actor), &pt_out, &v_in_max);
            }
            Step::SwapPtForV { actor, pt_in, min_v_out } => {
                let _ = self.f.pool.try_swap_pt_for_v(&self.actor(actor), &pt_in, &min_v_out);
            }
            Step::YmDeposit { actor, shares } => {
                let who = self.actor(actor);
                let expiry_ledger = e.ledger().sequence() + 1000;
                if shares > 0 {
                    self.f.vault.approve(&who, &self.f.yield_manager, &shares, &expiry_ledger);
                }
                let _ = e.try_invoke_contract::<(), soroban_sdk::Error>(
                    &self.f.yield_manager,
                    &Symbol::new(e, "deposit"),
                    (&who, shares).into_val(e),
                );
            }
            Step::RedeemCombined { actor, amount } => {
                let who = self.actor(actor);
                let _ = e.try_invoke_contract::<(), soroban_sdk::Error>(
                    &self.f.yield_manager,
                    &Symbol::new(e, "redeem_combined"),
                    (&who, amount).into_val(e),
                );
            }
            Step::RedeemPrincipal { actor, pt_amount } => {
                let who = self.actor(actor);
                let redeemed = e.try_invoke_contract::<(), soroban_sdk::Error>(
                    &self.f.yield_manager,
                    &Symbol::new(e, "redeem_principal"),
                    (&who, pt_amount).into_val(e),
                );
                if redeemed.is_ok() {
                    self.supplies_decoupled = true;
                }
            }
            Step::AmmDeposit { actor, pt, v } => {
                let who = self.actor(actor);
                let expiry_ledger = e.ledger().sequence() + 1000;
                if pt > 0 {
                    e.invoke_contract::<()>(
                        &self.f.pt,
                        &Symbol::new(e, "approve"),
                        (&who, &self.f.pool.address, pt, expiry_ledger).into_val(e),
                    );
                }
                if v > 0 {
                    self.f.vault.approve(&who, &self.f.pool.address, &v, &expiry_ledger);
                }
                let _ = self.f.pool.try_deposit(&who, &pt, &0, &v, &0);
            }
            Step::AmmWithdraw { actor, shares } => {
                let _ = self.f.pool.try_withdraw(&self.actor(actor), &shares, &0, &0);
            }
            Step::ExitExpired { actor, lp_shares, min_shares_out } => {
                let who = self.actor(actor);
                // An exit redeems the actor's PT (wallet or LP-withdrawn)
                // without a paired YT burn, and claim_yield burns their YT
                // without a paired PT burn — either decouples the supplies.
                let touches_supply = self.f.pt_balance(&who) > 0
                    || self.f.yt_balance(&who) > 0
                    || (lp_shares > 0 && self.f.pool.balance_shares(&who) >= lp_shares);
                let exited = e.try_invoke_contract::<i128, soroban_sdk::Error>(
                    &self.f.router,
                    &Symbol::new(e, "exit_expired"),
                    (&self.f.vault.address, self.f.maturity, &who, lp_shares, min_shares_out)
                        .into_val(e),
                );
                if exited.is_ok() && touches_supply {
                    self.supplies_decoupled = true;
                }
            }
            Step::AdvanceTime { secs } => {
                self.f.advance_time(secs % (MAX_TIME_STEP + 1));
            }
            Step::RaiseVaultRate { pct } => {
                let bumped = self.vault_rate * (100 + pct.clamp(0, 100)) / 100;
                self.vault_rate = bumped.min(MAX_VAULT_RATE);
                self.f.vault.set_exchange_rate(&self.vault_rate);
            }
        }
    }

    fn total_supply(&self, token: &soroban_sdk::Address) -> i128 {
        let e = &self.f.env;
        e.invoke_contract::<i128>(
            token,
            &Symbol::new(e, "total_supply"),
            soroban_sdk::Vec::new(e),
        )
    }

    /// System-wide invariants that must hold after every step.
    fn assert_invariants(&self) {
        let f = &self.f;

        // 1. AMM stored reserves match its actual token balances — including
        //    across flash swaps, where reserves re-sync from balances mid-call.
        let (reserve_pt, reserve_v) = f.pool.get_reserves();
        assert_eq!(reserve_pt, f.pt_balance(&f.pool.address), "PT reserve diverged from balance");
        assert_eq!(reserve_v, f.vault.balance(&f.pool.address), "V reserve diverged from balance");
        assert!(reserve_pt > 0 && reserve_v > 0, "pool drained");
        assert!(f.pool.get_implied_rate() >= 0, "implied rate went negative");

        // 2. PT and YT are minted/burned in pairs everywhere except the
        //    post-maturity paths: redeem_principal burns PT alone and
        //    claim_yield burns the claimer's YT alone. Supplies stay exactly
        //    equal until the first such call; afterwards they can diverge in
        //    either direction, so no relation is asserted.
        if !self.supplies_decoupled {
            assert_eq!(
                self.total_supply(&f.pt),
                self.total_supply(&f.yt),
                "PT and YT supplies diverged"
            );
        }

        // 3. The router and YM are pass-throughs for user assets: neither may
        //    retain PT, YT, or (router only) V after a completed operation.
        assert_eq!(f.vault.balance(&f.router), 0, "router retained V");
        assert_eq!(f.pt_balance(&f.router), 0, "router retained PT");
        assert_eq!(f.yt_balance(&f.router), 0, "router retained YT");
        assert_eq!(f.pt_balance(&f.yield_manager), 0, "YM retained PT");
        assert_eq!(f.yt_balance(&f.yield_manager), 0, "YM retained YT");

        // 4. V conservation: only the harness mints V, so across every holder
        //    the total never changes — no path may create or leak vault shares.
        let mut v_total = f.vault.balance(&f.admin)
            + f.vault.balance(&f.pool.address)
            + f.vault.balance(&f.yield_manager)
            + f.vault.balance(&f.router);
        for actor in &self.actors {
            v_total += f.vault.balance(actor);
        }
        assert_eq!(
            v_total,
            NUM_ACTORS as i128 * USER_FUNDS,
            "vault shares not conserved"
        );
    }
}

// ── Strategies ───────────────────────────────────────────────────────────────

/// Mostly plausible sizes against the 50M-seeded pool; sometimes zero,
/// negative, or huge to exercise rejection paths.
fn amount() -> impl Strategy<Value = i128> {
    prop_oneof![
        8 => 1i128..=30_000_000i128,
        1 => Just(0i128),
        1 => any::<i64>().prop_map(|x| x as i128),
    ]
}

/// Mostly short hops; occasionally a jump big enough to cross maturity.
fn time_step() -> impl Strategy<Value = u64> {
    prop_oneof![
        4 => 0u64..=40 * 24 * 3600,
        1 => 300 * 24 * 3600..=MAX_TIME_STEP,
    ]
}

fn step() -> impl Strategy<Value = Step> {
    let actor = any::<u8>();
    prop_oneof![
        (actor.clone(), amount(), amount())
            .prop_map(|(actor, yt_out, max_v_in)| Step::BuyYt { actor, yt_out, max_v_in }),
        (actor.clone(), amount(), amount())
            .prop_map(|(actor, yt_in, min_v_out)| Step::SellYt { actor, yt_in, min_v_out }),
        (actor.clone(), amount(), amount())
            .prop_map(|(actor, pt_out, v_in_max)| Step::SwapVForPt { actor, pt_out, v_in_max }),
        (actor.clone(), amount(), amount())
            .prop_map(|(actor, pt_in, min_v_out)| Step::SwapPtForV { actor, pt_in, min_v_out }),
        (actor.clone(), amount()).prop_map(|(actor, shares)| Step::YmDeposit { actor, shares }),
        (actor.clone(), amount())
            .prop_map(|(actor, amount)| Step::RedeemCombined { actor, amount }),
        (actor.clone(), amount())
            .prop_map(|(actor, pt_amount)| Step::RedeemPrincipal { actor, pt_amount }),
        (actor.clone(), amount(), amount())
            .prop_map(|(actor, pt, v)| Step::AmmDeposit { actor, pt, v }),
        (actor.clone(), amount())
            .prop_map(|(actor, shares)| Step::AmmWithdraw { actor, shares }),
        (actor, amount(), amount()).prop_map(|(actor, lp_shares, min_shares_out)| {
            Step::ExitExpired { actor, lp_shares, min_shares_out }
        }),
        time_step().prop_map(|secs| Step::AdvanceTime { secs }),
        (0i128..=100i128).prop_map(|pct| Step::RaiseVaultRate { pct }),
    ]
}

// ── Properties ───────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn router_stateful_invariants_hold(steps in proptest::collection::vec(step(), 1..20)) {
        let env = quiet_env();
        let mut harness = RouterHarness::new(&env);
        harness.assert_invariants();
        for step in steps {
            harness.apply(step);
            harness.assert_invariants();
        }
    }

    /// Buying YT and immediately selling it back through the router — the full
    /// flash-swap round trip across AMM, YM, and both tokens — never profits.
    #[test]
    fn router_yt_round_trip_never_profits(yt in 1_000i128..=10_000_000i128) {
        let env = quiet_env();
        let harness = RouterHarness::new(&env);
        let f = &harness.f;

        let v_before = f.vault.balance(&f.user);
        let yt_before = f.yt_balance(&f.user);

        // Buy through the router. The user's signed auth entry transfers exactly
        // max_v_in up front (surplus refunded), so the cap must not exceed their
        // balance; skip the case if the pool rejects the size anyway.
        let e = &f.env;
        let bought = e.try_invoke_contract::<(), soroban_sdk::Error>(
            &f.router,
            &Symbol::new(e, "swap_v_for_yt"),
            (&f.vault.address, f.maturity, &f.user, yt, v_before).into_val(e),
        );
        prop_assume!(bought.is_ok());
        prop_assert_eq!(f.yt_balance(&f.user), yt_before + yt);

        // Sell straight back.
        f.router_swap_yt_for_v(&f.user, yt, 1);
        prop_assert_eq!(f.yt_balance(&f.user), yt_before);

        let net = f.vault.balance(&f.user) - v_before;
        prop_assert!(net <= 0, "YT round trip profited the user by {} V", net);
        harness.assert_invariants();
    }
}
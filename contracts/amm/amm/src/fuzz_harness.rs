//! Stateful fuzz/property-test harness for the AMM.
//!
//! Shared by the cargo-fuzz target (`fuzz/fuzz_targets/amm_stateful.rs`) and the
//! proptest suite (`tests/proptests.rs`) so both exercise identical logic: apply
//! an arbitrary sequence of pool operations and assert the state invariants
//! after every step. Per Soroban fuzzing doctrine, operations go through `try_`
//! clients — a rejected call (slippage, insufficient balance, expired market) is
//! not a failure; only a violated invariant or a panic outside a `try_` call is.

use soroban_sdk::testutils::{Address as _, EnvTestConfig, Ledger};
use soroban_sdk::{token, Address, Env, String};

use crate::contract::{LiquidityPool, LiquidityPoolClient};
use mock_vault::MockVaultClient;

// Market params, mirroring tests/fixture.rs (all 1e7-scaled APYs).
const CURRENT_APY: i128 = 1_000_000; // 10%
const APY_MIN: i128 = 200_000; // 2%
const APY_MAX: i128 = 2_000_000; // 20%
const FEE_APY: i128 = 100_000; // 1%
const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;

/// Total of each token minted per holder (admin and user).
pub const HOLDER_FUNDS: i128 = 100_000_000_0000000; // 100M units at 1e7
/// Initial balanced liquidity seeded by the admin.
pub const POOL_SEED: i128 = 1_000_000_0000000; // 1M units at 1e7

/// Vault exchange-rate clamp: 0.1x – 10.0x (1e7-scaled).
const MIN_VAULT_RATE: i128 = 1_000_000;
const MAX_VAULT_RATE: i128 = 100_000_000;
/// Single time-step clamp: at most ~120 days per step.
const MAX_TIME_STEP: u64 = 120 * 24 * 3600;

/// Number of funded actors (actor 0 is the admin/LP seeder, the rest traders).
pub const NUM_ACTORS: usize = 3;

/// One randomized pool operation. `actor` picks who performs it (mod NUM_ACTORS);
/// amounts are taken as-is — invalid ones are expected to be rejected by the
/// contract, not sanitized here.
#[derive(Clone, Copy, Debug)]
pub enum Step {
    Deposit { actor: u8, pt: i128, v: i128 },
    Withdraw { actor: u8, shares: i128 },
    SwapVForPt { actor: u8, pt_out: i128, v_in_max: i128 },
    SwapPtForV { actor: u8, pt_in: i128, min_v_out: i128 },
    AdvanceTime { secs: u64 },
    SetVaultRate { rate: i128 },
}

pub struct Harness<'a> {
    pub env: Env,
    pub pt: MockVaultClient<'a>,
    pub vault: MockVaultClient<'a>,
    pub pool: LiquidityPoolClient<'a>,
    /// actors[0] is the admin (seeds initial liquidity); all are funded equally.
    pub actors: [Address; NUM_ACTORS],
}

impl<'a> Harness<'a> {
    /// Deploys tokens and pool, funds admin/user, and seeds balanced liquidity.
    /// Setup uses non-`try_` calls: a panic here is a harness bug, not a finding.
    pub fn new(env: &'a Env) -> Self {
        env.mock_all_auths();
        env.cost_estimate().budget().reset_unlimited();
        env.ledger().with_mut(|l| l.timestamp = 1_000_000);

        let actors: [Address; NUM_ACTORS] =
            core::array::from_fn(|_| Address::generate(env));
        let admin = actors[0].clone();

        // PT registered first so pt_addr < vault_addr, matching pool convention.
        let pt_addr = env.register(
            mock_vault::MockVault,
            (
                admin.clone(),
                String::from_str(env, "Principal Token"),
                String::from_str(env, "PT"),
                7u32,
            ),
        );
        let pt = MockVaultClient::new(env, &pt_addr);

        let vault_addr = env.register(
            mock_vault::MockVault,
            (
                admin.clone(),
                String::from_str(env, "Vault"),
                String::from_str(env, "VLT"),
                7u32,
            ),
        );
        let vault = MockVaultClient::new(env, &vault_addr);
        assert!(pt_addr < vault_addr, "counter addresses must be sequential");

        let ym = Address::generate(env);
        let expiry = env.ledger().timestamp() + ONE_YEAR_SECS;
        let pool_addr = env.register(
            LiquidityPool,
            (
                pt_addr,
                vault_addr,
                expiry,
                CURRENT_APY,
                APY_MIN,
                APY_MAX,
                FEE_APY,
                ym,
            ),
        );
        let pool = LiquidityPoolClient::new(env, &pool_addr);

        vault.set_exchange_rate(&10_000_000);
        for actor in &actors {
            pt.mint(actor, &HOLDER_FUNDS);
            vault.mint(actor, &HOLDER_FUNDS);
        }

        pool.deposit(&admin, &POOL_SEED, &0, &POOL_SEED, &0);

        Harness { env: env.clone(), pt, vault, pool, actors }
    }

    fn actor(&self, idx: u8) -> &Address {
        &self.actors[idx as usize % NUM_ACTORS]
    }

    /// Applies one step as the chosen actor. Contract rejections (`Err`) are
    /// discarded; only panics escaping `try_` or invariant violations count.
    pub fn apply(&self, step: Step) {
        match step {
            Step::Deposit { actor, pt, v } => {
                let (pre_pt, pre_v) = self.pool.get_reserves();
                let pre_total = self.pool.get_total_shares();
                if self.pool.try_deposit(self.actor(actor), &pt, &0, &v, &0).is_ok() {
                    self.assert_share_price_not_diluted(pre_pt, pre_v, pre_total);
                }
            }
            Step::Withdraw { actor, shares } => {
                let (pre_pt, pre_v) = self.pool.get_reserves();
                let pre_total = self.pool.get_total_shares();
                if let Ok(Ok((out_pt, out_v))) =
                    self.pool.try_withdraw(self.actor(actor), &shares, &0, &0)
                {
                    // Pro-rata exactly, floor-rounded: the pool may keep the
                    // sub-stroop dust but never short an LP a full unit — and
                    // never pay out more than the shares' proportional claim.
                    assert_eq!(out_pt, pre_pt * shares / pre_total, "withdraw paid non-pro-rata PT");
                    assert_eq!(out_v, pre_v * shares / pre_total, "withdraw paid non-pro-rata V");
                    self.assert_share_price_not_diluted(pre_pt, pre_v, pre_total);
                }
            }
            Step::SwapVForPt { actor, pt_out, v_in_max } => {
                let _ = self.pool.try_swap_v_for_pt(self.actor(actor), &pt_out, &v_in_max);
            }
            Step::SwapPtForV { actor, pt_in, min_v_out } => {
                let _ = self.pool.try_swap_pt_for_v(self.actor(actor), &pt_in, &min_v_out);
            }
            Step::AdvanceTime { secs } => {
                let secs = secs % (MAX_TIME_STEP + 1);
                self.env.ledger().with_mut(|l| l.timestamp += secs);
            }
            Step::SetVaultRate { rate } => {
                let rate = rate.clamp(MIN_VAULT_RATE, MAX_VAULT_RATE);
                self.vault.set_exchange_rate(&rate);
            }
        }
    }

    /// LP fairness across a deposit or withdraw: per-share reserves of BOTH
    /// tokens must not decrease (compared cross-multiplied to avoid division).
    /// Only liquidity ops may be checked this way — swaps legitimately shift
    /// composition, trading one reserve against the other.
    fn assert_share_price_not_diluted(&self, pre_pt: i128, pre_v: i128, pre_total: i128) {
        let (post_pt, post_v) = self.pool.get_reserves();
        let post_total = self.pool.get_total_shares();
        for (pre, post, label) in [(pre_pt, post_pt, "PT"), (pre_v, post_v, "V")] {
            assert!(
                post.checked_mul(pre_total).expect("share-price check overflow")
                    >= pre.checked_mul(post_total).expect("share-price check overflow"),
                "{} per-share reserve diluted by liquidity op",
                label
            );
        }
    }

    /// The invariants that must hold after every step, no matter what the
    /// step was or whether the contract accepted it.
    pub fn assert_invariants(&self) {
        let (reserve_pt, reserve_v) = self.pool.get_reserves();
        let pool_addr = &self.pool.address;

        let pt_token = token::TokenClient::new(&self.env, &self.pt.address);
        let v_token = token::TokenClient::new(&self.env, &self.vault.address);

        // 1. Stored reserves must exactly match the pool's actual token balances.
        assert_eq!(
            reserve_pt,
            pt_token.balance(pool_addr),
            "stored PT reserve diverged from actual pool balance"
        );
        assert_eq!(
            reserve_v,
            v_token.balance(pool_addr),
            "stored V reserve diverged from actual pool balance"
        );

        // 2. Once seeded, reserves never empty (MINIMUM_LIQUIDITY shares are locked).
        assert!(reserve_pt > 0, "PT reserve drained to zero");
        assert!(reserve_v > 0, "V reserve drained to zero");

        // 3. A negative implied rate would brick the pool: compute_rate_anchor
        //    asserts last_implied_rate >= 0, so every future trade would panic.
        assert!(self.pool.get_implied_rate() >= 0, "implied rate went negative");

        // 4. Token conservation: pool operations only move tokens between the
        //    actors and the pool; nothing is minted, burned, or leaked.
        for (tok, label) in [(&pt_token, "PT"), (&v_token, "V")] {
            let mut sum = tok.balance(pool_addr);
            for actor in &self.actors {
                sum += tok.balance(actor);
            }
            assert_eq!(sum, NUM_ACTORS as i128 * HOLDER_FUNDS, "{} tokens not conserved", label);
        }
    }
}

/// Entry point shared by the fuzz target and the stateful proptest:
/// fresh environment, seeded pool, then apply steps checking invariants.
pub fn run_steps(steps: &[Step]) {
    // No snapshot JSON per run: fuzz/proptest runs create thousands of envs.
    let env = Env::new_with_config(EnvTestConfig { capture_snapshot_at_drop: false });
    let harness = Harness::new(&env);
    harness.assert_invariants();
    for step in steps {
        harness.apply(*step);
        harness.assert_invariants();
    }
}
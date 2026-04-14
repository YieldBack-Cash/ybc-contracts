use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

use crate::contract::{LiquidityPool, LiquidityPoolClient};
use mock_vault::MockVaultClient;

// ── Constants ────────────────────────────────────────────────────────────────

const FP: i128 = 10_000_000; // 1e7 fixed-point scale

// Default market params (all 1e7-scaled)
const SCALAR_ROOT: i128 = 250_000_000;   // 25.0 — moderate curve steepness
const FEE_RATE_ROOT: i128 = 500_000;     // 0.05 — 5% annualised fee root
const INITIAL_ANCHOR: i128 = 11_000_000; // 1.1  — 10% initial implied rate anchor
const LAST_IMPLIED_RATE: i128 = 1_000_000; // 0.1 — 10% starting implied rate

const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;

// ── Fixture ──────────────────────────────────────────────────────────────────

struct AmmFixture<'a> {
    env: Env,
    /// PT token client (mock_vault registered first → lower counter address).
    pt: MockVaultClient<'a>,
    /// Vault (V) token client.
    vault: MockVaultClient<'a>,
    pool: LiquidityPoolClient<'a>,
    admin: Address,
    user: Address,
}

impl<'a> AmmFixture<'a> {
    /// Deploy env, mock PT, mock vault, and the AMM pool.
    /// Expiry is set to `now + ONE_YEAR_SECS` by default.
    ///
    /// Both tokens are registered via `env.register` so their addresses come
    /// from the same sequential counter.  PT is registered first, guaranteeing
    /// `pt_addr < vault_addr` without any randomness.
    fn new(env: &'a Env) -> Self {
        env.ledger().with_mut(|l| { l.timestamp = 1_000_000; });

        let admin = Address::generate(env);
        let user  = Address::generate(env);

        // PT registered first → lower counter address.
        let pt_addr = env.register(
            mock_vault::MockVault,
            (admin.clone(), String::from_str(env, "Principal Token"), String::from_str(env, "PT"), 7u32),
        );
        let pt = MockVaultClient::new(env, &pt_addr);

        // Vault registered second → higher counter address.
        let vault_addr = env.register(
            mock_vault::MockVault,
            (admin.clone(), String::from_str(env, "Vault"), String::from_str(env, "VLT"), 7u32),
        );
        let vault = MockVaultClient::new(env, &vault_addr);

        assert!(pt_addr < vault_addr, "counter addresses must be sequential");

        // AMM pool.
        let now    = env.ledger().timestamp();
        let expiry = now + ONE_YEAR_SECS;
        let pool_addr = env.register(
            LiquidityPool,
            (pt_addr.clone(), vault_addr.clone(), expiry, SCALAR_ROOT, INITIAL_ANCHOR, FEE_RATE_ROOT, LAST_IMPLIED_RATE),
        );
        let pool = LiquidityPoolClient::new(env, &pool_addr);

        // Set vault exchange rate to 1 (1 share = 1 asset unit, unscaled).
        // The mock default is 1e7 which makes reserve_b_assets overflow proportion checks.
        vault.set_exchange_rate(&1);

        // Fund admin and user.
        pt.mint(&admin, &1_000_000_000);
        pt.mint(&user,  &1_000_000_000);
        vault.mint(&admin, &1_000_000_000);
        vault.mint(&user,  &1_000_000_000);

        AmmFixture { env: env.clone(), pt, vault, pool, admin, user }
    }

    fn set_time(&self, ts: u64) {
        self.env.ledger().with_mut(|l| l.timestamp = ts);
    }

    fn set_vault_rate(&self, rate: i128) {
        self.vault.set_exchange_rate(&rate);
    }

    /// Seed the pool with initial liquidity (called after fixture creation).
    fn deposit(&self, from: &Address, pt_amount: i128, v_amount: i128) {
        let expiry_ledger = self.env.ledger().sequence() + 1000;
        self.pt.approve(from, &self.pool.address, &pt_amount, &expiry_ledger);
        self.vault.approve(from, &self.pool.address, &v_amount, &expiry_ledger);
        self.pool.deposit(from, &pt_amount, &0, &v_amount, &0);
    }
}

// ── Deposit tests ─────────────────────────────────────────────────────────────

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

// ── swap_v_for_pt tests ───────────────────────────────────────────────────────

#[test]
fn test_swap_v_for_pt_basic() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let pt_before = f.pt.balance(&f.user);
    let v_before = f.vault.balance(&f.user);

    let pt_out = 1_000_000i128;
    f.pool.swap_v_for_pt(&f.user, &pt_out, &100_000_000);

    let pt_after = f.pt.balance(&f.user);
    let v_after = f.vault.balance(&f.user);

    assert_eq!(pt_after - pt_before, pt_out);
    assert!(v_before > v_after, "user should have paid V in");
}

#[test]
fn test_swap_v_for_pt_updates_reserves() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);
    let (pt_res_before, v_res_before) = f.pool.get_rsrvs();

    f.pool.swap_v_for_pt(&f.user, &1_000_000, &100_000_000);

    let (pt_res_after, v_res_after) = f.pool.get_rsrvs();
    assert!(pt_res_after < pt_res_before, "PT reserve should decrease");
    assert!(v_res_after > v_res_before, "V reserve should increase");
}

#[test]
#[should_panic]
fn test_swap_v_for_pt_slippage_guard() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);
    // v_in_max set to 1 — should always be exceeded
    f.pool.swap_v_for_pt(&f.user, &1_000_000, &1);
}

#[test]
#[should_panic]
fn test_swap_v_for_pt_expired_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // Advance past expiry
    let now = env.ledger().timestamp();
    f.set_time(now + ONE_YEAR_SECS + 1);

    f.pool.swap_v_for_pt(&f.user, &1_000_000, &100_000_000);
}

// ── swap_pt_for_v tests ───────────────────────────────────────────────────────

#[test]
fn test_swap_pt_for_v_basic() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let pt_before = f.pt.balance(&f.user);
    let v_before = f.vault.balance(&f.user);

    let pt_in = 1_000_000i128;
    f.pool.swap_pt_for_v(&f.user, &pt_in, &1);

    let pt_after = f.pt.balance(&f.user);
    let v_after = f.vault.balance(&f.user);

    assert_eq!(pt_before - pt_after, pt_in);
    assert!(v_after > v_before, "user should have received V out");
}

#[test]
fn test_swap_pt_for_v_updates_reserves() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);
    let (pt_res_before, v_res_before) = f.pool.get_rsrvs();

    f.pool.swap_pt_for_v(&f.user, &1_000_000, &1);

    let (pt_res_after, v_res_after) = f.pool.get_rsrvs();
    assert!(pt_res_after > pt_res_before, "PT reserve should increase");
    assert!(v_res_after < v_res_before, "V reserve should decrease");
}

#[test]
#[should_panic]
fn test_swap_pt_for_v_slippage_guard() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);
    // min_v_out set absurdly high
    f.pool.swap_pt_for_v(&f.user, &1_000_000, &999_999_999);
}

#[test]
#[should_panic]
fn test_swap_pt_for_v_expired_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let now = env.ledger().timestamp();
    f.set_time(now + ONE_YEAR_SECS + 1);

    f.pool.swap_pt_for_v(&f.user, &1_000_000, &1);
}

// ── Convergence-to-parity test ────────────────────────────────────────────────

/// As expiry approaches, PT converges to par with V (1 PT ≈ 1 V).
/// So the V cost of buying a fixed PT amount rises toward face value over time.
/// Early in the market PT trades at a discount (costs fewer V than face value).
#[test]
fn test_pt_converges_to_parity_near_expiry() {
    let pt_out = 1_000_000i128;

    // Early trade — just after market open
    let env = Env::default();
    env.mock_all_auths();
    let f_early = AmmFixture::new(&env);
    f_early.deposit(&f_early.admin, 100_000_000, 100_000_000);
    let v_before = f_early.vault.balance(&f_early.user);
    f_early.pool.swap_v_for_pt(&f_early.user, &pt_out, &100_000_000);
    let v_cost_early = v_before - f_early.vault.balance(&f_early.user);

    // Late trade — 1 day before expiry
    let env2 = Env::default();
    env2.mock_all_auths();
    let f_late = AmmFixture::new(&env2);
    f_late.deposit(&f_late.admin, 100_000_000, 100_000_000);
    f_late.set_time(env2.ledger().timestamp() + ONE_YEAR_SECS - 86_400);
    let v_before_late = f_late.vault.balance(&f_late.user);
    f_late.pool.swap_v_for_pt(&f_late.user, &pt_out, &100_000_000);
    let v_cost_late = v_before_late - f_late.vault.balance(&f_late.user);

    // Early: PT at discount → costs less V than face value
    assert!(
        v_cost_early < pt_out,
        "early: PT should trade below par, cost={} pt_out={}",
        v_cost_early, pt_out,
    );
    // Near expiry: PT converges to par → costs more V than early
    assert!(
        v_cost_late > v_cost_early,
        "late cost should exceed early cost as PT converges to par: early={} late={}",
        v_cost_early, v_cost_late,
    );
}

// ── Withdraw tests ────────────────────────────────────────────────────────────

#[test]
fn test_withdraw_returns_proportional_tokens() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);
    assert!(shares > 0);

    let pt_before = f.pt.balance(&f.admin);
    let v_before = f.vault.balance(&f.admin);

    f.pool.withdraw(&f.admin, &shares, &0, &0);

    let pt_after = f.pt.balance(&f.admin);
    let v_after = f.vault.balance(&f.admin);

    assert!(pt_after > pt_before, "should receive PT back");
    assert!(v_after > v_before, "should receive V back");
    assert_eq!(f.pool.balance_shares(&f.admin), 0);
}

#[test]
#[should_panic]
fn test_withdraw_insufficient_shares_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);
    // Try to withdraw more than owned
    f.pool.withdraw(&f.admin, &(shares + 1), &0, &0);
}

#[test]
#[should_panic]
fn test_withdraw_min_not_satisfied_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);

    f.deposit(&f.admin, 10_000_000, 10_000_000);
    let shares = f.pool.balance_shares(&f.admin);
    // min_a set absurdly high
    f.pool.withdraw(&f.admin, &shares, &999_999_999, &0);
}

// ── Vault rate tests ──────────────────────────────────────────────────────────

/// When the vault exchange rate doubles, the same number of shares buys fewer PT
/// (because each share is now worth more underlying, shifting the curve).
#[test]
fn test_higher_vault_rate_affects_pricing() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let v_before_base = f.vault.balance(&f.user);
    f.pool.swap_v_for_pt(&f.user, &1_000_000, &100_000_000);
    let v_cost_base = v_before_base - f.vault.balance(&f.user);

    // Reset with higher vault rate
    let env2 = Env::default();
    env2.mock_all_auths();
    let f2 = AmmFixture::new(&env2);
    f2.set_vault_rate(2); // 2 assets per share
    f2.deposit(&f2.admin, 100_000_000, 100_000_000);

    let v_before_high = f2.vault.balance(&f2.user);
    f2.pool.swap_v_for_pt(&f2.user, &1_000_000, &100_000_000);
    let v_cost_high = v_before_high - f2.vault.balance(&f2.user);

    // With a higher vault rate each share converts to more assets,
    // so fewer shares are needed to cover the same PT purchase.
    assert!(
        v_cost_high < v_cost_base,
        "higher vault rate should reduce share cost: base={} high={}",
        v_cost_base,
        v_cost_high,
    );
}

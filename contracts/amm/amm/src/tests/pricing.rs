use soroban_sdk::Env;

use super::fixture::{AmmFixture, ONE_YEAR_SECS};

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
    f2.set_vault_rate(2 * 1_000_0000); // 2 assets per share (1e7-scaled)
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
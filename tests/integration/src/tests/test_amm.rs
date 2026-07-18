use soroban_sdk::Env;

use super::fixture::IntegrationFixture;

const SCALAR_7: i128 = 10_000_000;
const POOL_SEED: i128 = 50_000_000; // 5 shares each side of initial liquidity

/// Fixture: vault rate fixed at 1.0 (1 share = 1 asset) for clean curve math,
/// pool seeded with POOL_SEED PT and POOL_SEED vault shares, user keeps
/// remaining PT for swap tests.
fn seeded<'a>(env: &'a Env) -> IntegrationFixture<'a> {
    let f = IntegrationFixture::new(env);
    f.vault.set_exchange_rate(&SCALAR_7); // 1:1 so convert_to_assets(x) = x
    f.ym_deposit(&f.user, 2 * POOL_SEED); // → 2*POOL_SEED PT (user still holds vault shares)
    f.amm_deposit(&f.user, POOL_SEED, POOL_SEED);
    f
}

// ── reserves ─────────────────────────────────────────────────────────────────

#[test]
fn test_reserves_after_initial_deposit() {
    let env = Env::default();
    let f = seeded(&env);

    let (pt_res, v_res) = f.pool.get_reserves();
    assert_eq!(pt_res, POOL_SEED);
    assert_eq!(v_res, POOL_SEED);
}

// ── swap_v_for_pt (buy PT by spending vault shares) ──────────────────────────

#[test]
fn test_swap_v_for_pt_increases_pt_balance() {
    let env = Env::default();
    let f = seeded(&env);

    let pt_before = f.pt_balance(&f.user);
    let pt_out = 1_000_000i128;

    f.pool.swap_v_for_pt(&f.user, &pt_out, &(10 * SCALAR_7));

    assert_eq!(f.pt_balance(&f.user), pt_before + pt_out, "user receives exact PT");

    let (pt_res, v_res) = f.pool.get_reserves();
    assert_eq!(pt_res, POOL_SEED - pt_out, "pool PT reserve decreases");
    assert!(v_res > POOL_SEED, "pool V reserve increases");
}

#[test]
#[should_panic]
fn test_swap_v_for_pt_slippage_reverts() {
    let env = Env::default();
    let f = seeded(&env);
    // v_in_max of 1 stroop is far too tight.
    f.pool.swap_v_for_pt(&f.user, &1_000_000, &1);
}

// ── swap_pt_for_v (sell PT for vault shares) ──────────────────────────────────

#[test]
fn test_swap_pt_for_v_returns_vault_shares() {
    let env = Env::default();
    let f = seeded(&env);

    let pt_in = 1_000_000i128;
    let v_before = f.vault.balance(&f.user);

    f.pool.swap_pt_for_v(&f.user, &pt_in, &1i128);

    let v_received = f.vault.balance(&f.user) - v_before;
    assert!(v_received > 0, "user receives vault shares");

    let (pt_res, _) = f.pool.get_reserves();
    assert_eq!(pt_res, POOL_SEED + pt_in, "pool absorbs the PT");
}

#[test]
#[should_panic]
fn test_swap_pt_for_v_slippage_reverts() {
    let env = Env::default();
    let f = seeded(&env);
    f.pool.swap_pt_for_v(&f.user, &1_000_000, &999_999_999);
}

// ── LP withdraw ───────────────────────────────────────────────────────────────

#[test]
fn test_withdraw_returns_both_tokens() {
    let env = Env::default();
    let f = seeded(&env);

    let lp_shares = f.pool.balance_shares(&f.user);
    assert!(lp_shares > 0);

    let pt_before = f.pt_balance(&f.user);
    let v_before = f.vault.balance(&f.user);

    f.pool.withdraw(&f.user, &lp_shares, &0i128, &0i128);

    assert!(f.pt_balance(&f.user) > pt_before, "PT returned to LP");
    assert!(f.vault.balance(&f.user) > v_before, "vault shares returned to LP");
    assert_eq!(f.pool.balance_shares(&f.user), 0, "all LP shares burned");
}
use soroban_sdk::Env;

use super::fixture::{AmmFixture, ONE_YEAR_SECS};

// ── swap_v_for_pt ─────────────────────────────────────────────────────────────

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
    let (pt_res_before, v_res_before) = f.pool.get_reserves();

    f.pool.swap_v_for_pt(&f.user, &1_000_000, &100_000_000);

    let (pt_res_after, v_res_after) = f.pool.get_reserves();
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

    let now = env.ledger().timestamp();
    f.set_time(now + ONE_YEAR_SECS + 1);

    f.pool.swap_v_for_pt(&f.user, &1_000_000, &100_000_000);
}

// ── swap_pt_for_v ─────────────────────────────────────────────────────────────

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
    let (pt_res_before, v_res_before) = f.pool.get_reserves();

    f.pool.swap_pt_for_v(&f.user, &1_000_000, &1);

    let (pt_res_after, v_res_after) = f.pool.get_reserves();
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
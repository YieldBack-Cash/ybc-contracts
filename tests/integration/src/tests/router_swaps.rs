use soroban_sdk::Env;

use super::fixture::{IntegrationFixture, ONE_YEAR_SECS};

const POOL_PT: i128 = 50_000_000;
const POOL_V: i128 = 50_000_000;
const YM_DEPOSIT: i128 = 100_000_000;

/// Fixture with: the vault rate normalised to 1 (keeps the AMM curve math well-conditioned;
/// the YM already cached its 1e7 rate at construction, so its redemptions stay 1:1), the user
/// holding `YM_DEPOSIT` PT + YT, and the pool seeded with `POOL_PT` PT / `POOL_V` V.
fn seeded<'a>(env: &'a Env) -> IntegrationFixture<'a> {
    let f = IntegrationFixture::new(env);
    f.vault.set_exchange_rate(&1);
    f.ym_deposit(&f.user, YM_DEPOSIT);
    f.amm_deposit(&f.user, POOL_PT, POOL_V);
    f
}

// ── swap_yt_for_v (sell YT → V, via flash_swap_v) ────────────────────────────

#[test]
fn test_router_swap_yt_for_v() {
    let env = Env::default();
    let f = seeded(&env);

    let yt_before = f.yt_balance(&f.user);
    let v_before = f.vault.balance(&f.user);

    let yt_in = 1_000_000i128;
    f.router_swap_yt_for_v(&f.user, yt_in, 1);

    // User burned YT and received some V.
    assert_eq!(f.yt_balance(&f.user), yt_before - yt_in, "user's YT decreases by yt_in");
    let v_received = f.vault.balance(&f.user) - v_before;
    assert!(v_received > 0, "user receives V");
    assert!(v_received < yt_in, "received V is below face value (PT leg priced at a discount)");

    // Pool: PT reserve fell by exactly the borrowed-then-burned amount; V reserve grew.
    let (pt_res, v_res) = f.pool.get_reserves();
    assert_eq!(pt_res, POOL_PT - yt_in, "pool PT reserve drops by yt_in");
    assert!(v_res > POOL_V, "pool V reserve grew by the repayment");

    // Conservation: the redeemed V (yt_in at the 1:1 YM rate) splits between the user and the pool.
    assert_eq!(v_received + (v_res - POOL_V), yt_in, "redeemed V split between user and pool");
}

#[test]
#[should_panic]
fn test_router_swap_yt_for_v_slippage_reverts() {
    let env = Env::default();
    let f = seeded(&env);
    // min_v_out far above anything achievable for 1M YT.
    f.router_swap_yt_for_v(&f.user, 1_000_000, 999_999_999);
}

#[test]
#[should_panic]
fn test_router_swap_yt_for_v_zero_reverts() {
    let env = Env::default();
    let f = seeded(&env);
    f.router_swap_yt_for_v(&f.user, 0, 1);
}

/// When the vault rate rises, the pool needs fewer (but more valuable) shares
/// to cover the same PT price, so the user keeps a larger slice of the redeemed
/// position.
///
///   shares returned by YM  = yt_in  (fixed — YM redeems 1:1 at its stored rate)
///   shares owed to pool     = curve_price_in_assets / vault_rate  (halves as rate doubles)
///   shares kept by user     = returned − owed  (grows)
#[test]
fn test_router_swap_yt_for_v_higher_vault_rate() {
    let yt_in = 10_000_000i128;

    // Baseline swap at vault rate 1 — seeded already gives the user YT.
    let env_base = Env::default();
    let f_base = seeded(&env_base);
    let yt_before_base = f_base.yt_balance(&f_base.user);
    let v_before_base = f_base.vault.balance(&f_base.user);
    f_base.router_swap_yt_for_v(&f_base.user, yt_in, 1);
    let v_received_base = f_base.vault.balance(&f_base.user) - v_before_base;
    assert!(v_received_base > 0);
    assert_eq!(f_base.yt_balance(&f_base.user), yt_before_base - yt_in);

    // Same swap after vault rate doubles.
    let env_up = Env::default();
    let f_up = seeded(&env_up);
    f_up.vault.set_exchange_rate(&2);
    let yt_before_up = f_up.yt_balance(&f_up.user);
    let v_before_up = f_up.vault.balance(&f_up.user);
    f_up.router_swap_yt_for_v(&f_up.user, yt_in, 1);
    let v_received_up = f_up.vault.balance(&f_up.user) - v_before_up;

    // Pool takes fewer shares at the higher rate → user keeps more.
    assert!(v_received_up > v_received_base);
    assert_eq!(f_up.yt_balance(&f_up.user), yt_before_up - yt_in);
}

// ── swap_v_for_yt (buy YT with V, via flash_swap_pt) ─────────────────────────

#[test]
fn test_router_swap_v_for_yt() {
    let env = Env::default();
    let f = seeded(&env);

    let yt_before = f.yt_balance(&f.user);
    let v_before = f.vault.balance(&f.user);

    let v_in = 1_000_000i128;
    f.router_swap_v_for_yt(&f.user, v_in, 1);

    // User spent V and received YT (1:1 at this rate).
    assert_eq!(f.yt_balance(&f.user), yt_before + v_in, "user's YT increases by v_in");
    assert_eq!(v_before - f.vault.balance(&f.user), v_in, "user spent exactly v_in V");

    // Pool: PT reserve grew by the minted PT; V reserve unchanged (user's V went to the YM).
    let (pt_res, v_res) = f.pool.get_reserves();
    assert_eq!(pt_res, POOL_PT + v_in, "pool absorbed the minted PT");
    assert_eq!(v_res, POOL_V, "pool V reserve unchanged");
}

// ── swap_v_for_yt edge cases ──────────────────────────────────────────────────

#[test]
#[should_panic]
fn test_router_swap_v_for_yt_slippage_reverts() {
    let env = Env::default();
    let f = seeded(&env);
    // min_yt_out far above what 1M V can produce.
    f.router_swap_v_for_yt(&f.user, 1_000_000, 999_999_999);
}

#[test]
#[should_panic]
fn test_router_swap_v_for_yt_zero_reverts() {
    let env = Env::default();
    let f = seeded(&env);
    f.router_swap_v_for_yt(&f.user, 0, 1);
}

#[test]
#[should_panic]
fn test_router_swap_v_for_yt_expired_reverts() {
    let env = Env::default();
    let f = seeded(&env);
    f.advance_time(ONE_YEAR_SECS + 1);
    f.router_swap_v_for_yt(&f.user, 1_000_000, 1);
}

#[test]
#[should_panic]
fn test_router_swap_yt_for_v_expired_reverts() {
    let env = Env::default();
    let f = seeded(&env);
    f.advance_time(ONE_YEAR_SECS + 1);
    f.router_swap_yt_for_v(&f.user, 1_000_000, 1);
}

// ── round trip ───────────────────────────────────────────────────────────────

#[test]
fn test_router_buy_then_sell_yt_round_trip() {
    let env = Env::default();
    let f = seeded(&env);

    let yt_before = f.yt_balance(&f.user);
    f.router_swap_v_for_yt(&f.user, 1_000_000, 1);
    assert_eq!(f.yt_balance(&f.user), yt_before + 1_000_000);

    // Sell the YT we just bought; should succeed and reduce YT back toward the start.
    f.router_swap_yt_for_v(&f.user, 1_000_000, 1);
    assert_eq!(f.yt_balance(&f.user), yt_before, "YT returns to its pre-trade balance");
}

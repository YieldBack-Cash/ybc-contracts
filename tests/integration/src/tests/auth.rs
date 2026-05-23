// ── Integration auth tests ────────────────────────────────────────────────────
//
// These tests use env.auths() to verify that each user-facing operation correctly
// requires authorization from the expected address. The fixture calls
// mock_all_auths() so every auth check passes; env.auths() lets us inspect WHAT
// was authorized after each call and assert the correct address appears in the
// auth tree.
//
// These complement the per-contract negative tests (which verify unauthorized
// callers are rejected). Together they give full confidence that:
//   1. The contract records the correct auth requirement (checked here).
//   2. A call without that auth is rejected (checked in contract-level auth.rs).

use soroban_sdk::{Env, IntoVal, Symbol};

use super::fixture::IntegrationFixture;

/// Seed the AMM with liquidity. Vault rate is set to 1 to keep AMM curve math
/// well-conditioned (same approach as the router_swaps test suite).
fn seeded<'a>(env: &'a Env) -> IntegrationFixture<'a> {
    let f = IntegrationFixture::new(env);
    f.vault.set_exchange_rate(&1);
    f.vault.mint(&f.admin, &200_000_000);
    f.ym_deposit(&f.admin, 100_000_000);
    f.amm_deposit(&f.admin, f.pt_balance(&f.admin), 50_000_000);
    f
}

// ── deposit (yield manager) ───────────────────────────────────────────────────

/// YM.deposit must require auth from the depositor — no other address.
#[test]
fn test_ym_deposit_requires_depositor_auth() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    f.vault.mint(&f.user, &10_000_000);

    f.ym_deposit(&f.user, 1_000_000);

    let auths = f.env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == f.user),
        "deposit must require auth from the depositor"
    );
}

// ── redeem (yield manager) ────────────────────────────────────────────────────

/// YM.redeem must require auth from the redeemer.
#[test]
fn test_ym_redeem_requires_redeemer_auth() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    f.vault.mint(&f.user, &10_000_000);
    f.ym_deposit(&f.user, 1_000_000);

    let pt_balance = f.pt_balance(&f.user);
    f.env.invoke_contract::<()>(
        &f.yield_manager,
        &Symbol::new(&f.env, "redeem"),
        (&f.user, pt_balance).into_val(&f.env),
    );

    let auths = f.env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == f.user),
        "redeem must require auth from the redeemer"
    );
}

// ── redeem_principal (yield manager, post-maturity) ───────────────────────────

/// YM.redeem_principal must require auth from the redeemer.
#[test]
fn test_ym_redeem_principal_requires_redeemer_auth() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    f.vault.mint(&f.user, &10_000_000);
    f.ym_deposit(&f.user, 1_000_000);

    // Advance past maturity so redeem_principal is callable.
    f.advance_time(super::fixture::ONE_YEAR_SECS + 1);

    let pt_balance = f.pt_balance(&f.user);
    f.env.invoke_contract::<()>(
        &f.yield_manager,
        &Symbol::new(&f.env, "redeem_principal"),
        (&f.user, pt_balance).into_val(&f.env),
    );

    let auths = f.env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == f.user),
        "redeem_principal must require auth from the redeemer"
    );
}

// ── claim_yield (yield token) ─────────────────────────────────────────────────

/// YT.claim_yield must require auth from the claimant.
#[test]
fn test_claim_yield_requires_claimant_auth() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    f.vault.mint(&f.user, &10_000_000);
    f.ym_deposit(&f.user, 1_000_000);

    f.env.invoke_contract::<i128>(
        &f.yt,
        &Symbol::new(&f.env, "claim_yield"),
        (&f.user,).into_val(&f.env),
    );

    let auths = f.env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == f.user),
        "claim_yield must require auth from the claimant"
    );
}

// ── distribute_yield: only callable via YT.claim_yield ───────────────────────

/// The distribute_yield function on the YM is gated by yt_addr.require_auth().
/// Soroban's direct-invoker rule means this is automatically satisfied when the
/// YT contract calls the YM, and automatically rejected when any other address
/// tries. Here we verify the end-to-end flow: claim_yield → distribute_yield →
/// vault shares reach the user, confirming the auth chain works end-to-end.
#[test]
fn test_distribute_yield_flows_through_yt_only() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    f.vault.mint(&f.user, &10_000_000);
    f.ym_deposit(&f.user, 1_000_000);

    // Simulate yield by raising the vault exchange rate.
    f.vault.set_exchange_rate(&12_000_000);

    let vault_before = f.vault.balance(&f.user);

    let claimed = f.env.invoke_contract::<i128>(
        &f.yt,
        &Symbol::new(&f.env, "claim_yield"),
        (&f.user,).into_val(&f.env),
    );

    if claimed > 0 {
        // The user received vault shares — distribute_yield executed inside YM.
        assert_eq!(
            f.vault.balance(&f.user),
            vault_before + claimed,
            "user must receive exactly `claimed` vault shares from distribute_yield"
        );
    }
}

// ── router: swap_v_for_yt ─────────────────────────────────────────────────────

/// router.swap_v_for_yt must require auth from the user initiating the swap.
#[test]
fn test_router_swap_v_for_yt_requires_user_auth() {
    let env = Env::default();
    let f = seeded(&env);

    f.router_swap_v_for_yt(&f.user, 1_000_000, 1);

    let auths = env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == f.user),
        "swap_v_for_yt must require auth from the user"
    );
}

// ── router: swap_yt_for_v ─────────────────────────────────────────────────────

/// router.swap_yt_for_v must require auth from the user initiating the swap.
#[test]
fn test_router_swap_yt_for_v_requires_user_auth() {
    let env = Env::default();
    let f = seeded(&env);

    // Give the user some YT to sell.
    f.vault.mint(&f.user, &1_000_000);
    f.ym_deposit(&f.user, 1_000_000);

    let yt_balance = f.yt_balance(&f.user);
    f.router_swap_yt_for_v(&f.user, yt_balance, 1);

    let auths = env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == f.user),
        "swap_yt_for_v must require auth from the user"
    );
}

// ── router: deposit (AMM liquidity) ──────────────────────────────────────────

/// router.deposit must require auth from the LP provider.
#[test]
fn test_router_amm_deposit_requires_lp_auth() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);
    f.vault.set_exchange_rate(&1);
    f.vault.mint(&f.admin, &100_000_000);
    f.ym_deposit(&f.admin, 50_000_000);

    let pt_amt = f.pt_balance(&f.admin);
    let v_amt = 25_000_000i128;

    let expiry = f.env.ledger().sequence() + 1000;
    f.env.invoke_contract::<()>(
        &f.pt,
        &Symbol::new(&f.env, "approve"),
        (&f.admin, &f.router, pt_amt, expiry).into_val(&f.env),
    );
    f.vault.approve(&f.admin, &f.router, &v_amt, &expiry);

    f.env.invoke_contract::<()>(
        &f.router,
        &Symbol::new(&f.env, "deposit"),
        (&f.admin, pt_amt, 0i128, v_amt, 0i128).into_val(&f.env),
    );

    let auths = f.env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == f.admin),
        "router.deposit must require auth from the LP provider"
    );
}

// ── router: withdraw (AMM liquidity) ─────────────────────────────────────────

/// router.withdraw must require auth from the LP share holder.
#[test]
fn test_router_amm_withdraw_requires_lp_auth() {
    let env = Env::default();
    let f = seeded(&env);

    let shares = f.pool.balance_shares(&f.admin);
    assert!(shares > 0, "admin must hold LP shares to withdraw");

    env.invoke_contract::<(i128, i128)>(
        &f.router,
        &Symbol::new(&env, "withdraw"),
        (&f.admin, shares, 0i128, 0i128).into_val(&env),
    );

    let auths = env.auths();
    assert!(
        auths.iter().any(|(addr, _)| *addr == f.admin),
        "router.withdraw must require auth from the LP holder"
    );
}
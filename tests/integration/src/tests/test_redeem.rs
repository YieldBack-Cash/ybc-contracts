use soroban_sdk::{Env, IntoVal, Symbol};

use super::fixture::{IntegrationFixture, ONE_YEAR_SECS};

const SCALAR_7: i128 = 10_000_000;

// ── early exit (redeem PT+YT → vault shares before maturity) ─────────────────

/// Depositing X shares and immediately redeeming the same PT amount returns
/// exactly X vault shares at the initial 1.0 exchange rate.
#[test]
fn test_redeem_returns_vault_shares() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    let shares = 10 * SCALAR_7;
    f.vault.mint(&f.user, &shares);
    f.ym_deposit(&f.user, shares);

    let vault_before = f.vault.balance(&f.user);
    let pt = f.pt_balance(&f.user);

    f.env.invoke_contract::<()>(
        &f.yield_manager,
        &Symbol::new(&f.env, "redeem"),
        (&f.user, pt).into_val(&f.env),
    );

    assert_eq!(f.pt_balance(&f.user), 0, "all PT burned");
    assert_eq!(f.yt_balance(&f.user), 0, "all YT burned");
    assert_eq!(
        f.vault.balance(&f.user) - vault_before,
        shares,
        "original vault shares returned 1:1"
    );
}

/// A partial redeem burns the specified PT+YT and returns a proportional
/// share of vault tokens, leaving the remainder intact.
#[test]
fn test_partial_redeem() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    let shares = 100 * SCALAR_7;
    f.vault.mint(&f.user, &shares);
    f.ym_deposit(&f.user, shares);

    let redeem_pt = 40 * SCALAR_7;
    let vault_before = f.vault.balance(&f.user);

    f.env.invoke_contract::<()>(
        &f.yield_manager,
        &Symbol::new(&f.env, "redeem"),
        (&f.user, redeem_pt).into_val(&f.env),
    );

    assert_eq!(f.pt_balance(&f.user), shares - redeem_pt, "remaining PT intact");
    assert_eq!(
        f.vault.balance(&f.user) - vault_before,
        redeem_pt,
        "redeemed shares proportional to PT burned"
    );
}

/// redeem must panic after maturity — use redeem_principal instead.
#[test]
#[should_panic]
fn test_redeem_after_maturity_panics() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    f.vault.mint(&f.user, &SCALAR_7);
    f.ym_deposit(&f.user, SCALAR_7);
    f.advance_time(ONE_YEAR_SECS + 1);

    f.env.invoke_contract::<()>(
        &f.yield_manager,
        &Symbol::new(&f.env, "redeem"),
        (&f.user, SCALAR_7).into_val(&f.env),
    );
}

// ── principal redemption (post-maturity) ─────────────────────────────────────

/// After maturity, PT holders can redeem their principal for vault shares.
/// At the initial 1.0 rate, shares returned equal the PT amount.
#[test]
fn test_redeem_principal_after_maturity() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    let shares = 10 * SCALAR_7;
    f.vault.mint(&f.user, &shares);
    f.ym_deposit(&f.user, shares);

    f.advance_time(ONE_YEAR_SECS + 1);

    let pt = f.pt_balance(&f.user);
    let vault_before = f.vault.balance(&f.user);

    f.env.invoke_contract::<()>(
        &f.yield_manager,
        &Symbol::new(&f.env, "redeem_principal"),
        (&f.user, pt).into_val(&f.env),
    );

    assert_eq!(f.pt_balance(&f.user), 0, "all PT burned");
    assert!(
        f.vault.balance(&f.user) > vault_before,
        "vault shares returned"
    );
}

/// redeem_principal must panic before maturity.
#[test]
#[should_panic]
fn test_redeem_principal_before_maturity_panics() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    f.vault.mint(&f.user, &SCALAR_7);
    f.ym_deposit(&f.user, SCALAR_7);

    f.env.invoke_contract::<()>(
        &f.yield_manager,
        &Symbol::new(&f.env, "redeem_principal"),
        (&f.user, SCALAR_7).into_val(&f.env),
    );
}
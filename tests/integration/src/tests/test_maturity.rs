use soroban_sdk::{Env, IntoVal, Symbol};

use super::fixture::{IntegrationFixture, ONE_YEAR_SECS};

const SCALAR_7: i128 = 10_000_000;

// ── exchange rate tracking ────────────────────────────────────────────────────

/// When the vault rate rises, the yield manager's stored rate rises to match.
#[test]
fn test_exchange_rate_tracks_vault() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    let initial: i128 = f.env.invoke_contract(
        &f.yield_manager,
        &Symbol::new(&f.env, "get_exchange_rate"),
        ().into_val(&f.env),
    );

    f.vault.set_exchange_rate(&12_000_000); // 1.2x

    let updated: i128 = f.env.invoke_contract(
        &f.yield_manager,
        &Symbol::new(&f.env, "get_exchange_rate"),
        ().into_val(&f.env),
    );

    assert!(updated > initial, "rate must increase when vault rate rises");
}

/// The stored rate is a high-water mark — it never drops even if the vault rate falls.
#[test]
fn test_exchange_rate_high_water_mark() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    f.vault.set_exchange_rate(&15_000_000); // 1.5x

    let peak: i128 = f.env.invoke_contract(
        &f.yield_manager,
        &Symbol::new(&f.env, "get_exchange_rate"),
        ().into_val(&f.env),
    );

    f.vault.set_exchange_rate(&12_000_000); // drops to 1.2x

    let after_drop: i128 = f.env.invoke_contract(
        &f.yield_manager,
        &Symbol::new(&f.env, "get_exchange_rate"),
        ().into_val(&f.env),
    );

    assert_eq!(after_drop, peak, "rate must not fall below its peak");
}

// ── rate locking at maturity ──────────────────────────────────────────────────

/// Once maturity is passed, the exchange rate is locked and further vault
/// rate increases are ignored.
#[test]
fn test_exchange_rate_locked_after_maturity() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    f.vault.set_exchange_rate(&12_000_000); // 1.2x before maturity
    f.advance_time(ONE_YEAR_SECS / 2);

    // Force YM to store the 1.2x rate before crossing maturity.
    let _: i128 = f.env.invoke_contract(
        &f.yield_manager,
        &Symbol::new(&f.env, "get_exchange_rate"),
        ().into_val(&f.env),
    );

    // Cross maturity — next get_exchange_rate call will lock the rate at 1.2x.
    f.advance_time(ONE_YEAR_SECS / 2 + 1);
    let locked_rate: i128 = f.env.invoke_contract(
        &f.yield_manager,
        &Symbol::new(&f.env, "get_exchange_rate"),
        ().into_val(&f.env),
    );
    assert_eq!(locked_rate, 12_000_000, "rate locked at 1.2x");

    // Push vault rate higher — must be ignored now that rate is locked.
    f.vault.set_exchange_rate(&15_000_000); // 1.5x

    let post_maturity: i128 = f.env.invoke_contract(
        &f.yield_manager,
        &Symbol::new(&f.env, "get_exchange_rate"),
        ().into_val(&f.env),
    );

    assert_eq!(
        post_maturity, locked_rate,
        "rate must be frozen after maturity"
    );
}

// ── redeem_principal math at elevated rate ────────────────────────────────────

/// At a 1.5x vault rate, 15 PT redeems for 10 vault shares.
///
/// Deposit:   15 shares  @ rate 1.0 → mints 15 PT
/// At expiry: rate locked at 1.5
/// Redeem:    15 PT → 15 * SCALAR_7 / 15_000_000 = 10 shares
///
/// PT is fixed income: it recovers 15 *asset units*, not 15 shares. Because
/// each share is now worth 1.5 assets, 15 assets cost only 10 shares.
/// The remaining 5 shares of yield accrue to YT holders via claim_yield.
#[test]
fn test_redeem_principal_math_at_elevated_rate() {
    let env = Env::default();
    let f = IntegrationFixture::new(&env);

    let deposit_shares = 15 * SCALAR_7;
    f.vault.mint(&f.user, &deposit_shares);
    f.ym_deposit(&f.user, deposit_shares); // 15 PT + 15 YT minted

    f.vault.set_exchange_rate(&15_000_000); // rate → 1.5x
    f.advance_time(ONE_YEAR_SECS + 1);     // lock rate at maturity

    let pt = f.pt_balance(&f.user);
    let vault_before = f.vault.balance(&f.user);

    f.env.invoke_contract::<()>(
        &f.yield_manager,
        &Symbol::new(&f.env, "redeem_principal"),
        (&f.user, pt).into_val(&f.env),
    );

    let received = f.vault.balance(&f.user) - vault_before;
    assert_eq!(received, 10 * SCALAR_7, "15 PT at 1.5x rate → 10 shares");
    assert!(
        received < deposit_shares,
        "PT holder gets fewer-but-more-valuable shares; yield went to YT"
    );
}
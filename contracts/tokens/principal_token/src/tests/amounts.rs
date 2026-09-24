// ── Amount validation for PrincipalToken ─────────────────────────────────────
//
// Every mutating entrypoint must reject a negative amount. The balance helpers
// are plain arithmetic, so without the guard a negative `transfer` credits
// `from` and debits `to` — a holder's own signature would move PT out of any
// address. Regression tests for that guard; the yield token has the same one.

use soroban_sdk::{testutils::Address as _, Address, Env, String};

use crate::contract::PrincipalTokenClient;
use crate::PrincipalToken;

fn register_pt(env: &Env) -> PrincipalTokenClient<'_> {
    let admin = Address::generate(env);
    let pt_addr = env.register(
        PrincipalToken,
        (&admin, String::from_str(env, "PT"), String::from_str(env, "PT"), 7u32),
    );
    PrincipalTokenClient::new(env, &pt_addr)
}

#[test]
#[should_panic(expected = "negative amount is not allowed")]
fn transfer_rejects_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let pt = register_pt(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    pt.mint(&b, &1_000);

    pt.transfer(&a, &b, &-1);
}

#[test]
#[should_panic(expected = "negative amount is not allowed")]
fn transfer_from_rejects_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let pt = register_pt(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    pt.mint(&b, &1_000);

    pt.transfer_from(&a, &a, &b, &-1);
}

#[test]
#[should_panic(expected = "negative amount is not allowed")]
fn approve_rejects_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let pt = register_pt(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);

    pt.approve(&a, &b, &-1, &(env.ledger().sequence() + 100));
}

#[test]
#[should_panic(expected = "negative amount is not allowed")]
fn mint_rejects_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let pt = register_pt(&env);
    let a = Address::generate(&env);

    pt.mint(&a, &-1);
}

#[test]
#[should_panic(expected = "negative amount is not allowed")]
fn burn_rejects_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let pt = register_pt(&env);
    let a = Address::generate(&env);
    pt.mint(&a, &1_000);

    pt.burn(&a, &-1);
}

#[test]
#[should_panic(expected = "negative amount is not allowed")]
fn burn_from_rejects_negative_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let pt = register_pt(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    pt.mint(&a, &1_000);

    pt.burn_from(&b, &a, &-1);
}

/// Balances are untouched by a rejected call: the guard runs before any write.
#[test]
fn rejected_transfer_leaves_balances_unchanged() {
    let env = Env::default();
    env.mock_all_auths();
    let pt = register_pt(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    pt.mint(&b, &1_000);

    assert!(pt.try_transfer(&a, &b, &-600).is_err());

    assert_eq!(pt.balance(&a), 0);
    assert_eq!(pt.balance(&b), 1_000);
    assert_eq!(pt.total_supply(), 1_000);
}

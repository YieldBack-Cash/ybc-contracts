// ── Auth tests for YieldManager ───────────────────────────────────────────────
//
// Rule: never call env.mock_all_auths() here. Each test proves that a specific
// protected function rejects callers who have not provided the required auth.

use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, String, Symbol};

use crate::YieldManager;
use mock_vault::MockVault;
use yield_manager_interface::VaultType;

fn register_ym(env: &Env) -> (Address, Address, Address) {
    let admin = Address::generate(env);
    let vault_addr = env.register(
        MockVault,
        (&admin, String::from_str(env, "Mock Vault"), String::from_str(env, "MVT"), 7u32),
    );
    let maturity = env.ledger().timestamp() + 1000;
    let ym_addr = env.register(
        YieldManager,
        (&admin, &vault_addr, VaultType::Vault4626, maturity),
    );
    (admin, vault_addr, ym_addr)
}

// ── set_token_contracts ───────────────────────────────────────────────────────

/// Only the admin can register the PT and YT contract addresses.
/// A stranger's call must panic.
#[test]
#[should_panic]
fn test_set_token_contracts_non_admin_reverts() {
    let env = Env::default();
    let (_admin, _vault, ym_addr) = register_ym(&env);
    let stranger_pt = Address::generate(&env);
    let stranger_yt = Address::generate(&env);

    env.invoke_contract::<()>(
        &ym_addr,
        &Symbol::new(&env, "set_token_contracts"),
        (&stranger_pt, &stranger_yt).into_val(&env),
    );
}

// ── deposit ───────────────────────────────────────────────────────────────────

/// YM.deposit requires the depositor to authorize the call.
/// A transaction that does not carry the depositor's signature must panic.
#[test]
#[should_panic]
fn test_deposit_without_from_auth_reverts() {
    let env = Env::default();
    let (_admin, _vault, ym_addr) = register_ym(&env);
    let user = Address::generate(&env);

    env.invoke_contract::<()>(
        &ym_addr,
        &Symbol::new(&env, "deposit"),
        (&user, 1_000_000i128).into_val(&env),
    );
}

// ── redeem ────────────────────────────────────────────────────────────────────

/// YM.redeem requires the redeemer to authorize the call.
#[test]
#[should_panic]
fn test_redeem_without_from_auth_reverts() {
    let env = Env::default();
    let (_admin, _vault, ym_addr) = register_ym(&env);
    let user = Address::generate(&env);

    env.invoke_contract::<()>(
        &ym_addr,
        &Symbol::new(&env, "redeem"),
        (&user, 1_000_000i128).into_val(&env),
    );
}

// ── redeem_principal ──────────────────────────────────────────────────────────

/// YM.redeem_principal requires the redeemer to authorize the call.
#[test]
#[should_panic]
fn test_redeem_principal_without_from_auth_reverts() {
    let env = Env::default();
    let (_admin, _vault, ym_addr) = register_ym(&env);
    let user = Address::generate(&env);

    env.invoke_contract::<()>(
        &ym_addr,
        &Symbol::new(&env, "redeem_principal"),
        (&user, 1_000_000i128).into_val(&env),
    );
}
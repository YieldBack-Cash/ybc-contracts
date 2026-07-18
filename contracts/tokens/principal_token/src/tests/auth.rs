// ── Auth tests for PrincipalToken ────────────────────────────────────────────
//
// Rule: never call env.mock_all_auths() here. Each test proves that a specific
// protected function rejects callers who have not provided the required auth.
// Soroban host panics with an auth error before any state is read or written.

use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, String, Symbol};

use crate::PrincipalToken;

fn register_pt(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env);
    let pt_addr = env.register(
        PrincipalToken,
        (&admin, String::from_str(env, "PT"), String::from_str(env, "PT"), 7u32),
    );
    (admin, pt_addr)
}

/// Only the admin (yield manager) can mint PT. A stranger's call must panic.
#[test]
#[should_panic]
fn test_mint_non_admin_reverts() {
    let env = Env::default();
    let (_admin, pt_addr) = register_pt(&env);
    let stranger = Address::generate(&env);

    env.invoke_contract::<()>(
        &pt_addr,
        &Symbol::new(&env, "mint"),
        (&stranger, 1_000_000i128).into_val(&env),
    );
}

/// PT.burn is admin-gated — not from-gated — meaning token holders cannot
/// burn their own PT directly. Only the yield manager (admin) can trigger burns.
/// A stranger calling burn must panic even if passed as the `from` address.
#[test]
#[should_panic]
fn test_burn_non_admin_reverts() {
    let env = Env::default();
    let (_admin, pt_addr) = register_pt(&env);
    let holder = Address::generate(&env);

    env.invoke_contract::<()>(
        &pt_addr,
        &Symbol::new(&env, "burn"),
        (&holder, 1_000_000i128).into_val(&env),
    );
}

/// PT.transfer requires the sender to authorize the transfer.
#[test]
#[should_panic]
fn test_transfer_without_from_auth_reverts() {
    let env = Env::default();
    let (_admin, pt_addr) = register_pt(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    env.invoke_contract::<()>(
        &pt_addr,
        &Symbol::new(&env, "transfer"),
        (&from, &to, 1_000_000i128).into_val(&env),
    );
}

/// PT.approve requires the owner to authorize the allowance grant.
#[test]
#[should_panic]
fn test_approve_without_owner_auth_reverts() {
    let env = Env::default();
    let (_admin, pt_addr) = register_pt(&env);
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let expiry = env.ledger().sequence() + 100;

    env.invoke_contract::<()>(
        &pt_addr,
        &Symbol::new(&env, "approve"),
        (&owner, &spender, 1_000_000i128, expiry).into_val(&env),
    );
}
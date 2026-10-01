// ── Auth tests for PrincipalToken ────────────────────────────────────────────
//
// Rule: never call env.mock_all_auths() here. Each test proves that a specific
// protected function rejects callers who have not provided the required auth.

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, Symbol,
};

use super::register_pt;

/// Only the admin (yield manager) can mint PT. With no admin signature, `mint`
/// must panic.
#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_mint_non_admin_reverts() {
    let env = Env::default();
    let pt_addr = register_pt(&env).address;
    let stranger = Address::generate(&env);

    env.invoke_contract::<()>(
        &pt_addr,
        &Symbol::new(&env, "mint"),
        (&stranger, 1_000_000i128).into_val(&env),
    );
}

/// PT.burn is admin-gated, not from-gated: holders cannot retire their own PT,
/// only the yield manager can. The holder signs here, so the admin check is
/// the only thing that can fail.
#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_burn_non_admin_reverts() {
    let env = Env::default();
    let pt_addr = register_pt(&env).address;
    let holder = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &holder,
        invoke: &MockAuthInvoke {
            contract: &pt_addr,
            fn_name: "burn",
            args: (&holder, 1_000_000i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    env.invoke_contract::<()>(
        &pt_addr,
        &Symbol::new(&env, "burn"),
        (&holder, 1_000_000i128).into_val(&env),
    );
}

/// PT.transfer requires the sender to authorize the transfer.
#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_transfer_without_from_auth_reverts() {
    let env = Env::default();
    let pt_addr = register_pt(&env).address;
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
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn test_approve_without_owner_auth_reverts() {
    let env = Env::default();
    let pt_addr = register_pt(&env).address;
    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let expiry = env.ledger().sequence() + 100;

    env.invoke_contract::<()>(
        &pt_addr,
        &Symbol::new(&env, "approve"),
        (&owner, &spender, 1_000_000i128, expiry).into_val(&env),
    );
}

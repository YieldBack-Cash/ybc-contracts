// ── Auth tests for YieldToken ─────────────────────────────────────────────────
//
// Rule: never call env.mock_all_auths() here. Each test proves that a specific
// protected function rejects callers who have not provided the required auth.

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, String, Symbol,
};

use crate::YieldToken;

fn register_yt(env: &Env) -> (Address, Address) {
    let admin = Address::generate(env); // stands in for the yield manager address
    let yt_addr = env.register(
        YieldToken,
        (&admin, String::from_str(env, "YT"), String::from_str(env, "YT"), 7u32),
    );
    (admin, yt_addr)
}

/// Only the admin (yield manager) can mint YT. A stranger's call must panic.
#[test]
#[should_panic]
fn test_mint_non_admin_reverts() {
    let env = Env::default();
    let (_admin, yt_addr) = register_yt(&env);
    let stranger = Address::generate(&env);

    env.invoke_contract::<()>(
        &yt_addr,
        &Symbol::new(&env, "mint"),
        (&stranger, 1_000_000i128, 10_000_000i128).into_val(&env),
    );
}

/// YT.transfer requires the sender to authorize the transfer.
#[test]
#[should_panic]
fn test_transfer_without_from_auth_reverts() {
    let env = Env::default();
    let (_admin, yt_addr) = register_yt(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    env.invoke_contract::<()>(
        &yt_addr,
        &Symbol::new(&env, "transfer"),
        (&from, &to, 1_000_000i128).into_val(&env),
    );
}

/// YT.transfer_with_rate requires the sender to authorize.
#[test]
#[should_panic]
fn test_transfer_with_rate_without_from_auth_reverts() {
    let env = Env::default();
    let (_admin, yt_addr) = register_yt(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    env.invoke_contract::<()>(
        &yt_addr,
        &Symbol::new(&env, "transfer_with_rate"),
        (&from, &to, 1_000_000i128, 10_000_000i128).into_val(&env),
    );
}

/// transfer_with_rate accepts a caller-supplied exchange_rate, so it must also
/// require the yield manager (admin) to authorize -- otherwise any holder
/// could pass an inflated rate to inflate their own accrued_yield. The
/// sender's auth alone must not be enough.
#[test]
#[should_panic]
fn test_transfer_with_rate_without_admin_auth_reverts() {
    let env = Env::default();
    let (_admin, yt_addr) = register_yt(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &from,
        invoke: &MockAuthInvoke {
            contract: &yt_addr,
            fn_name: "transfer_with_rate",
            args: (&from, &to, 1_000_000i128, 10_000_000i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    env.invoke_contract::<()>(
        &yt_addr,
        &Symbol::new(&env, "transfer_with_rate"),
        (&from, &to, 1_000_000i128, 10_000_000i128).into_val(&env),
    );
}

/// With both the sender's and the yield manager's auth present (the only way
/// this function is invoked in practice, via YieldManager::redeem_combined /
/// on_flash_receive_v), transfer_with_rate succeeds.
#[test]
fn test_transfer_with_rate_with_from_and_admin_auth_succeeds() {
    let env = Env::default();
    let (admin, yt_addr) = register_yt(&env);
    let from = Address::generate(&env);
    let to = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &yt_addr,
            fn_name: "mint",
            args: (&from, 1_000_000i128, 10_000_000i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    env.invoke_contract::<()>(
        &yt_addr,
        &Symbol::new(&env, "mint"),
        (&from, 1_000_000i128, 10_000_000i128).into_val(&env),
    );

    env.mock_auths(&[
        MockAuth {
            address: &from,
            invoke: &MockAuthInvoke {
                contract: &yt_addr,
                fn_name: "transfer_with_rate",
                args: (&from, &to, 500_000i128, 10_000_000i128).into_val(&env),
                sub_invokes: &[],
            },
        },
        MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &yt_addr,
                fn_name: "transfer_with_rate",
                args: (&from, &to, 500_000i128, 10_000_000i128).into_val(&env),
                sub_invokes: &[],
            },
        },
    ]);

    env.invoke_contract::<()>(
        &yt_addr,
        &Symbol::new(&env, "transfer_with_rate"),
        (&from, &to, 500_000i128, 10_000_000i128).into_val(&env),
    );
}

/// burn_with_rate takes the same caller-supplied exchange_rate as
/// transfer_with_rate, so it must also require the yield manager's auth.
#[test]
#[should_panic]
fn test_burn_with_rate_without_admin_auth_reverts() {
    let env = Env::default();
    let (_admin, yt_addr) = register_yt(&env);
    let from = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &from,
        invoke: &MockAuthInvoke {
            contract: &yt_addr,
            fn_name: "burn_with_rate",
            args: (&from, 1_000_000i128, 10_000_000i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    env.invoke_contract::<()>(
        &yt_addr,
        &Symbol::new(&env, "burn_with_rate"),
        (&from, 1_000_000i128, 10_000_000i128).into_val(&env),
    );
}

/// YT.burn requires the holder to authorize the burn.
#[test]
#[should_panic]
fn test_burn_without_from_auth_reverts() {
    let env = Env::default();
    let (_admin, yt_addr) = register_yt(&env);
    let holder = Address::generate(&env);

    env.invoke_contract::<()>(
        &yt_addr,
        &Symbol::new(&env, "burn"),
        (&holder, 1_000_000i128).into_val(&env),
    );
}

/// YT.claim_yield requires the user to authorize their own claim.
#[test]
#[should_panic]
fn test_claim_yield_without_user_auth_reverts() {
    let env = Env::default();
    let (_admin, yt_addr) = register_yt(&env);
    let user = Address::generate(&env);

    env.invoke_contract::<i128>(
        &yt_addr,
        &Symbol::new(&env, "claim_yield"),
        (&user,).into_val(&env),
    );
}
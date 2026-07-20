// ── Auth tests for LiquidityPool (AMM) ────────────────────────────────────────
//
// Rule: never call env.mock_all_auths() here. Each test proves that a specific
// protected function rejects callers who have not provided the required auth.

use soroban_sdk::{testutils::Address as _, Address, Env, IntoVal, String, Symbol};

use crate::contract::LiquidityPool;
use mock_vault::MockVault;

const CURRENT_APY: i128 = 1_000_000;
const APY_MIN: i128 = 200_000;
const APY_MAX: i128 = 2_000_000;
const FEE_APY: i128 = 100_000;
const ONE_YEAR_SECS: u64 = 365 * 24 * 3600;

fn register_pool(env: &Env) -> (Address, Address, Address) {
    let admin = Address::generate(env);

    let pt_addr = env.register(
        MockVault,
        (&admin, String::from_str(env, "PT"), String::from_str(env, "PT"), 7u32),
    );
    let vault_addr = env.register(
        MockVault,
        (&admin, String::from_str(env, "Vault"), String::from_str(env, "VLT"), 7u32),
    );

    let expiry = env.ledger().timestamp() + ONE_YEAR_SECS;
    let ym = Address::generate(env);
    let pool_addr = env.register(
        LiquidityPool,
        (&pt_addr, &vault_addr, expiry, CURRENT_APY, APY_MIN, APY_MAX, FEE_APY, &ym),
    );
    (pt_addr, vault_addr, pool_addr)
}

/// AMM.deposit requires the liquidity provider to authorize the call.
#[test]
#[should_panic]
fn test_deposit_without_to_auth_reverts() {
    let env = Env::default();
    let (_pt, _vault, pool_addr) = register_pool(&env);
    let user = Address::generate(&env);

    env.invoke_contract::<()>(
        &pool_addr,
        &Symbol::new(&env, "deposit"),
        (&user, 1_000_000i128, 0i128, 1_000_000i128, 0i128).into_val(&env),
    );
}

/// AMM.withdraw requires the LP share holder to authorize the call.
#[test]
#[should_panic]
fn test_withdraw_without_to_auth_reverts() {
    let env = Env::default();
    let (_pt, _vault, pool_addr) = register_pool(&env);
    let user = Address::generate(&env);

    env.invoke_contract::<(i128, i128)>(
        &pool_addr,
        &Symbol::new(&env, "withdraw"),
        (&user, 1_000_000i128, 0i128, 0i128).into_val(&env),
    );
}

/// AMM.swap_v_for_pt requires the swapper to authorize the call.
#[test]
#[should_panic]
fn test_swap_v_for_pt_without_to_auth_reverts() {
    let env = Env::default();
    let (_pt, _vault, pool_addr) = register_pool(&env);
    let user = Address::generate(&env);

    env.invoke_contract::<()>(
        &pool_addr,
        &Symbol::new(&env, "swap_v_for_pt"),
        (&user, 1_000_000i128, 1_000_000i128).into_val(&env),
    );
}

/// AMM.swap_pt_for_v requires the swapper to authorize the call.
#[test]
#[should_panic]
fn test_swap_pt_for_v_without_to_auth_reverts() {
    let env = Env::default();
    let (_pt, _vault, pool_addr) = register_pool(&env);
    let user = Address::generate(&env);

    env.invoke_contract::<()>(
        &pool_addr,
        &Symbol::new(&env, "swap_pt_for_v"),
        (&user, 1_000_000i128, 1i128).into_val(&env),
    );
}
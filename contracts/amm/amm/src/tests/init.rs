// Constructor bounds: creator-supplied APY params outside the protocol's
// ranges must be rejected at deployment, since the curve they produce is
// degenerate (see the bound constants in contract.rs).

use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, String};

use crate::contract::LiquidityPool;
use crate::tests::fixture::{APY_MAX, APY_MIN, CURRENT_APY, FEE_APY, ONE_YEAR_SECS};
use mock_vault::MockVault;

fn register_pool(env: &Env, current_apy: i128, apy_min: i128, apy_max: i128, fee_apy: i128) {
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
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
    let treasury = Address::generate(env);
    env.register(
        LiquidityPool,
        (&pt_addr, &vault_addr, expiry, current_apy, apy_min, apy_max, fee_apy, &ym, &treasury, 0i128),
    );
}

#[test]
fn test_valid_params_derive_expected_implied_rate() {
    let env = Env::default();
    let admin = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let pt_addr = env.register(
        MockVault,
        (&admin, String::from_str(&env, "PT"), String::from_str(&env, "PT"), 7u32),
    );
    let vault_addr = env.register(
        MockVault,
        (&admin, String::from_str(&env, "Vault"), String::from_str(&env, "VLT"), 7u32),
    );
    let expiry = env.ledger().timestamp() + ONE_YEAR_SECS;
    let ym = Address::generate(&env);
    let treasury = Address::generate(&env);
    let pool_addr = env.register(
        LiquidityPool,
        (&pt_addr, &vault_addr, expiry, CURRENT_APY, APY_MIN, APY_MAX, FEE_APY, &ym, &treasury, 0i128),
    );
    let pool = crate::contract::LiquidityPoolClient::new(&env, &pool_addr);

    // 10% APY seeds an ln implied rate of ln(1.10) ≈ 0.0953102 (1e7-scaled).
    let rate = pool.get_implied_rate();
    assert!(
        (rate - 953_102).abs() < 200,
        "derived implied rate {} not near ln(1.10)",
        rate
    );
}

#[test]
#[should_panic(expected = "band too narrow")]
fn test_band_too_narrow_panics() {
    let env = Env::default();
    // 0.5-point band, below the 1-point minimum.
    register_pool(&env, 1_000_000, 980_000, 1_030_000, FEE_APY);
}

#[test]
#[should_panic(expected = "apy_max too high")]
fn test_apy_max_above_cap_panics() {
    let env = Env::default();
    // 150% top of band, above the 100% cap.
    register_pool(&env, 1_000_000, 200_000, 15_000_000, FEE_APY);
}

#[test]
#[should_panic(expected = "current_apy must be inside the band")]
fn test_current_apy_below_band_panics() {
    let env = Env::default();
    register_pool(&env, 100_000, APY_MIN, APY_MAX, FEE_APY);
}

#[test]
#[should_panic(expected = "current_apy must be inside the band")]
fn test_current_apy_above_band_panics() {
    let env = Env::default();
    register_pool(&env, 3_000_000, APY_MIN, APY_MAX, FEE_APY);
}

#[test]
#[should_panic(expected = "apy_min must be non-negative")]
fn test_negative_apy_min_panics() {
    let env = Env::default();
    register_pool(&env, CURRENT_APY, -100_000, APY_MAX, FEE_APY);
}

#[test]
#[should_panic(expected = "fee_apy out of range")]
fn test_zero_fee_panics() {
    let env = Env::default();
    register_pool(&env, CURRENT_APY, APY_MIN, APY_MAX, 0);
}

#[test]
#[should_panic(expected = "fee_apy out of range")]
fn test_fee_above_cap_panics() {
    let env = Env::default();
    // 5% fee spread, above the 2% cap.
    register_pool(&env, CURRENT_APY, APY_MIN, APY_MAX, 500_000);
}
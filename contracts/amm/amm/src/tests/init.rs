// Constructor bounds: creator-supplied APY params outside the protocol's
// ranges must be rejected at deployment, since the curve they produce is
// degenerate (see the bound constants in contract.rs).

use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, String};

use crate::contract::LiquidityPool;
use crate::tests::fixture::{APY_MAX, APY_MIN, CURRENT_APY, FEE_APY, ONE_YEAR_SECS};
use mock_vault::MockVault;

fn register_pool(env: &Env, current_apy: i128, apy_min: i128, apy_max: i128, fee_apy: i128) {
    register_pool_with_expiry(env, current_apy, apy_min, apy_max, fee_apy, ONE_YEAR_SECS);
}

fn register_pool_with_expiry(
    env: &Env,
    current_apy: i128,
    apy_min: i128,
    apy_max: i128,
    fee_apy: i128,
    secs_to_expiry: u64,
) -> crate::contract::LiquidityPoolClient<'_> {
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    let admin = Address::generate(env);

    let pt_addr = env.register(
        MockVault,
        (
            &admin,
            String::from_str(env, "PT"),
            String::from_str(env, "PT"),
            7u32,
        ),
    );
    let vault_addr = env.register(
        MockVault,
        (
            &admin,
            String::from_str(env, "Vault"),
            String::from_str(env, "VLT"),
            7u32,
        ),
    );

    let expiry = env.ledger().timestamp() + secs_to_expiry;
    let ym = Address::generate(env);
    let treasury = Address::generate(env);
    let pool = env.register(
        LiquidityPool,
        (
            &pt_addr,
            &vault_addr,
            expiry,
            current_apy,
            apy_min,
            apy_max,
            fee_apy,
            &ym,
            &treasury,
            0i128,
        ),
    );
    crate::contract::LiquidityPoolClient::new(env, &pool)
}

#[test]
fn test_valid_params_derive_expected_implied_rate() {
    let env = Env::default();
    let admin = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let pt_addr = env.register(
        MockVault,
        (
            &admin,
            String::from_str(&env, "PT"),
            String::from_str(&env, "PT"),
            7u32,
        ),
    );
    let vault_addr = env.register(
        MockVault,
        (
            &admin,
            String::from_str(&env, "Vault"),
            String::from_str(&env, "VLT"),
            7u32,
        ),
    );
    let expiry = env.ledger().timestamp() + ONE_YEAR_SECS;
    let ym = Address::generate(&env);
    let treasury = Address::generate(&env);
    let pool_addr = env.register(
        LiquidityPool,
        (
            &pt_addr,
            &vault_addr,
            expiry,
            CURRENT_APY,
            APY_MIN,
            APY_MAX,
            FEE_APY,
            &ym,
            &treasury,
            0i128,
        ),
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
#[should_panic(expected = "Error(Contract, #4)")]
fn test_band_too_narrow_panics() {
    let env = Env::default();
    // 0.5-point band, below the 1-point minimum.
    register_pool(&env, 1_000_000, 980_000, 1_030_000, FEE_APY);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_apy_max_above_cap_panics() {
    let env = Env::default();
    // 15,000% top of band, above the competition branch's 10,000% cap.
    register_pool(&env, 1_000_000, 200_000, 1_500_000_000, FEE_APY);
}

// ── Competition branch: high rates at short maturities ───────────────────────

#[test]
fn test_competition_rate_at_one_year_constructs() {
    let env = Env::default();
    // 2000% opening rate in a 200%–3000% band, one year out: ln(21) × 1.0 is
    // 3.04, inside MAX_RATE_TIME. The implied rate is ln(1 + 20) = 3.0445224.
    let pool = register_pool_with_expiry(
        &env,
        200_000_000,
        20_000_000,
        300_000_000,
        FEE_APY,
        ONE_YEAR_SECS,
    );
    let rate = pool.get_implied_rate();
    assert!(
        (rate - 30_445_224).abs() < 5_000,
        "derived implied rate {} not near ln(21)",
        rate
    );
}

#[test]
fn test_competition_rate_at_six_hours_constructs() {
    let env = Env::default();
    // The live-event shape: six hours at 2000%, fee at 5% of the band width.
    let pool = register_pool_with_expiry(
        &env,
        200_000_000,
        20_000_000,
        300_000_000,
        140_000_000,
        6 * 3600,
    );
    assert!(pool.get_implied_rate() > 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #26)")]
fn test_rate_time_product_too_large_panics() {
    let env = Env::default();
    // The same band ten years out: ln(31) × 10 ≈ 34, far past MAX_RATE_TIME.
    register_pool_with_expiry(
        &env,
        200_000_000,
        20_000_000,
        300_000_000,
        FEE_APY,
        10 * ONE_YEAR_SECS,
    );
}

#[test]
fn test_main_branch_limit_still_constructs() {
    let env = Env::default();
    // What main allows at its edge: a 100% top of band for ten years,
    // ln(2) × 10 = 6.93, just under MAX_RATE_TIME. Nothing legal on main is
    // rejected here.
    register_pool_with_expiry(
        &env,
        5_000_000,
        1_000_000,
        10_000_000,
        FEE_APY,
        10 * ONE_YEAR_SECS,
    );
}

#[test]
fn test_zero_fee_constructs() {
    let env = Env::default();
    // A fee-free heat is a deliberate choice on this branch, not an error.
    let pool = register_pool_with_expiry(&env, CURRENT_APY, APY_MIN, APY_MAX, 0, ONE_YEAR_SECS);
    assert!(pool.get_implied_rate() > 0);
}

#[test]
fn test_fee_at_half_the_band_constructs() {
    let env = Env::default();
    register_pool(&env, CURRENT_APY, APY_MIN, APY_MAX, (APY_MAX - APY_MIN) / 2);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_current_apy_below_band_panics() {
    let env = Env::default();
    register_pool(&env, 100_000, APY_MIN, APY_MAX, FEE_APY);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_current_apy_above_band_panics() {
    let env = Env::default();
    register_pool(&env, 3_000_000, APY_MIN, APY_MAX, FEE_APY);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_negative_apy_min_panics() {
    let env = Env::default();
    register_pool(&env, CURRENT_APY, -100_000, APY_MAX, FEE_APY);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_negative_fee_panics() {
    let env = Env::default();
    register_pool(&env, CURRENT_APY, APY_MIN, APY_MAX, -1);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_fee_above_half_the_band_panics() {
    let env = Env::default();
    // One unit over half the band width.
    register_pool(
        &env,
        CURRENT_APY,
        APY_MIN,
        APY_MAX,
        (APY_MAX - APY_MIN) / 2 + 1,
    );
}

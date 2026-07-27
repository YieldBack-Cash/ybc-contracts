// ── Reserve (protocol) fee tests ─────────────────────────────────────────────
//
// The treasury's cut is a 1e7-scaled fraction OF the trading fee, not of the
// trade. It is remitted inline to the treasury on every swap and flash swap,
// and must never enter LP accounting: stored reserves have to keep matching
// the pool's actual balances exactly.

use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, String};

use super::fixture::{AmmFixture, APY_MAX, APY_MIN, CURRENT_APY, FEE_APY, ONE_YEAR_SECS};
use super::flash::{MockFlashPtReceiver, MockFlashVReceiver};
use crate::contract::LiquidityPool;
use mock_vault::MockVault;

/// 25% of the fee (1e7-scaled).
const QUARTER_OF_FEE: i128 = 2_500_000;
/// 50% of the fee — the constructor's cap.
const HALF_OF_FEE: i128 = 5_000_000;

/// The core accounting invariant: the treasury cut leaves the pool entirely,
/// so stored reserves still equal actual balances after every operation.
fn assert_reserves_match_balances(f: &AmmFixture) {
    let (reserve_pt, reserve_v) = f.pool.get_reserves();
    assert_eq!(reserve_pt, f.pt.balance(&f.pool.address), "PT reserve diverged from balance");
    assert_eq!(reserve_v, f.vault.balance(&f.pool.address), "V reserve diverged from balance");
}

fn register_pool_with_rate(env: &Env, rate: i128) {
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
        (&pt_addr, &vault_addr, expiry, CURRENT_APY, APY_MIN, APY_MAX, FEE_APY, &ym, &treasury, rate),
    );
}

#[test]
fn test_pool_stores_fee_config() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new_with_reserve_fee(&env, QUARTER_OF_FEE);

    assert_eq!(f.pool.get_treasury(), f.treasury);
    assert_eq!(f.pool.get_reserve_fee_rate(), QUARTER_OF_FEE);
}

#[test]
#[should_panic(expected = "reserve_fee_rate out of range")]
fn test_constructor_rejects_rate_above_cap() {
    let env = Env::default();
    register_pool_with_rate(&env, HALF_OF_FEE + 1);
}

#[test]
#[should_panic(expected = "reserve_fee_rate out of range")]
fn test_constructor_rejects_negative_rate() {
    let env = Env::default();
    register_pool_with_rate(&env, -1);
}

#[test]
fn test_zero_rate_swaps_pay_treasury_nothing() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env); // rate 0
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    f.swap_pt_for_v(&f.user, 1_000_000, 1);
    f.swap_v_for_pt(&f.user, 1_000_000, 2_000_000);

    assert_eq!(f.vault.balance(&f.treasury), 0, "zero-rate market must not pay the treasury");
    assert_reserves_match_balances(&f);
}

#[test]
fn test_swap_v_for_pt_remits_fee_to_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new_with_reserve_fee(&env, QUARTER_OF_FEE);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    f.swap_v_for_pt(&f.user, 1_000_000, 2_000_000);

    assert!(f.vault.balance(&f.treasury) > 0, "treasury received no fee");
    assert_eq!(f.pt.balance(&f.treasury), 0, "treasury must only receive V");
    assert_reserves_match_balances(&f);
}

#[test]
fn test_swap_pt_for_v_remits_fee_to_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new_with_reserve_fee(&env, QUARTER_OF_FEE);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    f.swap_pt_for_v(&f.user, 1_000_000, 1);

    assert!(f.vault.balance(&f.treasury) > 0, "treasury received no fee");
    assert_eq!(f.pt.balance(&f.treasury), 0, "treasury must only receive V");
    assert_reserves_match_balances(&f);
}

/// The remit is proportional to the rate: an identical trade at double the
/// rate pays the treasury exactly double, up to the final floor.
#[test]
fn test_fee_scales_with_rate() {
    let treasury_gain = |rate: i128| -> i128 {
        let env = Env::default();
        env.mock_all_auths();
        let f = AmmFixture::new_with_reserve_fee(&env, rate);
        f.deposit(&f.admin, 100_000_000, 100_000_000);
        f.swap_pt_for_v(&f.user, 1_000_000, 1);
        f.vault.balance(&f.treasury)
    };

    let quarter = treasury_gain(QUARTER_OF_FEE);
    let half = treasury_gain(HALF_OF_FEE);

    assert!(quarter > 0, "quarter-rate trade paid nothing");
    assert!(
        half >= 2 * quarter && half <= 2 * quarter + 1,
        "fee not proportional to rate: quarter={} half={}",
        quarter,
        half
    );
}

#[test]
fn test_flash_swap_pt_remits_fee_to_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new_with_reserve_fee(&env, QUARTER_OF_FEE);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // Well-behaved receiver at the trusted YM address, funded with PT so it
    // can deliver the bought PT back to the pool (stands in for YM minting).
    let receiver = env.register_at(
        &f.ym,
        MockFlashPtReceiver,
        (f.pool.address.clone(), f.pt.address.clone(), true),
    );
    f.pt.mint(&receiver, &1_000_000_000);

    f.pool.flash_swap_pt(&receiver, &1_000_000, &f.user, &2_000_000);

    assert!(f.vault.balance(&f.treasury) > 0, "flash PT swap paid no fee");
    assert_reserves_match_balances(&f);
}

#[test]
fn test_flash_swap_v_remits_fee_to_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new_with_reserve_fee(&env, QUARTER_OF_FEE);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // Well-behaved receiver (mode 0: repay exactly v_owed), funded with V.
    let receiver = env.register_at(
        &f.ym,
        MockFlashVReceiver,
        (f.pool.address.clone(), f.vault.address.clone(), f.pt.address.clone(), 0u32),
    );
    f.vault.mint(&receiver, &1_000_000_000);

    f.pool.flash_swap_v(&receiver, &1_000_000, &f.user, &1);

    assert!(f.vault.balance(&f.treasury) > 0, "flash V swap paid no fee");
    assert_reserves_match_balances(&f);
}
use soroban_sdk::{contract, contractimpl, token, Address, Env};
use soroban_sdk::testutils::Address as _;

use super::fixture::{AmmFixture, ONE_YEAR_SECS};
use amm_interface::{AmmClient, FlashSwapPtReceiver, FlashSwapVReceiver};

// ── Mock flash-V receiver ────────────────────────────────────────────────────
//
// Stands in for the router on the YT→V path. Behaviour `mode`:
//   0 — repay exactly `v_owed` in V; keep the borrowed PT (the correct behaviour)
//   1 — repay `v_owed - 1` in V (under-repay → AMM must revert)
//   2 — repay `v_owed` in V *and* hand the borrowed PT back (→ AMM must revert)

const POOL: &str = "pool";
const VTOK: &str = "vtok";
const PTOK: &str = "ptok";
const MODE: &str = "mode";

#[contract]
pub struct MockFlashVReceiver;

#[contractimpl]
impl MockFlashVReceiver {
    pub fn __constructor(e: Env, pool: Address, v_token: Address, pt_token: Address, mode: u32) {
        e.storage().instance().set(&POOL, &pool);
        e.storage().instance().set(&VTOK, &v_token);
        e.storage().instance().set(&PTOK, &pt_token);
        e.storage().instance().set(&MODE, &mode);
    }
}

#[contractimpl]
impl FlashSwapVReceiver for MockFlashVReceiver {
    fn on_flash_receive_v(e: Env, pt_borrowed: i128, v_owed: i128, _user: Address, _min_v_out: i128, amm: Address) {
        let v_token: Address = e.storage().instance().get(&VTOK).unwrap();
        let pt_token: Address = e.storage().instance().get(&PTOK).unwrap();
        let mode: u32 = e.storage().instance().get(&MODE).unwrap();
        let me = e.current_contract_address();

        let v_pay = if mode == 1 { v_owed - 1 } else { v_owed };
        token::Client::new(&e, &v_token).transfer(&me, &amm, &v_pay);

        if mode == 2 {
            token::Client::new(&e, &pt_token).transfer(&me, &amm, &pt_borrowed);
        }
    }
}

// ── Mock flash-PT receiver ───────────────────────────────────────────────────
//
// Stands in for the router on the V→YT path. `repay_ok = true` returns exactly
// the borrowed PT (valid: pool ends with ≥ what it had); `false` returns one less.

const PT_POOL: &str = "ptpool";
const PT_TOK: &str = "pttok";
const REPAY_OK: &str = "repay";

#[contract]
pub struct MockFlashPtReceiver;

#[contractimpl]
impl MockFlashPtReceiver {
    pub fn __constructor(e: Env, pool: Address, pt_token: Address, repay_ok: bool) {
        e.storage().instance().set(&PT_POOL, &pool);
        e.storage().instance().set(&PT_TOK, &pt_token);
        e.storage().instance().set(&REPAY_OK, &repay_ok);
    }
}

#[contractimpl]
impl FlashSwapPtReceiver for MockFlashPtReceiver {
    fn on_flash_receive_pt(e: Env, yt_out: i128, _v_from_pool: i128, _user: Address, _max_v_in: i128, amm: Address) {
        let pt_token: Address = e.storage().instance().get(&PT_TOK).unwrap();
        let repay_ok: bool = e.storage().instance().get(&REPAY_OK).unwrap();
        let me = e.current_contract_address();

        // Deliver the bought PT to the pool (repay_ok=false under-delivers by 1 → pool reverts).
        let amount = if repay_ok { yt_out } else { yt_out - 1 };
        token::Client::new(&e, &pt_token).transfer(&me, &amm, &amount);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn v_receiver(f: &AmmFixture, mode: u32) -> Address {
    // Deploy at the pool's trusted receiver address so `flash_swap_v` accepts it.
    let addr = f.env.register_at(
        &f.ym,
        MockFlashVReceiver,
        (f.pool.address.clone(), f.vault.address.clone(), f.pt.address.clone(), mode),
    );
    // Fund it with V so it can repay the pool.
    f.vault.mint(&addr, &1_000_000_000);
    addr
}

fn pt_receiver(f: &AmmFixture, repay_ok: bool) -> Address {
    // Deploy at the pool's trusted receiver address so `flash_swap_pt` accepts it.
    let addr = f.env.register_at(
        &f.ym,
        MockFlashPtReceiver,
        (f.pool.address.clone(), f.pt.address.clone(), repay_ok),
    );
    // Fund it with PT so it can deliver the bought PT to the pool (stands in for the YM
    // minting it — the pool only observes the PT arriving).
    f.pt.mint(&addr, &1_000_000_000);
    addr
}

// ── flash_swap_v ─────────────────────────────────────────────────────────────

#[test]
fn test_flash_swap_v_happy_path() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = v_receiver(&f, 0);
    let (pt_res_before, v_res_before) = f.pool.get_reserves();
    let recv_pt_before = f.pt.balance(&receiver);

    let pt_to_borrow = 1_000_000i128;
    f.pool.flash_swap_v(&receiver, &pt_to_borrow, &f.user, &1i128);

    let (pt_res_after, v_res_after) = f.pool.get_reserves();
    assert_eq!(pt_res_after, pt_res_before - pt_to_borrow, "PT reserve drops by the lent amount");
    assert!(v_res_after > v_res_before, "V reserve grew by the repayment");
    // Curve price of PT is below par, so the pool is repaid less V than the PT's face amount.
    assert!(v_res_after - v_res_before < pt_to_borrow, "repayment should be below face value");
    // The borrowed PT stays with the receiver (the router would burn it via the YM redeem).
    assert_eq!(f.pt.balance(&receiver), recv_pt_before + pt_to_borrow);
}

#[test]
#[should_panic]
fn test_flash_swap_v_under_repay_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = v_receiver(&f, 1);
    f.pool.flash_swap_v(&receiver, &1_000_000i128, &f.user, &1i128);
}

#[test]
#[should_panic]
fn test_flash_swap_v_returning_pt_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = v_receiver(&f, 2);
    f.pool.flash_swap_v(&receiver, &1_000_000i128, &f.user, &1i128);
}

#[test]
#[should_panic]
fn test_flash_swap_v_zero_borrow_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = v_receiver(&f, 0);
    f.pool.flash_swap_v(&receiver, &0i128, &f.user, &1i128);
}

#[test]
#[should_panic]
fn test_flash_swap_v_insufficient_liquidity_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = v_receiver(&f, 0);
    // Asking to borrow more PT than the pool holds.
    f.pool.flash_swap_v(&receiver, &200_000_000i128, &f.user, &1i128);
}

#[test]
#[should_panic]
fn test_flash_swap_v_expired_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = v_receiver(&f, 0);
    f.set_time(env.ledger().timestamp() + ONE_YEAR_SECS + 1);
    f.pool.flash_swap_v(&receiver, &1_000_000i128, &f.user, &1i128);
}

// ── flash_swap_pt ────────────────────────────────────────────────────────────

#[test]
fn test_flash_swap_pt_buys_pt_and_pays_v() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = pt_receiver(&f, true);
    let (pt_res_before, v_res_before) = f.pool.get_reserves();

    let yt_out = 1_000_000i128;
    f.pool.flash_swap_pt(&receiver, &yt_out, &f.user, &1_000_000_000i128);

    let (pt_res_after, v_res_after) = f.pool.get_reserves();
    // The pool bought exactly yt_out PT and paid V for it.
    assert_eq!(pt_res_after, pt_res_before + yt_out, "pool PT reserve grows by the bought PT");
    assert!(v_res_after < v_res_before, "pool paid V for the PT");
    // PT trades below par, so the V paid is below the face amount.
    assert!(v_res_before - v_res_after < yt_out, "V paid is below face value");
}

#[test]
#[should_panic]
fn test_flash_swap_pt_under_deliver_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // Receiver delivers one less PT than the pool bought → the exact-PT invariant reverts.
    let receiver = pt_receiver(&f, false);
    f.pool.flash_swap_pt(&receiver, &1_000_000i128, &f.user, &1_000_000_000i128);
}

#[test]
#[should_panic(expected = "trade pushes pool proportion out of bounds")]
fn test_flash_swap_pt_oversized_trade_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // Buying 200M YT pushes 200M PT into the pool: post-trade proportion
    // 300M / 200M exceeds the 96% cap, so the curve rejects the trade before
    // any V leaves the pool. The V-liquidity assert in flash_swap_pt cannot
    // fire first: exchange_rate >= 1 means v_paid <= yt_out, so draining the
    // V reserve would require proportion > 1.
    let receiver = pt_receiver(&f, true);
    f.pool.flash_swap_pt(&receiver, &200_000_000i128, &f.user, &1_000_000_000i128);
}

#[test]
#[should_panic(expected = "expected pool to pay V for PT")]
fn test_flash_swap_pt_dust_amount_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // yt_out = 1 stroop: the curve price truncates to zero V. The pool must refuse
    // the trade rather than advance nothing and buy PT for free.
    let receiver = pt_receiver(&f, true);
    f.pool.flash_swap_pt(&receiver, &1i128, &f.user, &1_000_000_000i128);
}

// ── Security regressions ─────────────────────────────────────────────────────
//
// A receiver that tries to re-enter the pool during its flash callback. Soroban
// forbids re-entering a contract already on the call stack at the host level, so
// the nested call fails and the whole flash swap reverts. This test pins that
// property: the classic flash-callback reentrancy drain is not reachable here.

#[contract]
pub struct MockReentrantReceiver;

#[contractimpl]
impl FlashSwapVReceiver for MockReentrantReceiver {
    fn on_flash_receive_v(e: Env, _pt_borrowed: i128, _v_owed: i128, _user: Address, _min_v_out: i128, amm: Address) {
        // Attempt to re-enter the pool mid-flash; the host rejects this.
        let me = e.current_contract_address();
        AmmClient::new(&e, &amm).swap_pt_for_v(&me, &1i128, &1i128);
    }
}

#[test]
#[should_panic]
fn test_flash_swap_v_reentrancy_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // Deploy the reentrant mock at the pool's trusted receiver address so it passes
    // the allowlist check and actually reaches the callback — where re-entry is refused.
    f.env.register_at(&f.ym, MockReentrantReceiver, ());
    f.pool.flash_swap_v(&f.ym, &1_000_000i128, &f.user, &1i128);
}

#[contract]
pub struct MockReentrantPtReceiver;

#[contractimpl]
impl FlashSwapPtReceiver for MockReentrantPtReceiver {
    fn on_flash_receive_pt(e: Env, _yt_out: i128, _v_from_pool: i128, _user: Address, _max_v_in: i128, amm: Address) {
        // Attempt to re-enter the pool mid-flash; the host rejects this.
        let me = e.current_contract_address();
        AmmClient::new(&e, &amm).swap_v_for_pt(&me, &1i128, &1i128);
    }
}

#[test]
#[should_panic]
fn test_flash_swap_pt_reentrancy_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // Same pin as the V side: a trusted receiver re-entering during the PT flash
    // callback is refused by the host, reverting the whole swap.
    f.env.register_at(&f.ym, MockReentrantPtReceiver, ());
    f.pool.flash_swap_pt(&f.ym, &1_000_000i128, &f.user, &1_000_000_000i128);
}

#[test]
#[should_panic(expected = "trusted yield manager")]
fn test_flash_swap_v_untrusted_receiver_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    // Any receiver other than the pool's configured ym must be rejected before the callback.
    let evil = Address::generate(&env);
    f.pool.flash_swap_v(&evil, &1_000_000i128, &f.user, &1i128);
}

#[test]
#[should_panic(expected = "trusted yield manager")]
fn test_flash_swap_pt_untrusted_receiver_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let evil = Address::generate(&env);
    f.pool.flash_swap_pt(&evil, &1_000_000i128, &f.user, &1_000_000_000i128);
}
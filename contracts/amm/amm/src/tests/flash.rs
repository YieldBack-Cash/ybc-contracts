use soroban_sdk::{contract, contractimpl, token, Address, Env};

use super::fixture::{AmmFixture, ONE_YEAR_SECS};
use amm_interface::{FlashSwapReceiver, FlashSwapVReceiver};

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
impl FlashSwapReceiver for MockFlashPtReceiver {
    fn on_flash_receive(e: Env, pt_borrowed: i128, _user: Address, _v_in: i128, _min_yt_out: i128, amm: Address) {
        let pt_token: Address = e.storage().instance().get(&PT_TOK).unwrap();
        let repay_ok: bool = e.storage().instance().get(&REPAY_OK).unwrap();
        let me = e.current_contract_address();

        let amount = if repay_ok { pt_borrowed } else { pt_borrowed - 1 };
        token::Client::new(&e, &pt_token).transfer(&me, &amm, &amount);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn v_receiver(f: &AmmFixture, mode: u32) -> Address {
    let addr = f.env.register(
        MockFlashVReceiver,
        (f.pool.address.clone(), f.vault.address.clone(), f.pt.address.clone(), mode),
    );
    // Fund it with V so it can repay the pool.
    f.vault.mint(&addr, &1_000_000_000);
    addr
}

fn pt_receiver(f: &AmmFixture, repay_ok: bool) -> Address {
    f.env.register(
        MockFlashPtReceiver,
        (f.pool.address.clone(), f.pt.address.clone(), repay_ok),
    )
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
fn test_flash_swap_pt_repaid_keeps_pt_reserve() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = pt_receiver(&f, true);
    let (pt_res_before, _) = f.pool.get_reserves();

    f.pool.flash_swap_pt(&receiver, &1_000_000i128, &f.user, &1i128, &1i128);

    let (pt_res_after, _) = f.pool.get_reserves();
    // Mock repays exactly what it borrowed (no minted PT), so the reserve is unchanged.
    assert_eq!(pt_res_after, pt_res_before);
}

#[test]
#[should_panic]
fn test_flash_swap_pt_under_repay_reverts() {
    let env = Env::default();
    env.mock_all_auths();
    let f = AmmFixture::new(&env);
    f.deposit(&f.admin, 100_000_000, 100_000_000);

    let receiver = pt_receiver(&f, false);
    f.pool.flash_swap_pt(&receiver, &1_000_000i128, &f.user, &1i128, &1i128);
}
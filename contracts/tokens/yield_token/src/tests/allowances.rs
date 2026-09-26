// ── Allowance-driven moves and the accrual hook ──────────────────────────────
//
// `transfer_from` and `burn_from` arrived with the OpenZeppelin ledger. They
// must settle yield exactly as a plain `transfer` and `burn` do: both parties
// accrue at the current rate before the balance moves, a receiver's index
// catches up to the rate at acquisition, and a self-move mints nothing.

use super::YieldTokenTest;
use soroban_sdk::{Address, IntoVal, Symbol};

const EXPIRY: u32 = 500_000;

fn approve(t: &YieldTokenTest, owner: &Address, spender: &Address, amount: i128) {
    t.env.invoke_contract::<()>(
        &t.yield_token,
        &Symbol::new(&t.env, "approve"),
        (owner, spender, amount, EXPIRY).into_val(&t.env),
    );
}

fn allowance(t: &YieldTokenTest, owner: &Address, spender: &Address) -> i128 {
    t.env.invoke_contract::<i128>(
        &t.yield_token,
        &Symbol::new(&t.env, "allowance"),
        (owner, spender).into_val(&t.env),
    )
}

fn transfer_from(t: &YieldTokenTest, spender: &Address, from: &Address, to: &Address, amount: i128) {
    t.env.invoke_contract::<()>(
        &t.yield_token,
        &Symbol::new(&t.env, "transfer_from"),
        (spender, from, to, amount).into_val(&t.env),
    );
}

fn burn_from(t: &YieldTokenTest, spender: &Address, from: &Address, amount: i128) {
    t.env.invoke_contract::<()>(
        &t.yield_token,
        &Symbol::new(&t.env, "burn_from"),
        (spender, from, amount).into_val(&t.env),
    );
}

const MINT: i128 = 2_000_000_000_000;

/// Mirror of `test_transfer_accrues_yield_for_both_parties`, through an allowance.
#[test]
fn transfer_from_accrues_yield_for_both_parties() {
    let t = YieldTokenTest::setup();
    let initial_rate = t.get_exchange_rate();
    t.mint_yt(&t.user1, MINT, initial_rate);
    approve(&t, &t.user1, &t.user2, MINT);

    let new_rate = initial_rate + 100_0000;
    t.set_vault_exchange_rate(new_rate);

    transfer_from(&t, &t.user2, &t.user1, &t.user2, MINT / 2);

    // the sender settled the growth on the whole balance before half left
    assert!(t.get_accrued_yield(&t.user1) > 0);
    assert_eq!(t.get_user_index(&t.user1), new_rate);
    // the receiver starts at today's rate and owns none of the growth before it
    assert_eq!(t.get_user_index(&t.user2), new_rate);
    assert_eq!(t.get_accrued_yield(&t.user2), 0);
    assert_eq!(t.get_balance(&t.user1), MINT / 2);
    assert_eq!(t.get_balance(&t.user2), MINT / 2);
    assert_eq!(allowance(&t, &t.user1, &t.user2), MINT / 2);
}

/// The sender's accrued yield equals what a plain transfer of the same amount at
/// the same rate would have settled: the path does not change the accounting.
#[test]
fn transfer_from_settles_the_same_yield_as_transfer() {
    let via_transfer = {
        let t = YieldTokenTest::setup();
        let r0 = t.get_exchange_rate();
        t.mint_yt(&t.user1, MINT, r0);
        t.set_vault_exchange_rate(r0 + 100_0000);
        t.transfer(&t.user1, &t.user2, MINT / 4);
        t.get_accrued_yield(&t.user1)
    };
    let via_allowance = {
        let t = YieldTokenTest::setup();
        let r0 = t.get_exchange_rate();
        t.mint_yt(&t.user1, MINT, r0);
        approve(&t, &t.user1, &t.user2, MINT);
        t.set_vault_exchange_rate(r0 + 100_0000);
        transfer_from(&t, &t.user2, &t.user1, &t.user2, MINT / 4);
        t.get_accrued_yield(&t.user1)
    };
    assert!(via_transfer > 0);
    assert_eq!(via_transfer, via_allowance);
}

/// The stale-index hazard: a receiver that once held YT, emptied out, and sat
/// through a rate rise must have its index moved up at acquisition, not carry
/// the old one into the new balance and later claim growth it never owned.
#[test]
fn transfer_from_to_an_emptied_holder_does_not_grant_past_growth() {
    let t = YieldTokenTest::setup();
    let r0 = t.get_exchange_rate();
    t.mint_yt(&t.user2, MINT, r0);
    t.transfer(&t.user2, &t.user1, MINT); // user2 now empty, index parked at r0
    assert_eq!(t.get_balance(&t.user2), 0);
    assert_eq!(t.get_user_index(&t.user2), r0);

    let r1 = r0 + 200_0000;
    t.set_vault_exchange_rate(r1);
    approve(&t, &t.user1, &t.user2, MINT);
    transfer_from(&t, &t.user2, &t.user1, &t.user2, MINT / 2);

    assert_eq!(t.get_user_index(&t.user2), r1);
    assert_eq!(t.get_accrued_yield(&t.user2), 0, "no yield for growth before ownership");
    // and nothing accrues from here until the rate moves again
    assert_eq!(t.claim_yield(&t.user2), 0);
}

/// A self-move through an allowance neither mints YT nor credits yield twice.
#[test]
fn self_transfer_from_is_a_no_op_on_balance_and_yield() {
    let t = YieldTokenTest::setup();
    let r0 = t.get_exchange_rate();
    t.mint_yt(&t.user1, MINT, r0);
    approve(&t, &t.user1, &t.user1, MINT);
    t.set_vault_exchange_rate(r0 + 100_0000);

    // what one settlement at the new rate is worth
    let expected = {
        let u = YieldTokenTest::setup();
        let r = u.get_exchange_rate();
        u.mint_yt(&u.user1, MINT, r);
        u.set_vault_exchange_rate(r + 100_0000);
        u.transfer(&u.user1, &u.user2, 1);
        u.get_accrued_yield(&u.user1)
    };

    transfer_from(&t, &t.user1, &t.user1, &t.user1, 14);

    assert_eq!(t.get_balance(&t.user1), MINT);
    assert_eq!(t.get_total_supply(), MINT);
    assert_eq!(t.get_accrued_yield(&t.user1), expected, "accrued once, not twice");
}

/// `burn_from` settles the holder's yield first, like `burn`, then retires
/// exactly the amount within the allowance; the yield stays claimable.
#[test]
fn burn_from_accrues_before_burning_and_keeps_the_yield_claimable() {
    let t = YieldTokenTest::setup();
    let r0 = t.get_exchange_rate();
    t.mint_yt(&t.user1, MINT, r0);
    approve(&t, &t.user1, &t.user2, MINT / 2);
    t.set_vault_exchange_rate(r0 + 100_0000);

    burn_from(&t, &t.user2, &t.user1, MINT / 2);

    assert!(t.get_accrued_yield(&t.user1) > 0);
    assert_eq!(t.get_balance(&t.user1), MINT / 2);
    assert_eq!(t.get_total_supply(), MINT / 2);
    assert_eq!(allowance(&t, &t.user1, &t.user2), 0);
    assert!(t.claim_yield(&t.user1) > 0, "the settled yield is still the holder's");
}

/// The allowance is the limit: a spender cannot move more than approved.
#[test]
#[should_panic(expected = "Error(Contract, #101)")]
fn transfer_from_beyond_the_allowance_is_refused() {
    let t = YieldTokenTest::setup();
    let r0 = t.get_exchange_rate();
    t.mint_yt(&t.user1, MINT, r0);
    approve(&t, &t.user1, &t.user2, MINT / 4);

    transfer_from(&t, &t.user2, &t.user1, &t.user2, MINT / 2);
}

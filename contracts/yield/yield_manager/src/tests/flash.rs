use soroban_sdk::{testutils::Address as _, token::TokenClient, Address, IntoVal, Symbol};

use super::fixture::YieldManagerTest;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Call on_flash_receive_v directly on the YM contract.
fn invoke_flash_receive_v(
    test: &YieldManagerTest,
    pt_borrowed: i128,
    v_owed: i128,
    user: &Address,
    min_v_out: i128,
    amm: &Address,
) {
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "on_flash_receive_v"),
        (pt_borrowed, v_owed, user, min_v_out, amm).into_val(&test.env),
    );
}

/// Deposit `deposit` V from user1 so the YM holds V backing.
/// Then simulate the AMM lending `pt_borrowed` PT to the YM (transfer from user1).
/// Give `pt_borrowed` YT to a fresh user2 (the YT seller).
/// Returns user2.
fn setup_flash(test: &YieldManagerTest, deposit: i128, pt_borrowed: i128) -> Address {
    let user2 = Address::generate(&test.env);
    test.mint_vault_shares(&test.user1, deposit);
    test.deposit(&test.user1, deposit);
    // Transfer PT from user1 → YM (mirrors what the AMM does before the callback).
    TokenClient::new(&test.env, &test.pt).transfer(&test.user1, &test.yield_manager, &pt_borrowed);
    // Give user2 the YT they want to sell.
    TokenClient::new(&test.env, &test.yt).transfer(&test.user1, &user2, &pt_borrowed);
    user2
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn test_on_flash_receive_v_happy_path() {
    let test = YieldManagerTest::setup();

    let deposit = 2_000_000i128;
    let pt_borrowed = 1_000_000i128;
    let user2 = setup_flash(&test, deposit, pt_borrowed);
    let amm = Address::generate(&test.env);

    // exchange_rate = 10_000_000 (1:1) → shares_returned = pt_borrowed = 1_000_000
    let v_owed = 900_000i128;
    let expected_v_to_user = 100_000i128;

    let pt = TokenClient::new(&test.env, &test.pt);
    let yt = TokenClient::new(&test.env, &test.yt);
    let vault = TokenClient::new(&test.env, &test.vault_addr);

    invoke_flash_receive_v(&test, pt_borrowed, v_owed, &user2, 0, &amm);

    // user2's YT was fully consumed.
    assert_eq!(yt.balance(&user2), 0, "user2 YT consumed");
    // AMM received exactly v_owed.
    assert_eq!(vault.balance(&amm), v_owed, "amm received v_owed");
    // user2 received the remainder.
    assert_eq!(vault.balance(&user2), expected_v_to_user, "user2 received remainder");
    // PT and YT were burned from the YM — it holds none.
    assert_eq!(pt.balance(&test.yield_manager), 0, "YM PT burned");
    assert_eq!(yt.balance(&test.yield_manager), 0, "YM YT burned");
    // Conservation: total V out = shares_returned = pt_borrowed (at 1:1 rate).
    assert_eq!(vault.balance(&amm) + vault.balance(&user2), pt_borrowed, "V conserved");
}

#[test]
#[should_panic(expected = "v out below minimum")]
fn test_on_flash_receive_v_min_v_out_reverts() {
    let test = YieldManagerTest::setup();

    let deposit = 2_000_000i128;
    let pt_borrowed = 1_000_000i128;
    let user2 = setup_flash(&test, deposit, pt_borrowed);
    let amm = Address::generate(&test.env);

    // shares_returned = 1_000_000, v_owed = 900_000 → v_to_user = 100_000
    // min_v_out = 200_000 > 100_000 → must revert
    invoke_flash_receive_v(&test, pt_borrowed, 900_000, &user2, 200_000, &amm);
}

#[test]
#[should_panic(expected = "redeem yielded less V than owed to pool")]
fn test_on_flash_receive_v_owed_exceeds_redeemed_panics() {
    let test = YieldManagerTest::setup();

    let deposit = 2_000_000i128;
    let pt_borrowed = 1_000_000i128;
    let user2 = setup_flash(&test, deposit, pt_borrowed);
    let amm = Address::generate(&test.env);

    // v_owed = 1_100_000 > shares_returned = 1_000_000 → must revert
    invoke_flash_receive_v(&test, pt_borrowed, 1_100_000, &user2, 0, &amm);
}
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
    // Give user2 the YT they want to sell, then move it into the YM — mirroring the
    // router, which pulls the seller's YT before initiating the flash swap.
    TokenClient::new(&test.env, &test.yt).transfer(&test.user1, &user2, &pt_borrowed);
    TokenClient::new(&test.env, &test.yt).transfer(&user2, &test.yield_manager, &pt_borrowed);
    user2
}

/// Call on_flash_receive_pt directly on the YM contract.
fn invoke_flash_receive_pt(
    test: &YieldManagerTest,
    yt_out: i128,
    v_from_pool: i128,
    user: &Address,
    max_v_in: i128,
    amm: &Address,
) {
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "on_flash_receive_pt"),
        (yt_out, v_from_pool, user, max_v_in, amm).into_val(&test.env),
    );
}

/// Mirror the AMM's flash advance: mint `v_from_pool` V to the YM (the pool's payment
/// for the PT it is buying) and fund user2 (the YT buyer) with `user_v` V for the top-up.
fn setup_flash_buy(test: &YieldManagerTest, v_from_pool: i128, user_v: i128) {
    test.mint_vault_shares(&test.yield_manager, v_from_pool);
    test.mint_vault_shares(&test.user2, user_v);
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn test_on_flash_receive_v_happy_path() {
    let test = YieldManagerTest::setup();

    let deposit = 2_000_000i128;
    let pt_borrowed = 1_000_000i128;
    let user2 = setup_flash(&test, deposit, pt_borrowed);
    let amm = test.pool.clone();

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
    let amm = test.pool.clone();

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
    let amm = test.pool.clone();

    // v_owed = 1_100_000 > shares_returned = 1_000_000 → must revert
    invoke_flash_receive_v(&test, pt_borrowed, 1_100_000, &user2, 0, &amm);
}

// ── on_flash_receive_pt (buy YT: pool advances V, YM mints, user tops up) ─────

#[test]
fn test_on_flash_receive_pt_happy_path() {
    let test = YieldManagerTest::setup();
    let amm = test.pool.clone();

    // exchange_rate = 10_000_000 (1:1) → v_to_mint = yt_out = 1_000_000
    let yt_out = 1_000_000i128;
    let v_from_pool = 900_000i128;
    let expected_user_cost = 100_000i128;
    let user_v = 500_000i128;
    setup_flash_buy(&test, v_from_pool, user_v);

    invoke_flash_receive_pt(&test, yt_out, v_from_pool, &test.user2, 200_000, &amm);

    // The user paid exactly the YT price and received exactly yt_out YT.
    assert_eq!(
        test.vault_balance(&test.user2),
        user_v - expected_user_cost,
        "user pays exactly v_to_mint - v_from_pool"
    );
    assert_eq!(test.get_yt_balance(&test.user2), yt_out, "user receives yt_out YT");
    // The pool received its yt_out PT; the YM kept none.
    assert_eq!(test.get_pt_balance(&amm), yt_out, "amm received yt_out PT");
    assert_eq!(test.get_pt_balance(&test.yield_manager), 0, "YM leaked PT");
    // Backing: pool advance + user top-up = v_to_mint, all held by the YM.
    assert_eq!(
        test.vault_balance(&test.yield_manager),
        yt_out,
        "YM holds the full mint cost in V backing the new PT+YT"
    );
}

#[test]
#[should_panic(expected = "cost exceeds max_v_in")]
fn test_on_flash_receive_pt_cost_exceeds_max_reverts() {
    let test = YieldManagerTest::setup();
    let amm = test.pool.clone();

    setup_flash_buy(&test, 900_000, 500_000);

    // user_cost = 100_000 but max_v_in = 99_999 → must revert
    invoke_flash_receive_pt(&test, 1_000_000, 900_000, &test.user2, 99_999, &amm);
}

#[test]
#[should_panic(expected = "non-positive YT cost")]
fn test_on_flash_receive_pt_pool_overpay_reverts() {
    let test = YieldManagerTest::setup();
    let amm = test.pool.clone();

    setup_flash_buy(&test, 1_000_000, 500_000);

    // v_from_pool = v_to_mint → user_cost = 0; a mispriced advance must not mint free YT.
    invoke_flash_receive_pt(&test, 1_000_000, 1_000_000, &test.user2, 200_000, &amm);
}

#[test]
#[should_panic]
fn test_on_flash_receive_pt_insufficient_user_v_reverts() {
    let test = YieldManagerTest::setup();
    let amm = test.pool.clone();

    // The callback pulls the full max_v_in (200_000) and refunds the excess, so the
    // user must hold max_v_in for the duration of the call — 150_000 covers the
    // 100_000 cost but not the pull, and the transfer fails.
    setup_flash_buy(&test, 900_000, 150_000);

    invoke_flash_receive_pt(&test, 1_000_000, 900_000, &test.user2, 200_000, &amm);
}

#[test]
fn test_on_flash_receive_pt_higher_rate() {
    let test = YieldManagerTest::setup();
    let amm = test.pool.clone();

    // rate 2.0 → v_to_mint = yt_out / 2 = 500_000
    test.set_vault_exchange_rate(20_000_000);

    let yt_out = 1_000_000i128;
    let v_from_pool = 400_000i128;
    let expected_user_cost = 100_000i128;
    let user_v = 500_000i128;
    setup_flash_buy(&test, v_from_pool, user_v);

    invoke_flash_receive_pt(&test, yt_out, v_from_pool, &test.user2, 200_000, &amm);

    assert_eq!(
        test.vault_balance(&test.user2),
        user_v - expected_user_cost,
        "user cost reflects the halved share requirement at rate 2.0"
    );
    assert_eq!(test.get_yt_balance(&test.user2), yt_out, "user receives yt_out YT");
    assert_eq!(test.get_pt_balance(&amm), yt_out, "amm received yt_out PT");
    assert_eq!(
        test.vault_balance(&test.yield_manager),
        500_000,
        "YM holds v_to_mint = yt_out / rate in V backing"
    );
}

#[test]
#[should_panic(expected = "non-positive YT cost")]
fn test_on_flash_receive_pt_dust_reverts() {
    let test = YieldManagerTest::setup();
    let amm = test.pool.clone();

    // At rate 2.0, yt_out = 1 truncates v_to_mint to 0, so user_cost = -v_from_pool < 0.
    // Dust buys must revert rather than mint PT+YT with no V backing.
    test.set_vault_exchange_rate(20_000_000);
    setup_flash_buy(&test, 1, 100);

    invoke_flash_receive_pt(&test, 1, 1, &test.user2, 100, &amm);
}
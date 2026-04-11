#![cfg(test)]

use crate::LiquidityPool;
use soroban_sdk::{
    testutils::Address as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

struct LiquidityPoolTest<'a> {
    env: Env,
    token_a: TokenClient<'a>,
    token_b: TokenClient<'a>,
    pool: crate::contract::LiquidityPoolClient<'a>,
    user: Address,
}

impl<'a> LiquidityPoolTest<'a> {
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        // Create token contracts using Soroban's test token
        let token_a_address = env.register_stellar_asset_contract_v2(admin.clone());
        let token_b_address = env.register_stellar_asset_contract_v2(admin.clone());

        let token_a = TokenClient::new(&env, &token_a_address.address());
        let token_b = TokenClient::new(&env, &token_b_address.address());

        // Ensure token_a address < token_b address for initialization
        let (token_a_final, token_b_final) = if token_a.address < token_b.address {
            (token_a, token_b)
        } else {
            (token_b, token_a)
        };

        // Deploy and initialize AMM with constructor arguments
        let pool_contract_id = env.register(
            LiquidityPool,
            (&token_a_final.address, &token_b_final.address),
        );
        let pool = crate::contract::LiquidityPoolClient::new(&env, &pool_contract_id);

        LiquidityPoolTest {
            env,
            token_a: token_a_final,
            token_b: token_b_final,
            pool,
            user,
        }
    }

    fn mint_tokens(&self, to: &Address, amount: i128) {
        // Use the admin client to mint
        let token_a_admin = StellarAssetClient::new(&self.env, &self.token_a.address);
        let token_b_admin = StellarAssetClient::new(&self.env, &self.token_b.address);

        token_a_admin.mint(to, &amount);
        token_b_admin.mint(to, &amount);
    }
}

#[test]
fn test_initialization() {
    let test = LiquidityPoolTest::setup();

    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    assert_eq!(reserve_a, 0);
    assert_eq!(reserve_b, 0);
}

#[test]
#[should_panic(expected = "token_a must be less than token_b")]
fn test_initialization_wrong_order() {
    let env = Env::default();
    let admin = Address::generate(&env);

    let token_a_address = env.register_stellar_asset_contract_v2(admin.clone());
    let token_b_address = env.register_stellar_asset_contract_v2(admin.clone());

    // Initialize with token addresses in wrong order
    if token_a_address.address() > token_b_address.address() {
        let _ = env.register(
            LiquidityPool,
            (&token_a_address.address(), &token_b_address.address()),
        );
    } else {
        let _ = env.register(
            LiquidityPool,
            (&token_b_address.address(), &token_a_address.address()),
        );
    }
}

#[test]
fn test_first_deposit() {
    let test = LiquidityPoolTest::setup();

    // Mint tokens to user
    test.mint_tokens(&test.user, 1000);

    // First deposit - any ratio is accepted
    test.pool.deposit(&test.user, &1000, &1000, &1000, &1000);

    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    assert_eq!(reserve_a, 1000);
    assert_eq!(reserve_b, 1000);

    // User gets sqrt(1000 * 1000) - MINIMUM_LIQUIDITY (100) = 900 shares
    let shares = test.pool.balance_shares(&test.user);
    assert_eq!(shares, 900);
}

#[test]
fn test_deposit_maintains_ratio() {
    let test = LiquidityPoolTest::setup();

    // First deposit: user gets 1000 - 100 = 900 shares, total = 1000
    test.mint_tokens(&test.user, 2000);
    test.pool.deposit(&test.user, &1000, &1000, &1000, &1000);

    // Second deposit with same ratio
    let user2 = Address::generate(&test.env);
    test.mint_tokens(&user2, 500);
    test.pool.deposit(&user2, &500, &500, &500, &500);

    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    assert_eq!(reserve_a, 1500);
    assert_eq!(reserve_b, 1500);

    let shares1 = test.pool.balance_shares(&test.user);
    let shares2 = test.pool.balance_shares(&user2);
    assert_eq!(shares1, 900);
    assert_eq!(shares2, 500);
}

#[test]
fn test_deposit_adjusts_to_pool_ratio() {
    let test = LiquidityPoolTest::setup();

    // First deposit: 1000:2000 ratio
    test.mint_tokens(&test.user, 3000);
    test.pool.deposit(&test.user, &1000, &1000, &2000, &2000);

    // Second deposit: ask for 1000:1000 but it should adjust
    let user2 = Address::generate(&test.env);
    test.mint_tokens(&user2, 2000);

    // Desired is 1000:1000, but pool ratio is 1:2, so it will deposit 1000:2000
    test.pool.deposit(&user2, &1000, &500, &2000, &1000);

    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    // Should maintain 1:2 ratio
    assert_eq!(reserve_a, 2000);
    assert_eq!(reserve_b, 4000);
}

#[test]
#[should_panic(expected = "amount_b less than min")]
fn test_deposit_fails_below_minimum() {
    let test = LiquidityPoolTest::setup();

    // First deposit with 1000:1000 ratio (1:1)
    test.mint_tokens(&test.user, 1000);
    test.pool.deposit(&test.user, &1000, &1000, &1000, &1000);

    // Second user tries to deposit
    let user2 = Address::generate(&test.env);
    test.mint_tokens(&user2, 10_000);

    // Pool ratio is 1:1
    // If we want to deposit 1000 A, we need 1000 B (since ratio is 1:1)
    // But we set min_b to 1500, which can't be satisfied
    // The contract will calculate amount_b = 1000 * 1000 / 1000 = 1000
    // Since 1000 < 1500 (min_b), it should panic with "amount_b less than min"
    test.pool.deposit(&user2, &1000, &900, &10_000, &1500);
}

#[test]
#[should_panic(expected = "both amounts must be strictly positive")]
fn test_deposit_fails_with_zero_amount() {
    let test = LiquidityPoolTest::setup();

    test.mint_tokens(&test.user, 1000);
    test.pool.deposit(&test.user, &0, &0, &1000, &1000);
}

#[test]
fn test_swap_pt_for_v() {
    let test = LiquidityPoolTest::setup();

    // Setup pool with liquidity
    test.mint_tokens(&test.user, 100_000);
    test.pool.deposit(&test.user, &100_000, &100_000, &100_000, &100_000);

    // Swapper sells PT (token_a), buys V (token_b)
    let swapper = Address::generate(&test.env);
    let token_a_admin = StellarAssetClient::new(&test.env, &test.token_a.address);
    token_a_admin.mint(&swapper, &10_000);

    let desired_out = 9_000;
    test.pool.swap_pt_for_v(&swapper, &desired_out, &i128::MAX);

    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    assert!(reserve_a > 100_000); // PT reserve increased
    assert!(reserve_b < 100_000); // V reserve decreased
    assert_eq!(reserve_b, 100_000 - desired_out);
}

#[test]
fn test_swap_v_for_pt() {
    let test = LiquidityPoolTest::setup();

    // Setup pool with liquidity
    test.mint_tokens(&test.user, 100_000);
    test.pool.deposit(&test.user, &100_000, &100_000, &100_000, &100_000);

    // Swapper sells V (token_b), buys PT (token_a)
    let swapper = Address::generate(&test.env);
    let token_b_admin = StellarAssetClient::new(&test.env, &test.token_b.address);
    token_b_admin.mint(&swapper, &10_000);

    let desired_out = 9_000;
    test.pool.swap_v_for_pt(&swapper, &desired_out, &i128::MAX);

    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    assert!(reserve_a < 100_000); // PT reserve decreased
    assert!(reserve_b > 100_000); // V reserve increased
    assert_eq!(reserve_a, 100_000 - desired_out);
}

#[test]
#[should_panic(expected = "not enough liquidity")]
fn test_swap_fails_insufficient_liquidity() {
    let test = LiquidityPoolTest::setup();

    // Setup pool with limited liquidity
    test.mint_tokens(&test.user, 1_000);
    test.pool.deposit(&test.user, &1_000, &1_000, &1_000, &1_000);

    // Try to buy more than available
    let swapper = Address::generate(&test.env);
    let token_a_admin = StellarAssetClient::new(&test.env, &test.token_a.address);
    token_a_admin.mint(&swapper, &10_000);

    test.pool.swap_pt_for_v(&swapper, &1_001, &i128::MAX);
}

#[test]
#[should_panic(expected = "in amount is over max")]
fn test_swap_fails_slippage_protection() {
    let test = LiquidityPoolTest::setup();

    // Setup pool
    test.mint_tokens(&test.user, 100_000);
    test.pool.deposit(&test.user, &100_000, &100_000, &100_000, &100_000);

    // Try to swap with very restrictive slippage
    let swapper = Address::generate(&test.env);
    let token_a_admin = StellarAssetClient::new(&test.env, &test.token_a.address);
    token_a_admin.mint(&swapper, &10_000);

    // Want 9,000 V but only willing to pay max 100 PT (way too low)
    test.pool.swap_pt_for_v(&swapper, &9_000, &100);
}

#[test]
fn test_swap_respects_fee() {
    let test = LiquidityPoolTest::setup();

    // Setup pool with 100,000:100,000 liquidity
    test.mint_tokens(&test.user, 100_000);
    test.pool.deposit(&test.user, &100_000, &100_000, &100_000, &100_000);

    let (initial_a, initial_b) = test.pool.get_rsrvs();
    let k_before = initial_a * initial_b;

    // Execute a swap
    let swapper = Address::generate(&test.env);
    let token_a_admin = StellarAssetClient::new(&test.env, &test.token_a.address);
    token_a_admin.mint(&swapper, &10_000);
    test.pool.swap_pt_for_v(&swapper, &9_000, &i128::MAX);

    // After swap, k should be slightly higher due to fees
    let (final_a, final_b) = test.pool.get_rsrvs();
    let k_after = final_a * final_b;

    // k should increase because of the 0.3% fee
    assert!(k_after > k_before);
}

#[test]
fn test_withdraw_full_liquidity() {
    let test = LiquidityPoolTest::setup();

    // Deposit liquidity: seed gives user 10000 - 100 = 9900 shares, total = 10000
    test.mint_tokens(&test.user, 10_000);
    test.pool.deposit(&test.user, &10_000, &10_000, &10_000, &10_000);

    let shares = test.pool.balance_shares(&test.user);
    assert_eq!(shares, 9_900);

    // Withdraw all user shares (100 remain locked to burn address)
    let (out_a, out_b) = test.pool.withdraw(&test.user, &shares, &0, &0);

    assert_eq!(out_a, 9_900);
    assert_eq!(out_b, 9_900);

    let remaining_shares = test.pool.balance_shares(&test.user);
    assert_eq!(remaining_shares, 0);

    // 100 locked shares still hold 100 of each token
    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    assert_eq!(reserve_a, 100);
    assert_eq!(reserve_b, 100);
}

#[test]
fn test_withdraw_partial_liquidity() {
    let test = LiquidityPoolTest::setup();

    // Deposit liquidity: user gets 9900 shares, total = 10000
    test.mint_tokens(&test.user, 10_000);
    test.pool.deposit(&test.user, &10_000, &10_000, &10_000, &10_000);

    let shares = test.pool.balance_shares(&test.user);

    // Withdraw half of user shares
    let (out_a, out_b) = test.pool.withdraw(&test.user, &(shares / 2), &0, &0);

    assert_eq!(out_a, 4_950);
    assert_eq!(out_b, 4_950);

    let remaining_shares = test.pool.balance_shares(&test.user);
    assert_eq!(remaining_shares, shares / 2);

    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    assert_eq!(reserve_a, 5_050);
    assert_eq!(reserve_b, 5_050);
}

#[test]
#[should_panic(expected = "insufficient shares")]
fn test_withdraw_fails_insufficient_shares() {
    let test = LiquidityPoolTest::setup();

    // Deposit liquidity
    test.mint_tokens(&test.user, 10_000);
    test.pool.deposit(&test.user, &10_000, &10_000, &10_000, &10_000);

    let shares = test.pool.balance_shares(&test.user);

    // Try to withdraw more than owned
    test.pool.withdraw(&test.user, &(shares + 1), &0, &0);
}

#[test]
#[should_panic(expected = "min not satisfied")]
fn test_withdraw_fails_minimum_not_met() {
    let test = LiquidityPoolTest::setup();

    // Deposit liquidity
    test.mint_tokens(&test.user, 10_000);
    test.pool.deposit(&test.user, &10_000, &10_000, &10_000, &10_000);

    let shares = test.pool.balance_shares(&test.user);

    // Try to withdraw with impossible minimum requirements
    test.pool.withdraw(&test.user, &shares, &20_000, &20_000);
}

#[test]
fn test_multiple_liquidity_providers() {
    let test = LiquidityPoolTest::setup();

    // First provider deposits: gets 10000 - 100 = 9900 shares, total = 10000
    test.mint_tokens(&test.user, 10_000);
    test.pool.deposit(&test.user, &10_000, &10_000, &10_000, &10_000);

    // Second provider deposits
    let user2 = Address::generate(&test.env);
    test.mint_tokens(&user2, 5_000);
    test.pool.deposit(&user2, &5_000, &5_000, &5_000, &5_000);

    let shares1 = test.pool.balance_shares(&test.user);
    let shares2 = test.pool.balance_shares(&user2);

    assert_eq!(shares1, 9_900);
    assert_eq!(shares2, 5_000);

    let (reserve_a, reserve_b) = test.pool.get_rsrvs();
    assert_eq!(reserve_a, 15_000);
    assert_eq!(reserve_b, 15_000);
}

#[test]
fn test_withdraw_after_profitable_swaps() {
    let test = LiquidityPoolTest::setup();

    // LP deposits liquidity
    test.mint_tokens(&test.user, 100_000);
    test.pool.deposit(&test.user, &100_000, &100_000, &100_000, &100_000);

    let initial_shares = test.pool.balance_shares(&test.user);

    // Execute multiple swaps (LPs earn fees)
    for i in 0..5 {
        let swapper = Address::generate(&test.env);

        // Alternate swap directions to keep pool balanced
        if i % 2 == 0 {
            let token_a_admin = StellarAssetClient::new(&test.env, &test.token_a.address);
            token_a_admin.mint(&swapper, &50_000);
            test.pool.swap_pt_for_v(&swapper, &5_000, &i128::MAX);
        } else {
            let token_b_admin = StellarAssetClient::new(&test.env, &test.token_b.address);
            token_b_admin.mint(&swapper, &50_000);
            test.pool.swap_v_for_pt(&swapper, &5_000, &i128::MAX);
        }
    }

    // LP withdraws all liquidity
    let (out_a, out_b) = test.pool.withdraw(&test.user, &initial_shares, &0, &0);

    // LP should get back more than initially deposited due to accumulated fees
    assert!(out_a + out_b > 200_000);
}

#[test]
fn test_large_deposit_small_deposit_fairness() {
    let test = LiquidityPoolTest::setup();

    // Large initial deposit: user gets 1_000_000 - 100 = 999_900 shares, total = 1_000_000
    test.mint_tokens(&test.user, 1_000_000);
    test.pool.deposit(
        &test.user,
        &1_000_000,
        &1_000_000,
        &1_000_000,
        &1_000_000,
    );

    // Small deposit
    let user2 = Address::generate(&test.env);
    test.mint_tokens(&user2, 100);
    test.pool.deposit(&user2, &100, &100, &100, &100);

    let shares1 = test.pool.balance_shares(&test.user);
    let shares2 = test.pool.balance_shares(&user2);

    // shares1 = 999_900, shares2 = 100 → ratio = 9999
    assert_eq!(shares1 / shares2, 9_999);
}

#[test]
fn test_swap_both_directions_maintains_balance() {
    let test = LiquidityPoolTest::setup();

    // Setup pool
    test.mint_tokens(&test.user, 100_000);
    test.pool.deposit(&test.user, &100_000, &100_000, &100_000, &100_000);

    // Sell PT for V
    let swapper1 = Address::generate(&test.env);
    let token_a_admin = StellarAssetClient::new(&test.env, &test.token_a.address);
    token_a_admin.mint(&swapper1, &5_000);
    test.pool.swap_pt_for_v(&swapper1, &4_500, &i128::MAX);

    let (mid_reserve_a, mid_reserve_b) = test.pool.get_rsrvs();

    // Sell V for PT (reverse direction)
    let swapper2 = Address::generate(&test.env);
    let token_b_admin = StellarAssetClient::new(&test.env, &test.token_b.address);
    token_b_admin.mint(&swapper2, &5_000);
    test.pool.swap_v_for_pt(&swapper2, &4_500, &i128::MAX);

    let (final_reserve_a, final_reserve_b) = test.pool.get_rsrvs();

    // After swaps in both directions, reserves should be reasonably balanced
    let ratio_mid = mid_reserve_a * 100 / mid_reserve_b;
    let ratio_final = final_reserve_a * 100 / final_reserve_b;

    // Both should deviate from 100 but in opposite directions
    assert!(ratio_mid > 100);
    assert!(ratio_final < ratio_mid);
}

#[test]
fn test_price_impact() {
    let test = LiquidityPoolTest::setup();

    // Setup pool with 100k:100k liquidity
    test.mint_tokens(&test.user, 100_000);
    test.pool.deposit(&test.user, &100_000, &100_000, &100_000, &100_000);

    // Small swap should have less price impact
    let swapper1 = Address::generate(&test.env);
    let token_a_admin = StellarAssetClient::new(&test.env, &test.token_a.address);
    token_a_admin.mint(&swapper1, &1_000);

    test.pool.swap_pt_for_v(&swapper1, &900, &i128::MAX);
    let (reserve_a_after_small, reserve_b_after_small) = test.pool.get_rsrvs();

    // Calculate price impact for small swap
    let small_ratio = reserve_a_after_small * 100 / reserve_b_after_small;

    // Reset pool
    let test2 = LiquidityPoolTest::setup();
    test2.mint_tokens(&test2.user, 100_000);
    test2
        .pool
        .deposit(&test2.user, &100_000, &100_000, &100_000, &100_000);

    // Large swap should have more price impact
    let swapper2 = Address::generate(&test2.env);
    let token_a_admin2 = StellarAssetClient::new(&test2.env, &test2.token_a.address);
    token_a_admin2.mint(&swapper2, &10_000);

    test2.pool.swap_pt_for_v(&swapper2, &9_000, &i128::MAX);
    let (reserve_a_after_large, reserve_b_after_large) = test2.pool.get_rsrvs();

    // Calculate price impact for large swap
    let large_ratio = reserve_a_after_large * 100 / reserve_b_after_large;

    // Large swap should deviate more from 100 than small swap
    assert!(large_ratio > small_ratio);
}
#![cfg(test)]

use crate::{YieldToken};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, String, Symbol,
};

// Import contracts from the workspace
use mock_vault::{MockVault, MockVaultClient};
use principal_token::PrincipalToken;
use yield_manager::YieldManager;
use yield_manager_interface::VaultType;

struct YieldTokenTest<'a> {
    env: Env,
    user1: Address,
    user2: Address,
    vault: MockVaultClient<'a>,
    yield_manager: Address,
    yield_token: Address,
    pt: Address,
    maturity: u64,
}

impl<'a> YieldTokenTest<'a> {
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        // Create underlying asset
        let underlying_admin = Address::generate(&env);
        let underlying_asset_addr = env.register_stellar_asset_contract_v2(underlying_admin.clone());
        let underlying_asset = TokenClient::new(&env, &underlying_asset_addr.address());

        // Deploy mock vault with 1 basis point per second yield rate (0.01% per second)
        let vault_id = env.register(MockVault, (&underlying_asset.address, 1i128));
        let vault = MockVaultClient::new(&env, &vault_id);

        // Set maturity to 1000 seconds from now
        let current_time = env.ledger().timestamp();
        let maturity = current_time + 1000;

        // Deploy yield manager
        let yield_manager_id = env.register(YieldManager, (&admin, &vault_id, VaultType::Vault4626, maturity));

        // Mint vault shares to the yield manager for distributing yield
        // In real usage, these come from user deposits
        let test_depositor = Address::generate(&env);
        let underlying_admin_client = StellarAssetClient::new(&env, &underlying_asset.address);
        underlying_admin_client.mint(&test_depositor, &1_000_000_0000000i128);
        let shares = vault.deposit(&test_depositor, &1_000_000_0000000i128);
        vault.transfer(&test_depositor, &yield_manager_id, &shares);

        // Deploy PT and YT tokens
        let pt_id = env.register(
            PrincipalToken,
            (
                yield_manager_id.clone(),
                String::from_str(&env, "Principal Token"),
                String::from_str(&env, "PT"),
            ),
        );

        let yt_id = env.register(
            YieldToken,
            (
                yield_manager_id.clone(),
                String::from_str(&env, "Yield Token"),
                String::from_str(&env, "YT"),
            ),
        );

        // Set token contracts in yield manager
        env.invoke_contract::<()>(
            &yield_manager_id,
            &Symbol::new(&env, "set_token_contracts"),
            (&pt_id, &yt_id).into_val(&env),
        );

        YieldTokenTest {
            env,
            user1,
            user2,
            vault,
            yield_manager: yield_manager_id,
            yield_token: yt_id,
            pt: pt_id,
            maturity,
        }
    }

    fn mint_yt(&self, to: &Address, amount: i128, exchange_rate: i128) {
        self.env.invoke_contract::<()>(
            &self.yield_token,
            &Symbol::new(&self.env, "mint"),
            (to, amount, exchange_rate).into_val(&self.env),
        );
    }

    fn get_balance(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "balance"),
            (user,).into_val(&self.env),
        )
    }

    fn get_user_index(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "user_index"),
            (user,).into_val(&self.env),
        )
    }

    fn get_accrued_yield(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "accrued_yield"),
            (user,).into_val(&self.env),
        )
    }

    fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|li| {
            li.timestamp += seconds;
        });
    }

    fn get_exchange_rate(&self) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_manager,
            &Symbol::new(&self.env, "get_exchange_rate"),
            ().into_val(&self.env),
        )
    }

    fn claim_yield(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yield_token,
            &Symbol::new(&self.env, "claim_yield"),
            (user,).into_val(&self.env),
        )
    }

    fn transfer(&self, from: &Address, to: &Address, amount: i128) {
        self.env.invoke_contract::<()>(
            &self.yield_token,
            &Symbol::new(&self.env, "transfer"),
            (from, to, amount).into_val(&self.env),
        );
    }
}

#[test]
fn test_initialization() {
    let test = YieldTokenTest::setup();

    let name: String = test.env.invoke_contract(
        &test.yield_token,
        &Symbol::new(&test.env, "name"),
        ().into_val(&test.env),
    );
    assert_eq!(name, String::from_str(&test.env, "Yield Token"));

    let symbol: String = test.env.invoke_contract(
        &test.yield_token,
        &Symbol::new(&test.env, "symbol"),
        ().into_val(&test.env),
    );
    assert_eq!(symbol, String::from_str(&test.env, "YT"));
}

#[test]
fn test_mint_sets_initial_index() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000i128;
    let exchange_rate = 1_000_000i128;

    test.mint_yt(&test.user1, mint_amount, exchange_rate);

    let balance = test.get_balance(&test.user1);
    assert_eq!(balance, mint_amount);

    let user_index = test.get_user_index(&test.user1);
    assert_eq!(user_index, exchange_rate);
}

#[test]
fn test_yield_accrues_when_exchange_rate_increases() {
    let test = YieldTokenTest::setup();

    // Mint YT at current rate
    let mint_amount = 1_000_000_000_000i128; // 1M tokens scaled by 1e6
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // Check no yield initially
    let initial_accrued = test.get_accrued_yield(&test.user1);
    assert_eq!(initial_accrued, 0);

    // Advance time by 100 seconds to increase exchange rate
    // At 1 basis point per second (0.01%), 100 seconds = 1% increase
    test.advance_time(100);

    // Verify exchange rate increased
    let new_rate = test.get_exchange_rate();
    assert!(new_rate > initial_rate);

    // Trigger yield accrual by claiming
    let claimed = test.claim_yield(&test.user1);

    // User should have received some yield
    assert!(claimed > 0);

    // User should have vault shares now
    let vault_balance = test.vault.balance(&test.user1);
    assert_eq!(vault_balance, claimed);
}

#[test]
fn test_user_index_updates_after_accrual() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    let initial_index = test.get_user_index(&test.user1);
    assert_eq!(initial_index, initial_rate);

    // Increase rate by advancing time
    test.advance_time(200);
    let new_rate = test.get_exchange_rate();

    // Claim to trigger accrual
    test.claim_yield(&test.user1);

    // User index should be updated
    let updated_index = test.get_user_index(&test.user1);
    assert_eq!(updated_index, new_rate);
}

#[test]
fn test_multiple_claims_accumulate_yield() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // First increase
    test.advance_time(100);
    let claimed1 = test.claim_yield(&test.user1);
    assert!(claimed1 > 0);

    // Second increase
    test.advance_time(100);
    let claimed2 = test.claim_yield(&test.user1);
    assert!(claimed2 > 0);

    // Total vault shares received
    let total_vault_balance = test.vault.balance(&test.user1);
    assert_eq!(total_vault_balance, claimed1 + claimed2);
}

#[test]
fn test_transfer_accrues_yield_for_both_parties() {
    let test = YieldTokenTest::setup();

    // User1 gets YT
    let mint_amount = 2_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // Increase rate before transfer
    test.advance_time(100);
    let new_rate = test.get_exchange_rate();

    // Transfer to user2
    let transfer_amount = 1_000_000_000_000i128;
    test.transfer(&test.user1, &test.user2, transfer_amount);

    // Both users should have yield accrued from the rate increase
    let accrued1 = test.get_accrued_yield(&test.user1);

    // User1 should have accrued yield on their full balance before transfer
    assert!(accrued1 > 0);

    // User2 is new, so their index should be set to current rate
    let user2_index = test.get_user_index(&test.user2);
    assert_eq!(user2_index, new_rate);

    // Balances should be correct
    assert_eq!(test.get_balance(&test.user1), mint_amount - transfer_amount);
    assert_eq!(test.get_balance(&test.user2), transfer_amount);
}

#[test]
fn test_transfer_to_existing_user_preserves_index() {
    let test = YieldTokenTest::setup();

    let initial_rate = test.get_exchange_rate();

    // Both users get YT
    test.mint_yt(&test.user1, 1_000_000_000_000i128, initial_rate);
    test.mint_yt(&test.user2, 1_000_000_000_000i128, initial_rate);

    // Increase rate
    test.advance_time(100);
    let new_rate = test.get_exchange_rate();

    // Transfer from user1 to user2
    test.transfer(&test.user1, &test.user2, 500_000_000_000i128);

    // Both should have accrued yield
    let accrued1 = test.get_accrued_yield(&test.user1);
    let accrued2 = test.get_accrued_yield(&test.user2);

    assert!(accrued1 > 0);
    assert!(accrued2 > 0);

    // Both indices should be updated to new rate
    assert_eq!(test.get_user_index(&test.user1), new_rate);
    assert_eq!(test.get_user_index(&test.user2), new_rate);
}

#[test]
fn test_burn_accrues_yield_before_burning() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // Increase rate
    test.advance_time(100);

    // Burn tokens
    let burn_amount = 500_000_000_000i128;
    test.env.invoke_contract::<()>(
        &test.yield_token,
        &Symbol::new(&test.env, "burn"),
        (&test.user1, burn_amount).into_val(&test.env),
    );

    // Check yield was accrued before burn
    let accrued = test.get_accrued_yield(&test.user1);
    assert!(accrued > 0);

    // Balance should be reduced
    let balance = test.get_balance(&test.user1);
    assert_eq!(balance, mint_amount - burn_amount);

    // Total supply should be reduced
    let total_supply: i128 = test.env.invoke_contract(
        &test.yield_token,
        &Symbol::new(&test.env, "total_supply"),
        ().into_val(&test.env),
    );
    assert_eq!(total_supply, mint_amount - burn_amount);
}

#[test]
fn test_no_yield_if_rate_unchanged() {
    let test = YieldTokenTest::setup();

    let mint_amount = 1_000_000_000_000i128;
    let initial_rate = test.get_exchange_rate();
    test.mint_yt(&test.user1, mint_amount, initial_rate);

    // Claim without changing rate (don't advance time)
    let claimed = test.claim_yield(&test.user1);

    // Should be zero
    assert_eq!(claimed, 0);

    let vault_balance = test.vault.balance(&test.user1);
    assert_eq!(vault_balance, 0);
}

#[test]
fn test_proportional_yield_distribution() {
    let test = YieldTokenTest::setup();

    let initial_rate = test.get_exchange_rate();

    // User1 gets 2x as much as user2
    test.mint_yt(&test.user1, 2_000_000_000_000i128, initial_rate);
    test.mint_yt(&test.user2, 1_000_000_000_000i128, initial_rate);

    // Increase rate
    test.advance_time(100);

    // Both claim
    let claimed1 = test.claim_yield(&test.user1);
    let claimed2 = test.claim_yield(&test.user2);

    // User1 should get approximately 2x the yield of user2
    assert!(claimed1 > 0);
    assert!(claimed2 > 0);

    // Allow 1% tolerance for rounding
    let ratio = claimed1 * 100 / claimed2;
    assert!(ratio >= 190 && ratio <= 210); // Should be ~200
}

#[test]
fn test_mint_to_existing_user_preserves_high_water_mark() {
    let test = YieldTokenTest::setup();

    let initial_rate = test.get_exchange_rate();

    // User gets initial YT
    test.mint_yt(&test.user1, 1_000_000_000_000i128, initial_rate);

    // Rate increases
    test.advance_time(100);
    let new_rate = test.get_exchange_rate();

    // User gets more YT (should preserve their high water mark at new rate)
    test.mint_yt(&test.user1, 1_000_000_000_000i128, new_rate);

    // User index should be at new rate (not reset to initial)
    let user_index = test.get_user_index(&test.user1);
    assert_eq!(user_index, new_rate);

    // Should have accrued yield from the first mint
    let accrued = test.get_accrued_yield(&test.user1);
    assert!(accrued > 0);
}

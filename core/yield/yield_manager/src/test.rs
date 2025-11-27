#![cfg(test)]

use crate::{YieldManager, YieldManagerTrait};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, String, Symbol,
};

// Import contracts from the workspace
use mock_vault::{MockVault, MockVaultClient};
use principal_token::{PrincipalToken, PrincipalTokenTrait};
use yield_token::{YieldToken, YieldTokenTrait};

struct YieldManagerTest<'a> {
    env: Env,
    admin: Address,
    user1: Address,
    user2: Address,
    underlying_asset: TokenClient<'a>,
    vault: MockVaultClient<'a>,
    yield_manager: Address,
    pt: Address,
    yt: Address,
    maturity: u64,
}

impl<'a> YieldManagerTest<'a> {
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
        let yield_manager_id = env.register(YieldManager, (&admin, &vault_id, maturity));

        // Deploy PT and YT tokens
        let pt_id = env.register(
            PrincipalToken,
            (
                &yield_manager_id,
                String::from_str(&env, "Principal Token"),
                String::from_str(&env, "PT"),
            ),
        );

        let yt_id = env.register(
            YieldToken,
            (
                &yield_manager_id,
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

        YieldManagerTest {
            env,
            admin,
            user1,
            user2,
            underlying_asset,
            vault,
            yield_manager: yield_manager_id,
            pt: pt_id,
            yt: yt_id,
            maturity,
        }
    }

    fn mint_underlying(&self, to: &Address, amount: i128) {
        let admin = StellarAssetClient::new(&self.env, &self.underlying_asset.address);
        admin.mint(to, &amount);
    }

    fn get_pt_balance(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.pt,
            &Symbol::new(&self.env, "balance"),
            (user,).into_val(&self.env),
        )
    }

    fn get_yt_balance(&self, user: &Address) -> i128 {
        self.env.invoke_contract::<i128>(
            &self.yt,
            &Symbol::new(&self.env, "balance"),
            (user,).into_val(&self.env),
        )
    }

    fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|li| {
            li.timestamp += seconds;
        });
    }
}

#[test]
fn test_initialization() {
    let test = YieldManagerTest::setup();

    // Verify yield manager is initialized correctly
    let vault_addr: Address = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_vault"),
        ().into_val(&test.env),
    );
    assert_eq!(vault_addr, test.vault.address);

    let maturity: u64 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_maturity"),
        ().into_val(&test.env),
    );
    assert_eq!(maturity, test.maturity);
}

#[test]
fn test_deposit_mints_pt_and_yt() {
    let test = YieldManagerTest::setup();

    // User deposits underlying to vault
    let deposit_amount = 1_000_0000i128; // 1000 units with 7 decimals
    test.mint_underlying(&test.user1, deposit_amount);
    let shares = test.vault.deposit(&test.user1, &deposit_amount);

    // User deposits vault shares to yield manager
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares).into_val(&test.env),
    );

    // Check PT and YT balances
    let pt_balance = test.get_pt_balance(&test.user1);
    let yt_balance = test.get_yt_balance(&test.user1);

    // Both should equal shares * exchange_rate
    // exchange_rate is 1_000_000 (1.0 scaled by 1e6) initially
    let expected_balance = shares * 1_000_000;
    assert_eq!(pt_balance, expected_balance);
    assert_eq!(yt_balance, expected_balance);

    // Yield manager should hold the vault shares
    let ym_vault_balance = test.vault.balance(&test.yield_manager);
    assert_eq!(ym_vault_balance, shares);
}

#[test]
fn test_exchange_rate_increases_over_time() {
    let test = YieldManagerTest::setup();

    // Get initial exchange rate
    let initial_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Advance time by 100 seconds
    test.advance_time(100);

    // Exchange rate should increase (vault accrues yield over time)
    let new_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    assert!(new_rate > initial_rate);
}

#[test]
fn test_yt_accrues_yield_over_time() {
    let test = YieldManagerTest::setup();

    // User deposits
    let deposit_amount = 1_000_0000i128;
    test.mint_underlying(&test.user1, deposit_amount);
    let shares = test.vault.deposit(&test.user1, &deposit_amount);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares).into_val(&test.env),
    );

    // Check initial accrued yield (should be 0)
    let initial_accrued: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "accrued_yield"),
        (&test.user1,).into_val(&test.env),
    );
    assert_eq!(initial_accrued, 0);

    // Advance time to accrue yield
    test.advance_time(100);

    // Trigger yield accrual by calling claim_yield
    let claimed: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "claim_yield"),
        (&test.user1,).into_val(&test.env),
    );

    // User should have received some yield
    assert!(claimed > 0);

    // User should now have vault shares from yield
    let user_vault_balance = test.vault.balance(&test.user1);
    assert_eq!(user_vault_balance, claimed);
}

#[test]
fn test_exchange_rate_locks_at_maturity() {
    let test = YieldManagerTest::setup();

    // Get exchange rate before maturity
    test.advance_time(500); // Halfway to maturity
    let rate_before_maturity: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Advance past maturity
    test.advance_time(600); // Now past maturity (500 + 600 > 1000)

    // Get exchange rate at maturity (should be locked)
    let rate_at_maturity: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Rate should be higher than before maturity
    assert!(rate_at_maturity > rate_before_maturity);

    // Advance time further
    test.advance_time(1000);

    // Rate should still be the same (locked at maturity)
    let rate_after_maturity: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );
    assert_eq!(rate_after_maturity, rate_at_maturity);
}

#[test]
fn test_exchange_rate_high_water_mark() {
    let test = YieldManagerTest::setup();

    // Get initial exchange rate
    let initial_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Advance time to increase the vault's exchange rate
    test.advance_time(100);

    // Get the higher rate
    let higher_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    assert!(higher_rate > initial_rate);

    // Now set a negative yield rate to simulate the vault's exchange rate decreasing
    // (simulating a vault issue/slashing)
    test.vault.set_yield_rate(&(-100)); // -1% per second

    // Advance time so the negative yield takes effect
    test.advance_time(50);

    // Get exchange rate again - it should NOT decrease due to high water mark
    let rate_after_vault_decrease: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Rate should remain at the high water mark, not decrease
    assert_eq!(rate_after_vault_decrease, higher_rate);

    // Verify the vault's rate actually did decrease
    let vault_rate = test.vault.exchange_rate();
    assert!(vault_rate < higher_rate, "Vault rate should have decreased");
}

#[test]
#[should_panic(expected = "Maturity not reached")]
fn test_cannot_redeem_principal_before_maturity() {
    let test = YieldManagerTest::setup();

    // User deposits
    let deposit_amount = 1_000_0000i128;
    test.mint_underlying(&test.user1, deposit_amount);
    let shares = test.vault.deposit(&test.user1, &deposit_amount);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares).into_val(&test.env),
    );

    let pt_balance = test.get_pt_balance(&test.user1);

    // Try to redeem PT before maturity (should panic)
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "redeem_principal"),
        (&test.user1, pt_balance).into_val(&test.env),
    );
}

#[test]
fn test_redeem_principal_after_maturity() {
    let test = YieldManagerTest::setup();

    // User deposits
    let deposit_amount = 1_000_0000i128;
    test.mint_underlying(&test.user1, deposit_amount);
    let shares = test.vault.deposit(&test.user1, &deposit_amount);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares).into_val(&test.env),
    );

    let pt_balance = test.get_pt_balance(&test.user1);

    // Advance past maturity
    test.advance_time(1100);

    // Redeem PT for vault shares
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "redeem_principal"),
        (&test.user1, pt_balance).into_val(&test.env),
    );

    // Check PT was burned
    let pt_balance_after = test.get_pt_balance(&test.user1);
    assert_eq!(pt_balance_after, 0);

    // User should have received vault shares back
    let user_vault_balance = test.vault.balance(&test.user1);
    assert!(user_vault_balance > 0);
}

#[test]
fn test_multiple_users_deposit() {
    let test = YieldManagerTest::setup();

    // User1 deposits
    let deposit1 = 1_000_0000i128;
    test.mint_underlying(&test.user1, deposit1);
    let shares1 = test.vault.deposit(&test.user1, &deposit1);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares1).into_val(&test.env),
    );

    // User2 deposits
    let deposit2 = 2_000_0000i128;
    test.mint_underlying(&test.user2, deposit2);
    let shares2 = test.vault.deposit(&test.user2, &deposit2);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user2, shares2).into_val(&test.env),
    );

    // Check balances
    let pt1 = test.get_pt_balance(&test.user1);
    let pt2 = test.get_pt_balance(&test.user2);

    // User2 should have roughly 2x the PT of User1
    assert!(pt2 > pt1);
    assert!(pt2 >= pt1 * 2 - 100); // Allow some rounding
}

#[test]
fn test_yield_distribution_proportional() {
    let test = YieldManagerTest::setup();

    // Both users deposit equal amounts
    let deposit_amount = 1_000_0000i128;

    test.mint_underlying(&test.user1, deposit_amount);
    let shares1 = test.vault.deposit(&test.user1, &deposit_amount);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares1).into_val(&test.env),
    );

    test.mint_underlying(&test.user2, deposit_amount);
    let shares2 = test.vault.deposit(&test.user2, &deposit_amount);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user2, shares2).into_val(&test.env),
    );

    // Advance time to accrue yield
    test.advance_time(200);

    // Both claim yield
    let claimed1: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "claim_yield"),
        (&test.user1,).into_val(&test.env),
    );

    let claimed2: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "claim_yield"),
        (&test.user2,).into_val(&test.env),
    );

    // Both should receive roughly equal yield (within 1% tolerance)
    let diff = if claimed1 > claimed2 {
        claimed1 - claimed2
    } else {
        claimed2 - claimed1
    };
    assert!(diff < claimed1 / 100);
}

#[test]
fn test_pt_transferable() {
    let test = YieldManagerTest::setup();

    // User1 deposits
    let deposit_amount = 1_000_0000i128;
    test.mint_underlying(&test.user1, deposit_amount);
    let shares = test.vault.deposit(&test.user1, &deposit_amount);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares).into_val(&test.env),
    );

    let pt_balance = test.get_pt_balance(&test.user1);

    // Transfer half to user2
    let transfer_amount = pt_balance / 2;
    test.env.invoke_contract::<()>(
        &test.pt,
        &Symbol::new(&test.env, "transfer"),
        (&test.user1, &test.user2, transfer_amount).into_val(&test.env),
    );

    // Check balances
    let pt1_after = test.get_pt_balance(&test.user1);
    let pt2_after = test.get_pt_balance(&test.user2);

    assert_eq!(pt1_after, pt_balance - transfer_amount);
    assert_eq!(pt2_after, transfer_amount);
}

#[test]
fn test_yt_transferable() {
    let test = YieldManagerTest::setup();

    // User1 deposits
    let deposit_amount = 1_000_0000i128;
    test.mint_underlying(&test.user1, deposit_amount);
    let shares = test.vault.deposit(&test.user1, &deposit_amount);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares).into_val(&test.env),
    );

    let yt_balance = test.get_yt_balance(&test.user1);

    // Transfer half to user2
    let transfer_amount = yt_balance / 2;
    test.env.invoke_contract::<()>(
        &test.yt,
        &Symbol::new(&test.env, "transfer"),
        (&test.user1, &test.user2, transfer_amount).into_val(&test.env),
    );

    // Check balances
    let yt1_after = test.get_yt_balance(&test.user1);
    let yt2_after = test.get_yt_balance(&test.user2);

    assert_eq!(yt1_after, yt_balance - transfer_amount);
    assert_eq!(yt2_after, transfer_amount);
}

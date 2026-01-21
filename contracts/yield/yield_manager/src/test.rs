#![cfg(test)]
use crate::{YieldManager, VaultType};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::TokenClient,
    Address, Env, IntoVal, String, Symbol,
};

// Import contracts from the workspace
use principal_token::PrincipalToken;
use yield_token::YieldToken;
use mock_vault::MockVault;

struct YieldManagerTest {
    env: Env,
    admin: Address,
    user1: Address,
    user2: Address,
    vault_addr: Address,
    yield_manager: Address,
    pt: Address,
    yt: Address,
    maturity: u64,
}

impl YieldManagerTest {
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        // Deploy mock vault
        let vault_addr = env.register(MockVault, (&admin,));

        // Set maturity to 1000 seconds from now
        let current_time = env.ledger().timestamp();
        let maturity = current_time + 1000;

        // Deploy yield manager
        let yield_manager_id = env.register(YieldManager, (&admin, &vault_addr, VaultType::Vault4626, maturity));

        // Deploy PT and YT tokens TODO: Check the arguments with the actual constructors to make sure it's correct
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
            vault_addr,
            yield_manager: yield_manager_id,
            pt: pt_id,
            yt: yt_id,
            maturity,
        }
    }

    fn mint_vault_shares(&self, to: &Address, amount: i128) {
        // Mint vault shares directly to the user (mock vault is also a token)
        let vault_token = TokenClient::new(&self.env, &self.vault_addr);
        vault_token.mint(to, &amount);
    }

    fn set_vault_exchange_rate(&self, rate: i128) {
        self.env.invoke_contract::<()>(
            &self.vault_addr,
            &Symbol::new(&self.env, "set_exchange_rate"),
            (rate,).into_val(&self.env),
        );
    }

    fn vault_balance(&self, user: &Address) -> i128 {
        let token = TokenClient::new(&self.env, &self.vault_addr);
        token.balance(user)
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
    assert_eq!(vault_addr, test.vault_addr);

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

    // Mint vault shares to user
    let shares = 1_000_0000i128; // 1000 shares with 7 decimals
    test.mint_vault_shares(&test.user1, shares);

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
    let ym_vault_balance = test.vault_balance(&test.yield_manager);
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

    // Simulate yield by increasing vault exchange rate
    test.set_vault_exchange_rate(1_200_0000); // Increase from 1.0 to 1.2

    // Exchange rate should increase
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
    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
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

    // Simulate yield by increasing vault exchange rate
    test.set_vault_exchange_rate(1_200_0000); // Increase from 1.0 to 1.2

    // Trigger yield accrual by calling claim_yield
    let claimed: i128 = test.env.invoke_contract(
        &test.yt,
        &Symbol::new(&test.env, "claim_yield"),
        (&test.user1,).into_val(&test.env),
    );

    // User should have received some yield
    assert!(claimed > 0);

    // User should now have vault shares from yield
    let user_vault_balance = test.vault_balance(&test.user1);
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

    // Get initial exchange rate (1.0)
    let initial_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // Increase vault exchange rate to 1.5
    test.set_vault_exchange_rate(1_500_0000);

    // Get the higher rate
    let higher_rate: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    assert!(higher_rate > initial_rate);

    // Now decrease vault exchange rate to 1.2 (simulating a loss)
    test.set_vault_exchange_rate(1_200_0000);

    // Get rate - should still be the high water mark (1.5), not the decreased rate (1.2)
    let rate_after_decrease: i128 = test.env.invoke_contract(
        &test.yield_manager,
        &Symbol::new(&test.env, "get_exchange_rate"),
        ().into_val(&test.env),
    );

    // The rate should be locked at the higher value (high water mark)
    assert_eq!(rate_after_decrease, higher_rate);
    assert!(rate_after_decrease > 1_200_0000);
}

#[test]
#[should_panic(expected = "Maturity not reached")]
fn test_cannot_redeem_principal_before_maturity() {
    let test = YieldManagerTest::setup();

    // User deposits
    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
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
    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
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
    let user_vault_balance = test.vault_balance(&test.user1);
    assert!(user_vault_balance > 0);
}

#[test]
fn test_multiple_users_deposit() {
    let test = YieldManagerTest::setup();

    // User1 deposits
    let shares1 = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares1);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares1).into_val(&test.env),
    );

    // User2 deposits
    let shares2 = 2_000_0000i128;
    test.mint_vault_shares(&test.user2, shares2);
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
    let shares = 1_000_0000i128;

    test.mint_vault_shares(&test.user1, shares);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user1, shares).into_val(&test.env),
    );

    test.mint_vault_shares(&test.user2, shares);
    test.env.invoke_contract::<()>(
        &test.yield_manager,
        &Symbol::new(&test.env, "deposit"),
        (&test.user2, shares).into_val(&test.env),
    );

    // Simulate yield by increasing vault exchange rate
    test.set_vault_exchange_rate(1_200_0000);

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
    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
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
    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);
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

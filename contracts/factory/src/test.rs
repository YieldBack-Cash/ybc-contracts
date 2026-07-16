#![cfg(test)]

use crate::{
    contract::{ContractUpgraded, Market, MarketRolledOver, WasmHashesUpdated},
    Factory, FactoryClient, WasmHashes,
};
use mock_vault::{MockVault, MockVaultClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger},
    token::TokenClient,
    Address, Env, Event, String,
};
use yield_manager_interface::VaultType;

// Import compiled WASM bytecode for contracts the factory deploys.
mod ym_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/yield_manager.wasm");
}

mod pt_wasm {
    // TODO: look into using the separate interface crate and importing this as bytes instead of contractimport!
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/principal_token.wasm");
}

mod yt_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/yield_token.wasm");
}

mod amm_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/amm.wasm");
}

// Default AMM curve parems for tests. Mirrors contracts/amm/amm/src/tests/fixture.rs
// so a factory-deployed pool behaves the same as the AMM crate's own test fixture
const SCALAR_ROOT: i128 = 250_000_000; // 25.0 moderate curve steepness
const FEE_RATE_ROOT: i128 = 500_000; // 0.05, 5% annualized fee root
const INITIAL_ANCHOR: i128 = 11_000_000; // 1.1, 10% initial implied rate anchor
const LAST_IMPLIED_RATE: i128 = 1_000_000; // 0.1, 10% starting implied rate

struct FactoryTest {
    env: Env,
    admin: Address,
    user1: Address,
    factory_addr: Address,
    factory: FactoryClient<'static>,
    vault_addr: Address,
}

impl FactoryTest {
    fn setup() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);

        // Deploy mock vault (not factory-deployed, use env.register)
        let vault_addr = env.register(
            MockVault,
            (
                &admin,
                String::from_str(&env, "Mock Vault Token"),
                String::from_str(&env, "MVT"),
                7u32,
            ),
        );

        // Upload contract WASMs and get hashes
        let ym_hash = env.deployer().upload_contract_wasm(ym_wasm::WASM);
        let pt_hash = env.deployer().upload_contract_wasm(pt_wasm::WASM);
        let yt_hash = env.deployer().upload_contract_wasm(yt_wasm::WASM);

        let amm_hash = env.deployer().upload_contract_wasm(amm_wasm::WASM);

        let wasm_hashes = WasmHashes {
            pt: pt_hash,
            yt: yt_hash,
            ym: ym_hash,
            amm: amm_hash,
        };

        let factory_addr = env.register(Factory, (&admin, wasm_hashes));
        let factory = FactoryClient::new(&env, &factory_addr);

        FactoryTest {
            env,
            admin,
            user1,
            factory_addr,
            factory,
            vault_addr,
        }
    }

    fn create_market(&self, maturity: u64) -> Address {
        self.factory
            .create_market(
                &self.vault_addr,
                &VaultType::Vault4626,
                &maturity,
                &SCALAR_ROOT,
                &INITIAL_ANCHOR,
                &FEE_RATE_ROOT,
                &LAST_IMPLIED_RATE,
            )
            .ym
    }

    fn mint_vault_shares(&self, to: &Address, amount: i128) {
        let vault_client = MockVaultClient::new(&self.env, &self.vault_addr);
        vault_client.mint(to, &amount);
    }

    fn advance_time(&self, seconds: u64) {
        self.env.ledger().with_mut(|li| {
            li.timestamp += seconds;
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_getters_before_deployment() {
    let test = FactoryTest::setup();

    assert_eq!(
        test.factory.get_current_yield_manager(&test.vault_addr),
        None
    );
    assert_eq!(test.factory.get_current_pt_token(&test.vault_addr), None);
    assert_eq!(test.factory.get_current_yt_token(&test.vault_addr), None);
}

#[test]
fn test_deploy_yield_manager() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let ym_addr = test.create_market(maturity);

    // All three addresses should be stored
    assert_eq!(
        test.factory.get_current_yield_manager(&test.vault_addr),
        Some(ym_addr.clone())
    );
    assert!(test
        .factory
        .get_current_pt_token(&test.vault_addr)
        .is_some());
    assert!(test
        .factory
        .get_current_yt_token(&test.vault_addr)
        .is_some());

    // Deployed YM should point at the correct vault
    let ym_client = ym_wasm::Client::new(&test.env, &ym_addr);
    assert_eq!(ym_client.get_vault(), test.vault_addr);

    // Deployed YM should have the correct maturity
    assert_eq!(ym_client.get_maturity(), maturity);
}

#[test]
fn test_ym_knows_its_tokens() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let ym_addr = test.create_market(maturity);
    let pt_addr = test.factory.get_current_pt_token(&test.vault_addr).unwrap();
    let yt_addr = test.factory.get_current_yt_token(&test.vault_addr).unwrap();

    let ym_client = ym_wasm::Client::new(&test.env, &ym_addr);

    // YM should know about PT
    assert_eq!(ym_client.get_principal_token(), pt_addr);

    // YM should know about YT
    assert_eq!(ym_client.get_yield_token(), yt_addr);
}

#[test]
fn test_deployed_pt_metadata() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;
    test.create_market(maturity);

    let pt_addr = test.factory.get_current_pt_token(&test.vault_addr).unwrap();
    let pt_token = TokenClient::new(&test.env, &pt_addr);
    let vault_symbol = TokenClient::new(&test.env, &test.vault_addr).symbol();

    assert_eq!(
        pt_token.name(),
        crate::contract::build_token_string(&test.env, "PT-", &vault_symbol, Some(maturity))
    );
    assert_eq!(
        pt_token.symbol(),
        crate::contract::build_token_string(&test.env, "PT-", &vault_symbol, None)
    );
    assert_eq!(pt_token.decimals(), 7);
}

#[test]
fn test_deployed_yt_metadata() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;
    test.create_market(maturity);

    let yt_addr = test.factory.get_current_yt_token(&test.vault_addr).unwrap();
    let yt_token = TokenClient::new(&test.env, &yt_addr);
    let vault_symbol = TokenClient::new(&test.env, &test.vault_addr).symbol();

    assert_eq!(
        yt_token.name(),
        crate::contract::build_token_string(&test.env, "YT-", &vault_symbol, Some(maturity))
    );
    assert_eq!(
        yt_token.symbol(),
        crate::contract::build_token_string(&test.env, "YT-", &vault_symbol, None)
    );
    assert_eq!(yt_token.decimals(), 7);
}

#[test]
fn test_deposit_through_factory_deployed_contracts() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let ym_addr = test.create_market(maturity);
    let pt_addr = test.factory.get_current_pt_token(&test.vault_addr).unwrap();
    let yt_addr = test.factory.get_current_yt_token(&test.vault_addr).unwrap();

    // Mint vault shares to user
    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);

    let vault_client = MockVaultClient::new(&test.env, &test.vault_addr);
    vault_client.approve(
        &test.user1,
        &ym_addr,
        &shares,
        &(test.env.ledger().sequence() + 1000),
    );

    // Deposit through the factory-deployed yield manager
    let ym_client = ym_wasm::Client::new(&test.env, &ym_addr);
    ym_client.deposit(&test.user1, &shares);

    // PT and YT should have been minted
    let pt_balance = TokenClient::new(&test.env, &pt_addr).balance(&test.user1);
    assert!(pt_balance > 0);

    let yt_balance = TokenClient::new(&test.env, &yt_addr).balance(&test.user1);
    assert!(yt_balance > 0);

    // Both should be equal (shares * exchange_rate)
    assert_eq!(pt_balance, yt_balance);

    // Yield manager should hold the vault shares
    let vault_token = TokenClient::new(&test.env, &test.vault_addr);
    assert_eq!(vault_token.balance(&ym_addr), shares);
}

#[test]
fn test_create_market_deploys_working_pool() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let market = test.factory.create_market(
        &test.vault_addr,
        &VaultType::Vault4626,
        &maturity,
        &SCALAR_ROOT,
        &INITIAL_ANCHOR,
        &FEE_RATE_ROOT,
        &LAST_IMPLIED_RATE,
    );

    let pool_client = amm_wasm::Client::new(&test.env, &market.pool);
    assert_eq!(pool_client.get_reserves(), (0, 0));
}

#[test]
fn test_rollover_before_maturity_returns_false() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;
    test.create_market(maturity);

    let rolled = test.factory.rollover_if_expired(
        &test.vault_addr,
        &VaultType::Vault4626,
        &(maturity + 2000),
        &SCALAR_ROOT,
        &INITIAL_ANCHOR,
        &FEE_RATE_ROOT,
        &LAST_IMPLIED_RATE,
    );
    assert!(!rolled);
}

#[test]
fn test_rollover_with_no_deployment_returns_false() {
    let test = FactoryTest::setup();

    let rolled = test.factory.rollover_if_expired(
        &test.vault_addr,
        &VaultType::Vault4626,
        &5000u64,
        &SCALAR_ROOT,
        &INITIAL_ANCHOR,
        &FEE_RATE_ROOT,
        &LAST_IMPLIED_RATE,
    );
    assert!(!rolled);
}

#[test]
fn test_set_wasm_hashes_emits_event() {
    let test = FactoryTest::setup();
    let old_hashes = test.factory.get_wasm_hashes();

    let new_hash = test.env.deployer().upload_contract_wasm(ym_wasm::WASM);
    let new_hashes = WasmHashes {
        pt: new_hash.clone(),
        yt: new_hash.clone(),
        ym: new_hash.clone(),
        amm: new_hash,
    };

    test.factory.set_wasm_hashes(&new_hashes);

    let expected = WasmHashesUpdated {
        old_hashes,
        new_hashes,
    };

    let events = test.env.events().all();
    let raw = events.events();
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0], expected.to_xdr(&test.env, &test.factory_addr));
}

#[test]
fn test_upgrade_emits_event() {
    let test = FactoryTest::setup();
    let new_wasm_hash = test.env.deployer().upload_contract_wasm(ym_wasm::WASM);

    test.factory.upgrade(&new_wasm_hash);

    let expected = ContractUpgraded { new_wasm_hash };

    let events = test.env.events().all();
    let raw = events.events();
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0], expected.to_xdr(&test.env, &test.factory_addr));
}

#[test]
fn test_rollover_after_expiry_emits_event() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    test.create_market(maturity);

    let old_market = Market {
        ym: test
            .factory
            .get_current_yield_manager(&test.vault_addr)
            .unwrap(),
        pt: test.factory.get_current_pt_token(&test.vault_addr).unwrap(),
        yt: test.factory.get_current_yt_token(&test.vault_addr).unwrap(),
        pool: test.factory.get_current_pool(&test.vault_addr).unwrap(),
        maturity,
        vault: test.vault_addr.clone(),
    };

    test.advance_time(1500); // past maturity
    let new_maturity = test.env.ledger().timestamp() + 1000;

    let rolled = test.factory.rollover_if_expired(
        &test.vault_addr,
        &VaultType::Vault4626,
        &new_maturity,
        &SCALAR_ROOT,
        &INITIAL_ANCHOR,
        &FEE_RATE_ROOT,
        &LAST_IMPLIED_RATE,
    );
    assert!(rolled);

    // we capture events immediately; the sub-deployed YM/AMM emit their own
    // init events, so keep only the factory's
    let events = test
        .env
        .events()
        .all()
        .filter_by_contract(&test.factory_addr);
    let raw = events.events();

    let new_market = Market {
        ym: test
            .factory
            .get_current_yield_manager(&test.vault_addr)
            .unwrap(),
        pt: test.factory.get_current_pt_token(&test.vault_addr).unwrap(),
        yt: test.factory.get_current_yt_token(&test.vault_addr).unwrap(),
        pool: test.factory.get_current_pool(&test.vault_addr).unwrap(),
        maturity: new_maturity,
        vault: test.vault_addr.clone(),
    };

    let expected = MarketRolledOver {
        vault: test.vault_addr.clone(),
        old_market,
        new_market,
    };

    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0], expected.to_xdr(&test.env, &test.factory_addr));
}

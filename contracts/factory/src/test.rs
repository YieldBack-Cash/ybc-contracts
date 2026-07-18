#![cfg(test)]

use crate::{
    contract::Market,
    events::{ContractUpgraded, MarketCreated, WasmHashesUpdated},
    Factory, FactoryClient, WasmHashes,
};
use mock_vault::{MockVault, MockVaultClient};
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger},
    token::TokenClient,
    Address, Env, Event, String,
};
use yield_manager_interface::VaultType;

mod ym_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/yield_manager.wasm");
}

mod pt_wasm {
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
            user1,
            factory_addr,
            factory,
            vault_addr,
        }
    }

    fn create_market(&self, maturity: u64) -> Market {
        self.factory.create_market(
            &self.vault_addr,
            &VaultType::Vault4626,
            &maturity,
            &SCALAR_ROOT,
            &INITIAL_ANCHOR,
            &FEE_RATE_ROOT,
            &LAST_IMPLIED_RATE,
        )
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

#[test]
fn test_getters_before_deployment() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    assert!(test.factory.get_market(&test.vault_addr, &maturity).is_none());
}

#[test]
#[should_panic(expected = "maturity must be in the future")]
fn test_create_market_past_maturity_panics() {
    let test = FactoryTest::setup();
    test.advance_time(1000);
    let maturity = test.env.ledger().timestamp() - 1;

    test.create_market(maturity);
}

#[test]
#[should_panic(expected = "maturity must be in the future")]
fn test_create_market_maturity_now_panics() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp();

    test.create_market(maturity);
}

#[test]
#[should_panic(expected = "maturity too far in the future")]
fn test_create_market_maturity_beyond_horizon_panics() {
    let test = FactoryTest::setup();
    // e.g. a milliseconds-instead-of-seconds mistake lands far past 10 years
    let maturity = test.env.ledger().timestamp() + 11 * 365 * 24 * 60 * 60;

    test.create_market(maturity);
}

#[test]
fn test_deploy_yield_manager() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let market = test.create_market(maturity);

    let stored = test
        .factory
        .get_market(&test.vault_addr, &maturity)
        .unwrap();
    assert_eq!(stored.ym, market.ym);
    assert_eq!(stored.pt, market.pt);
    assert_eq!(stored.yt, market.yt);
    assert_eq!(stored.pool, market.pool);

    let ym_client = ym_wasm::Client::new(&test.env, &market.ym);
    assert_eq!(ym_client.get_vault(), test.vault_addr);

    assert_eq!(ym_client.get_maturity(), maturity);
}

#[test]
fn test_ym_knows_its_tokens() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let market = test.create_market(maturity);

    let ym_client = ym_wasm::Client::new(&test.env, &market.ym);

    assert_eq!(ym_client.get_principal_token(), market.pt);

    assert_eq!(ym_client.get_yield_token(), market.yt);
}

#[test]
fn test_deployed_pt_metadata() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;
    let market = test.create_market(maturity);

    let pt_token = TokenClient::new(&test.env, &market.pt);
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
    let market = test.create_market(maturity);

    let yt_token = TokenClient::new(&test.env, &market.yt);
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

    let market = test.create_market(maturity);

    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);

    let vault_client = MockVaultClient::new(&test.env, &test.vault_addr);
    vault_client.approve(
        &test.user1,
        &market.ym,
        &shares,
        &(test.env.ledger().sequence() + 1000),
    );

    let ym_client = ym_wasm::Client::new(&test.env, &market.ym);
    ym_client.deposit(&test.user1, &shares);

    let pt_balance = TokenClient::new(&test.env, &market.pt).balance(&test.user1);
    assert!(pt_balance > 0);

    let yt_balance = TokenClient::new(&test.env, &market.yt).balance(&test.user1);
    assert!(yt_balance > 0);

    // Both should be equal (shares * exchange_rate)
    assert_eq!(pt_balance, yt_balance);

    let vault_token = TokenClient::new(&test.env, &test.vault_addr);
    assert_eq!(vault_token.balance(&market.ym), shares);
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
fn test_second_market_same_vault_different_maturity_coexists() {
    let test = FactoryTest::setup();
    let m1 = test.env.ledger().timestamp() + 1000;
    let m2 = m1 + 5000;

    let market1 = test.create_market(m1);
    let market2 = test.create_market(m2);

    assert_ne!(market1.ym, market2.ym);
    assert_ne!(market1.pool, market2.pool);
    assert_ne!(market1.pt, market2.pt);
    assert_ne!(market1.yt, market2.yt);

    // Both remain independently retrievable by their maturities — creating the
    // second did not overwrite the first.
    assert_eq!(test.factory.get_market(&test.vault_addr, &m1).unwrap().pool, market1.pool);
    assert_eq!(test.factory.get_market(&test.vault_addr, &m2).unwrap().pool, market2.pool);
}

#[test]
#[should_panic(expected = "market already exists for this vault and maturity")]
fn test_duplicate_market_same_maturity_panics() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;
    test.create_market(maturity);
    // A second market for the same (vault, maturity) must be rejected so the
    // existing pool can never be overwritten or orphaned.
    test.create_market(maturity);
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
fn test_create_market_emits_event() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let market = test.create_market(maturity);

    let expected = MarketCreated {
        vault: test.vault_addr.clone(),
        market,
    };

    // The sub-deployed YM/PT/YT/AMM emit their own init events, so keep only the
    // factory's — there should be exactly one: MarketCreated.
    let events = test
        .env
        .events()
        .all()
        .filter_by_contract(&test.factory_addr);
    let raw = events.events();

    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0], expected.to_xdr(&test.env, &test.factory_addr));
}

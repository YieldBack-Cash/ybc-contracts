#![cfg(test)]

use crate::{Factory, FactoryClient, WasmHashes};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::TokenClient,
    Address, Env, String,
};
use mock_vault::{MockVault, MockVaultClient};
use yield_manager_interface::VaultType;

// Import compiled WASM bytecode for contracts the factory deploys.
mod ym_wasm {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/yield_manager.wasm"
    );
}

mod pt_wasm { // TODO: look into using the separate interface crate and importing this as bytes instead of contractimport!
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/principal_token.wasm"
    );
}

mod yt_wasm {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32v1-none/release/yield_token.wasm"
    );
}

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

        // AMM is under development — use a placeholder hash for now
        let amm_hash = ym_hash.clone();

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

    fn deploy_yield_manager(&self, maturity: u64) -> Address {
        self.factory.deploy_yield_manager(&self.vault_addr, &VaultType::Vault4626, &maturity)
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

    assert_eq!(test.factory.get_current_yield_manager(), None);
    assert_eq!(test.factory.get_current_pt_token(), None);
    assert_eq!(test.factory.get_current_yt_token(), None);
}

#[test]
fn test_deploy_yield_manager() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let ym_addr = test.deploy_yield_manager(maturity);

    // All three addresses should be stored
    assert_eq!(test.factory.get_current_yield_manager(), Some(ym_addr.clone()));
    assert!(test.factory.get_current_pt_token().is_some());
    assert!(test.factory.get_current_yt_token().is_some());

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

    let ym_addr = test.deploy_yield_manager(maturity);
    let pt_addr = test.factory.get_current_pt_token().unwrap();
    let yt_addr = test.factory.get_current_yt_token().unwrap();

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
    test.deploy_yield_manager(maturity);

    let pt_addr = test.factory.get_current_pt_token().unwrap();
    let pt_token = TokenClient::new(&test.env, &pt_addr);

    assert_eq!(pt_token.name(), String::from_str(&test.env, "Principal Token"));
    assert_eq!(pt_token.symbol(), String::from_str(&test.env, "PT"));
    assert_eq!(pt_token.decimals(), 7);
}

#[test]
fn test_deployed_yt_metadata() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;
    test.deploy_yield_manager(maturity);

    let yt_addr = test.factory.get_current_yt_token().unwrap();
    let yt_token = TokenClient::new(&test.env, &yt_addr);

    assert_eq!(yt_token.name(), String::from_str(&test.env, "Yield Token"));
    assert_eq!(yt_token.symbol(), String::from_str(&test.env, "YT"));
    assert_eq!(yt_token.decimals(), 7);
}

#[test]
fn test_deposit_through_factory_deployed_contracts() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;

    let ym_addr = test.deploy_yield_manager(maturity);
    let pt_addr = test.factory.get_current_pt_token().unwrap();
    let yt_addr = test.factory.get_current_yt_token().unwrap();

    // Mint vault shares to user
    let shares = 1_000_0000i128;
    test.mint_vault_shares(&test.user1, shares);

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
fn test_rollover_before_maturity_returns_false() {
    let test = FactoryTest::setup();
    let maturity = test.env.ledger().timestamp() + 1000;
    test.deploy_yield_manager(maturity);

    let rolled = test.factory.rollover_if_expired(&VaultType::Vault4626, &(maturity + 2000));
    assert!(!rolled);
}

#[test]
fn test_rollover_with_no_deployment_returns_false() {
    let test = FactoryTest::setup();

    let rolled = test.factory.rollover_if_expired(&VaultType::Vault4626, &5000u64);
    assert!(!rolled);
}
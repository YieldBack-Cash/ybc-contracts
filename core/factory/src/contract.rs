use soroban_sdk::{Address, BytesN, Env, String};
use crate::storage;
use yield_manager_interface::YieldManagerClient;

#[cfg(feature = "contract")]
use soroban_sdk::{contract, contractimpl};

const PT_WASM_HASH: [u8; 32] = [0u8; 32];
const YT_WASM_HASH: [u8; 32] = [0u8; 32];
const YM_WASM_HASH: [u8; 32] = [0u8; 32];
const AMM_WASM_HASH: [u8; 32] = [0u8; 32];

pub trait FactoryTrait {
    fn __constructor(env: Env, admin: Address);

    fn deploy_yield_manager(
        env: Env,
        vault: Address,
        maturity: u64,
    ) -> Address;

    fn deploy_liquidity_pools(
        env: Env,
        pt_token: Address,
        yt_token: Address,
        vault_share_token: Address,
    ) -> (Address, Address);

    // Getter functions for current contracts
    fn get_current_yield_manager(env: Env) -> Option<Address>;
    fn get_current_pt_token(env: Env) -> Option<Address>;
    fn get_current_yt_token(env: Env) -> Option<Address>;
    fn get_current_pt_pool(env: Env) -> Option<Address>;
    fn get_current_yt_pool(env: Env) -> Option<Address>;

    // Rollover function to deploy new contracts after maturity
    fn rollover_if_expired(env: Env, new_maturity: u64) -> bool;
}

#[cfg(feature = "contract")]
#[contract]
pub struct Factory;

#[cfg(feature = "contract")]
#[contractimpl]
impl FactoryTrait for Factory {
    fn __constructor(env: Env, admin: Address) {
        storage::set_admin(&env, &admin);
    }

    fn deploy_yield_manager(
        env: Env,
        vault: Address,
        maturity: u64,
    ) -> Address {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        // Create WASM hash BytesN from constants
        let pt_wasm_hash = BytesN::from_array(&env, &PT_WASM_HASH);
        let yt_wasm_hash = BytesN::from_array(&env, &YT_WASM_HASH);
        let ym_wasm_hash = BytesN::from_array(&env, &YM_WASM_HASH);

        // Deploy yield manager first
        // Use a unique salt based on vault address and maturity
        let ym_salt_data = [0u8; 32];
        // Simple salt derivation - could be made more sophisticated
        let ym_salt = BytesN::from_array(&env, &ym_salt_data);

        let ym_addr = env
            .deployer()
            .with_current_contract(ym_salt.clone())
            .deploy_v2(
                ym_wasm_hash,
                (
                    env.current_contract_address(),
                    vault,
                    maturity,
                ),
            );

        // Deploy Principal Token with yield manager as admin
        let pt_salt = BytesN::from_array(&env, &[0u8; 32]);
        let pt_addr = env
            .deployer()
            .with_current_contract(pt_salt)
            .deploy_v2(
                pt_wasm_hash,
                (
                    ym_addr.clone(),
                    String::from_str(&env, "Principal Token"),
                    String::from_str(&env, "PT"),
                ),
            );

        // Deploy Yield Token with yield manager as admin
        let yt_salt = BytesN::from_array(&env, &[1u8; 32]);
        let yt_addr = env
            .deployer()
            .with_current_contract(yt_salt)
            .deploy_v2(
                yt_wasm_hash,
                (
                    ym_addr.clone(),
                    String::from_str(&env, "Yield Token"),
                    String::from_str(&env, "YT"),
                ),
            );

        // Set token contracts in yield manager
        let ym_client = YieldManagerClient::new(&env, &ym_addr);
        ym_client.set_token_contracts(&pt_addr, &yt_addr);

        // Store current contracts in factory storage
        storage::set_current_yield_manager(&env, &ym_addr);
        storage::set_current_pt_token(&env, &pt_addr);
        storage::set_current_yt_token(&env, &yt_addr);

        ym_addr
    }

    fn deploy_liquidity_pools(
        env: Env,
        pt_token: Address,
        yt_token: Address,
        vault_share_token: Address,
    ) -> (Address, Address) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let amm_wasm_hash = BytesN::from_array(&env, &AMM_WASM_HASH);

        // Deploy PT/Vault Share AMM pool
        let pt_pool_salt = BytesN::from_array(&env, &[2u8; 32]);
        let pt_pool_addr = env
            .deployer()
            .with_current_contract(pt_pool_salt)
            .deploy_v2(
                amm_wasm_hash.clone(),
                (pt_token, vault_share_token.clone()),
            );

        // Deploy YT/Vault Share AMM pool
        let yt_pool_salt = BytesN::from_array(&env, &[3u8; 32]);
        let yt_pool_addr = env
            .deployer()
            .with_current_contract(yt_pool_salt)
            .deploy_v2(
                amm_wasm_hash,
                (yt_token, vault_share_token),
            );

        // Store current pool addresses in factory storage
        storage::set_current_pt_pool(&env, &pt_pool_addr);
        storage::set_current_yt_pool(&env, &yt_pool_addr);

        (pt_pool_addr, yt_pool_addr)
    }

    // Getter functions for current contracts
    fn get_current_yield_manager(env: Env) -> Option<Address> {
        storage::get_current_yield_manager(&env)
    }

    fn get_current_pt_token(env: Env) -> Option<Address> {
        storage::get_current_pt_token(&env)
    }

    fn get_current_yt_token(env: Env) -> Option<Address> {
        storage::get_current_yt_token(&env)
    }

    fn get_current_pt_pool(env: Env) -> Option<Address> {
        storage::get_current_pt_pool(&env)
    }

    fn get_current_yt_pool(env: Env) -> Option<Address> {
        storage::get_current_yt_pool(&env)
    }

    /// Checks if current yield manager has expired and deploys new contracts if so
    /// Returns true if rollover occurred, false otherwise
    fn rollover_if_expired(env: Env, new_maturity: u64) -> bool {
        // Get current yield manager
        let current_ym = match storage::get_current_yield_manager(&env) {
            Some(ym) => ym,
            None => return false, // No yield manager deployed yet
        };

        // Check if maturity has expired
        let ym_client = YieldManagerClient::new(&env, &current_ym);
        let maturity = ym_client.get_maturity();
        let current_timestamp = env.ledger().timestamp();

        if current_timestamp < maturity {
            // Not expired yet
            return false;
        }

        // Maturity has expired, deploy new contracts
        let vault = ym_client.get_vault();

        // Deploy new yield manager with new maturity
        // This sets new yt/pt tokens in storage
        let new_ym_addr = Self::deploy_yield_manager(env.clone(), vault.clone(), new_maturity);

        // Get the newly deployed token addresses from storage
        let new_pt_addr = storage::get_current_pt_token(&env).unwrap();
        let new_yt_addr = storage::get_current_yt_token(&env).unwrap();

        // Deploy new liquidity pools
        // Vault address is the vault share token
        Self::deploy_liquidity_pools(
            env,
            new_pt_addr,
            new_yt_addr,
            vault,
        );

        true
    }
}

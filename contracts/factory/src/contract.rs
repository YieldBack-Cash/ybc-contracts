use soroban_sdk::{contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, String, Vec};
use crate::storage;
use yield_manager_interface::{YieldManagerClient, VaultType};

#[contracttype]
#[derive(Clone)]
pub struct Market {
    pub ym: Address,
    pub pt: Address,
    pub yt: Address,
    pub pt_pool: Address,
    pub yt_pool: Address,
    pub maturity: u64,
    pub vault: Address,
}

#[contracttype]
#[derive(Clone)]
pub struct WasmHashes {
    pub pt: BytesN<32>,
    pub yt: BytesN<32>,
    pub ym: BytesN<32>,
    pub amm: BytesN<32>,
}

pub trait FactoryTrait {
    fn __constructor(env: Env, admin: Address, wasm_hashes: WasmHashes);

    fn deploy_yield_manager(
        env: Env,
        vault: Address,
        vault_type: VaultType,
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

    fn get_markets(env: Env) -> Vec<Market>;

    // Rollover function to deploy new contracts after maturity
    fn rollover_if_expired(env: Env, vault_type: VaultType, new_maturity: u64) -> bool;
}

#[contract]
pub struct Factory;

fn next_salt(env: &Env) -> BytesN<32> {
    let counter = storage::get_salt_counter(env);
    storage::set_salt_counter(env, counter + 1);
    let mut buf = Bytes::new(env);
    buf.extend_from_array(&counter.to_be_bytes());
    env.crypto().keccak256(&buf).into()
}

#[contractimpl]
impl FactoryTrait for Factory {
    fn __constructor(env: Env, admin: Address, wasm_hashes: WasmHashes) {
        storage::set_admin(&env, &admin);
        storage::set_wasm_hashes(&env, &wasm_hashes);
    }

    fn deploy_yield_manager(
        env: Env,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
    ) -> Address {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let wasm_hashes = storage::get_wasm_hashes(&env);

        let ym_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(
                wasm_hashes.ym,
                (
                    env.current_contract_address(),
                    vault,
                    vault_type,
                    maturity,
                ),
            );

        let pt_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(
                wasm_hashes.pt,
                (
                    ym_addr.clone(),
                    String::from_str(&env, "Principal Token"),
                    String::from_str(&env, "PT"),
                    7u32,
                ),
            );

        let yt_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(
                wasm_hashes.yt,
                (
                    ym_addr.clone(),
                    String::from_str(&env, "Yield Token"),
                    String::from_str(&env, "YT"),
                    7u32,
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

        let wasm_hashes = storage::get_wasm_hashes(&env);

        let pt_pool_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(
                wasm_hashes.amm.clone(),
                (pt_token, vault_share_token.clone()),
            );

        let yt_pool_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(
                wasm_hashes.amm,
                (yt_token, vault_share_token),
            );

        // Store current pool addresses in factory storage
        storage::set_current_pt_pool(&env, &pt_pool_addr);
        storage::set_current_yt_pool(&env, &yt_pool_addr);

        // Record this market in history
        let ym_addr = storage::get_current_yield_manager(&env)
            .expect("No yield manager deployed");
        let ym_client = YieldManagerClient::new(&env, &ym_addr);
        storage::push_market(&env, Market {
            pt: ym_client.get_principal_token(),
            yt: ym_client.get_yield_token(),
            maturity: ym_client.get_maturity(),
            vault: ym_client.get_vault(),
            ym: ym_addr,
            pt_pool: pt_pool_addr.clone(),
            yt_pool: yt_pool_addr.clone(),
        });

        (pt_pool_addr, yt_pool_addr)
    }

    // Getter functions for current contracts
    fn get_markets(env: Env) -> Vec<Market> {
        storage::get_markets(&env)
    }

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
    fn rollover_if_expired(env: Env, vault_type: VaultType, new_maturity: u64) -> bool {
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

        // Deploy new yield manager with new maturity (nonce ensures unique salts)
        Self::deploy_yield_manager(env.clone(), vault.clone(), vault_type, new_maturity);

        // Get the newly deployed token addresses from storage
        let new_pt_addr = storage::get_current_pt_token(&env).unwrap();
        let new_yt_addr = storage::get_current_yt_token(&env).unwrap();

        // Deploy new liquidity pools
        Self::deploy_liquidity_pools(
            env,
            new_pt_addr,
            new_yt_addr,
            vault,
        );

        true
    }
}

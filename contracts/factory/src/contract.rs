use soroban_sdk::{contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, String, Vec};
use crate::storage;
use yield_manager_interface::{YieldManagerClient, VaultType};

#[contracttype]
#[derive(Clone)]
pub struct Market {
    pub ym:       Address,
    pub pt:       Address,
    pub yt:       Address,
    pub pool:     Address,
    pub maturity: u64,
    pub vault:    Address,
}

#[contracttype]
#[derive(Clone)]
pub struct WasmHashes {
    pub pt:  BytesN<32>,
    pub yt:  BytesN<32>,
    pub ym:  BytesN<32>,
    pub amm: BytesN<32>,
}

pub trait FactoryTrait {
    fn __constructor(env: Env, admin: Address, wasm_hashes: WasmHashes);

    fn deploy_yield_manager(env: Env, vault: Address, vault_type: VaultType, maturity: u64) -> Address;
    fn deploy_pool(env: Env, vault: Address, vault_share_token: Address) -> Address;

    fn get_vaults(env: Env) -> Vec<Address>;
    fn get_markets(env: Env, vault: Address) -> Vec<Market>;

    fn get_current_yield_manager(env: Env, vault: Address) -> Option<Address>;
    fn get_current_pt_token(env: Env, vault: Address) -> Option<Address>;
    fn get_current_yt_token(env: Env, vault: Address) -> Option<Address>;
    fn get_current_pool(env: Env, vault: Address) -> Option<Address>;

    fn rollover_if_expired(env: Env, vault: Address, vault_type: VaultType, new_maturity: u64) -> bool;
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
            .deploy_v2(wasm_hashes.ym, (env.current_contract_address(), vault.clone(), vault_type, maturity));

        let pt_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(wasm_hashes.pt, (ym_addr.clone(), String::from_str(&env, "Principal Token"), String::from_str(&env, "PT"), 7u32));

        let yt_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(wasm_hashes.yt, (ym_addr.clone(), String::from_str(&env, "Yield Token"), String::from_str(&env, "YT"), 7u32));

        let ym_client = YieldManagerClient::new(&env, &ym_addr);
        ym_client.set_token_contracts(&pt_addr, &yt_addr);

        storage::register_vault(&env, &vault);
        storage::set_current_yield_manager(&env, &vault, &ym_addr);
        storage::set_current_pt_token(&env, &vault, &pt_addr);
        storage::set_current_yt_token(&env, &vault, &yt_addr);

        ym_addr
    }

    fn deploy_pool(env: Env, vault: Address, vault_share_token: Address) -> Address {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        let wasm_hashes = storage::get_wasm_hashes(&env);

        let ym_addr = storage::get_current_yield_manager(&env, &vault)
            .expect("No yield manager for vault");
        let ym_client = YieldManagerClient::new(&env, &ym_addr);

        let pool_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(wasm_hashes.amm, (ym_client.get_principal_token(), vault_share_token));

        storage::set_current_pool(&env, &vault, &pool_addr);

        storage::push_market(&env, &vault, Market {
            ym:       ym_addr,
            pt:       ym_client.get_principal_token(),
            yt:       ym_client.get_yield_token(),
            maturity: ym_client.get_maturity(),
            vault:    vault.clone(),
            pool:     pool_addr.clone(),
        });

        pool_addr
    }

    fn get_vaults(env: Env) -> Vec<Address> {
        storage::get_vaults(&env)
    }

    fn get_markets(env: Env, vault: Address) -> Vec<Market> {
        storage::get_markets(&env, &vault)
    }

    fn get_current_yield_manager(env: Env, vault: Address) -> Option<Address> {
        storage::get_current_yield_manager(&env, &vault)
    }

    fn get_current_pt_token(env: Env, vault: Address) -> Option<Address> {
        storage::get_current_pt_token(&env, &vault)
    }

    fn get_current_yt_token(env: Env, vault: Address) -> Option<Address> {
        storage::get_current_yt_token(&env, &vault)
    }

    fn get_current_pool(env: Env, vault: Address) -> Option<Address> {
        storage::get_current_pool(&env, &vault)
    }

    fn rollover_if_expired(env: Env, vault: Address, vault_type: VaultType, new_maturity: u64) -> bool {
        let current_ym = match storage::get_current_yield_manager(&env, &vault) {
            Some(ym) => ym,
            None => return false,
        };

        let ym_client = YieldManagerClient::new(&env, &current_ym);
        if env.ledger().timestamp() < ym_client.get_maturity() {
            return false;
        }

        Self::deploy_yield_manager(env.clone(), vault.clone(), vault_type, new_maturity);
        Self::deploy_pool(env, vault.clone(), vault);

        true
    }
}
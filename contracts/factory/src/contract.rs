use crate::events::{AdminChanged, ContractUpgraded, MarketCreated, WasmHashesUpdated};
use crate::storage;
use soroban_sdk::token::TokenClient;
use soroban_sdk::{contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, String};
use yield_manager_interface::{VaultType, YieldManagerClient};

#[contracttype]
#[derive(Clone)]
pub struct Market {
    pub name: String,
    pub ym: Address,
    pub pt: Address,
    pub yt: Address,
    pub pool: Address,
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

    fn create_market(
        env: Env,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
        scalar_root: i128,
        initial_anchor: i128,
        fee_rate_root: i128,
        last_implied_rate: i128,
    ) -> Market;

    fn get_market(env: Env, vault: Address, maturity: u64) -> Option<Market>;
    fn get_wasm_hashes(env: Env) -> WasmHashes;

    fn set_admin(env: Env, new_admin: Address);
    fn set_wasm_hashes(env: Env, new_hashes: WasmHashes);
    fn upgrade(env: Env, new_wasm_hashes: BytesN<32>);
}

#[contract]
pub struct Factory;

/// Upper bound on how far in the future a market may mature (10 years in
/// seconds). Generous for any realistic fixed-yield product; mainly catches
/// unit mistakes like passing milliseconds.
const MAX_MATURITY_HORIZON: u64 = 10 * 365 * 24 * 60 * 60;

fn next_salt(env: &Env) -> BytesN<32> {
    let counter = storage::get_salt_counter(env);
    storage::set_salt_counter(env, counter + 1);
    let mut buf = Bytes::new(env);
    buf.extend_from_array(&counter.to_be_bytes());
    env.crypto().keccak256(&buf).into()
}

const MAX_TOKEN_STRING_LEN: usize = 64;
// Builds "<prefix><vault-symbol>" or "<prefix><vault-symbol>-<maturity>" as
// a soroban_sdk::String. Manual byte-buffer construction since this is a
// #![no_std] contract with no alloc/format! available.
pub(crate) fn build_token_string(
    env: &Env,
    prefix: &str,
    vault_symbol: &String,
    maturity: Option<u64>,
) -> String {
    let mut buffer = [0u8; MAX_TOKEN_STRING_LEN];
    let mut position = 0usize;

    let prefix_bytes = prefix.as_bytes();
    buffer[position..position + prefix_bytes.len()].copy_from_slice(prefix_bytes);
    position += prefix_bytes.len();

    let symbol_len = vault_symbol.len() as usize;
    assert!(
        position + symbol_len <= MAX_TOKEN_STRING_LEN,
        "vault symbol too long for token name"
    );
    vault_symbol.copy_into_slice(&mut buffer[position..position + symbol_len]);
    position += symbol_len;

    if let Some(maturity) = maturity {
        buffer[position] = b'-';
        position += 1;

        let digits_start = position;
        if maturity == 0 {
            buffer[position] = b'0';
            position += 1;
        } else {
            let mut value = maturity;
            while value > 0 {
                assert!(
                    position < MAX_TOKEN_STRING_LEN,
                    "token name exceeds max length"
                );
                buffer[position] = b'0' + (value % 10) as u8;
                value /= 10;
                position += 1;
            }
            buffer[digits_start..position].reverse();
        }
    }

    String::from_bytes(env, &buffer[..position])
}

#[contractimpl]
impl FactoryTrait for Factory {
    fn __constructor(env: Env, admin: Address, wasm_hashes: WasmHashes) {
        storage::set_admin(&env, &admin);
        storage::set_wasm_hashes(&env, &wasm_hashes);
    }

    // TODO: support permissionless market creation — drop the admin gate (or add a
    // separate ungated entry point) so any user can create a market for any vault.
    // Needs: validation that `vault` is a real vault (not just any token contract),
    // spam/duplicate-market protection, and a story for who curates what the
    // frontend/indexer surfaces.
    fn create_market(
        env: Env,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
        scalar_root: i128,
        initial_anchor: i128,
        fee_rate_root: i128,
        last_implied_rate: i128,
    ) -> Market {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::extend_instance_ttl(&env);

        // Fail early with a clear message rather than deep inside the AMM
        // constructor. The upper bound guards against fat-fingered timestamps
        // (e.g. milliseconds instead of seconds) creating a market that can
        // never mature.
        let now = env.ledger().timestamp();
        assert!(maturity > now, "maturity must be in the future");
        assert!(
            maturity <= now + MAX_MATURITY_HORIZON,
            "maturity too far in the future"
        );

        // Markets are immutable once created: refuse a second market for the same
        // (vault, maturity) so an existing pool can never be overwritten/orphaned.
        // Different maturities on the same vault ARE allowed — they coexist as
        // independent, concurrently-tradeable markets, each keyed by its maturity.
        assert!(
            storage::get_market(&env, &vault, maturity).is_none(),
            "market already exists for this vault and maturity"
        );

        let ym_addr =
            Self::deploy_yield_manager_internal(env.clone(), vault.clone(), vault_type, maturity);
        let vault_symbol = soroban_sdk::token::TokenClient::new(&env, &vault).symbol();
        let market_name = build_token_string(&env, "", &vault_symbol, Some(maturity));
        let market = Self::deploy_pool_internal(
            env.clone(),
            vault.clone(),
            ym_addr,
            market_name,
            scalar_root,
            initial_anchor,
            fee_rate_root,
            last_implied_rate,
        );

        MarketCreated {
            vault: vault.clone(),
            market: market.clone(),
        }
        .publish(&env);

        market
    }

    fn set_admin(env: Env, new_admin: Address) {
        let old_admin = storage::get_admin(&env);
        old_admin.require_auth();
        storage::extend_instance_ttl(&env);

        storage::set_admin(&env, &new_admin);

        AdminChanged {
            old_admin,
            new_admin,
        }
        .publish(&env);
    }

    fn set_wasm_hashes(env: Env, new_hashes: WasmHashes) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::extend_instance_ttl(&env);

        let old_hashes = storage::get_wasm_hashes(&env);
        storage::set_wasm_hashes(&env, &new_hashes);

        WasmHashesUpdated {
            old_hashes,
            new_hashes,
        }
        .publish(&env);
    }

    fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::extend_instance_ttl(&env);

        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());

        ContractUpgraded { new_wasm_hash }.publish(&env);
    }

    fn get_market(env: Env, vault: Address, maturity: u64) -> Option<Market> {
        storage::extend_instance_ttl(&env);
        storage::get_market(&env, &vault, maturity)
    }

    fn get_wasm_hashes(env: Env) -> WasmHashes {
        storage::extend_instance_ttl(&env);
        storage::get_wasm_hashes(&env)
    }
}

impl Factory {
    fn deploy_yield_manager_internal(
        env: Env,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
    ) -> Address {
        let wasm_hashes = storage::get_wasm_hashes(&env);
        let vault_symbol = TokenClient::new(&env, &vault).symbol();

        let ym_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(
                wasm_hashes.ym,
                (
                    env.current_contract_address(),
                    vault.clone(),
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
                    build_token_string(&env, "PT-", &vault_symbol, Some(maturity)),
                    build_token_string(&env, "PT-", &vault_symbol, None),
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
                    build_token_string(&env, "YT-", &vault_symbol, Some(maturity)),
                    build_token_string(&env, "YT-", &vault_symbol, None),
                    7u32,
                ),
            );

        let ym_client = YieldManagerClient::new(&env, &ym_addr);
        ym_client.set_token_contracts(&pt_addr, &yt_addr);

        ym_addr
    }

    // Deploys the AMM pool for an already-deployed yield manager, records the
    // market under its (vault, maturity) key, and returns it. Takes `ym_addr`
    // directly rather than reading a per-vault "current" pointer, so a vault can
    // host several markets at different maturities without them clobbering
    // each other.
    fn deploy_pool_internal(
        env: Env,
        vault: Address,
        ym_addr: Address,
        name: String,
        scalar_root: i128,
        initial_anchor: i128,
        fee_rate_root: i128,
        last_implied_rate: i128,
    ) -> Market {
        let wasm_hashes = storage::get_wasm_hashes(&env);

        let ym_client = YieldManagerClient::new(&env, &ym_addr);
        let pt = ym_client.get_principal_token();
        let yt = ym_client.get_yield_token();
        let maturity = ym_client.get_maturity();

        let pool_addr = env
            .deployer()
            .with_current_contract(next_salt(&env))
            .deploy_v2(
                wasm_hashes.amm,
                (
                    pt.clone(),
                    // the vault contract is itself the share token the AMM
                    // trades against PT
                    vault.clone(),
                    maturity,
                    scalar_root,
                    initial_anchor,
                    fee_rate_root,
                    last_implied_rate,
                    ym_addr.clone(),
                ),
            );

        ym_client.set_pool(&pool_addr);

        let market = Market {
            name,
            ym: ym_addr,
            pt,
            yt,
            maturity,
            vault: vault.clone(),
            pool: pool_addr,
        };
        storage::set_market(&env, &vault, market.clone());

        market
    }
}

use crate::events::{ContractUpgraded, FeeConfigUpdated, MarketCreated, WasmHashesUpdated};
use crate::storage;
use soroban_sdk::token::TokenClient;
use soroban_sdk::{
    contract, contractimpl, contracttype, Address, Bytes, BytesN, Env, String,
};
use stellar_access::ownable::{self as ownable, Ownable};
use stellar_macros::only_owner;
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

/// Protocol fee configuration snapshotted into each market at creation.
/// Changing it never reaches live markets — their pools bake the values in
/// at construction and expose no setters.
#[contracttype]
#[derive(Clone)]
pub struct FeeConfig {
    /// Fee sink every new pool remits its reserve cut to.
    pub treasury: Address,
    /// Treasury's share of each trade's fee (1e7-scaled fraction of the fee,
    /// e.g. `1_000_000` = 10% of the fee; not a share of the trade).
    pub reserve_fee_rate: i128,
}

pub trait FactoryTrait {
    fn __constructor(env: Env, admin: Address, wasm_hashes: WasmHashes, fee_config: FeeConfig);

    fn create_market(
        env: Env,
        creator: Address,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
        current_apy: i128,
        apy_min: i128,
        apy_max: i128,
        fee_apy: i128,
    ) -> Market;

    fn get_market(env: Env, vault: Address, maturity: u64) -> Option<Market>;
    fn get_wasm_hashes(env: Env) -> WasmHashes;
    fn get_fee_config(env: Env) -> FeeConfig;

    // Ownership (get_owner / two-step transfer_ownership + accept_ownership /
    // renounce_ownership) comes from the stellar-access Ownable impl below.
    fn set_wasm_hashes(env: Env, new_hashes: WasmHashes);
    fn set_fee_config(env: Env, new_config: FeeConfig);
    fn upgrade(env: Env, new_wasm_hashes: BytesN<32>);
}

#[contract]
pub struct Factory;

/// Upper bound on how far in the future a market may mature (10 years in
/// seconds). Generous for any realistic fixed-yield product; mainly catches
/// unit mistakes like passing milliseconds.
const MAX_MATURITY_HORIZON: u64 = 10 * 365 * 24 * 60 * 60;

/// Cap on the treasury's share of the trading fee (1e7-scaled; 50% of the
/// fee). Mirrors the AMM constructor's own bound so a bad config fails here,
/// at config time, rather than on the next `create_market`.
const MAX_RESERVE_FEE_RATE: i128 = 5_000_000;

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
    fn __constructor(env: Env, owner: Address, wasm_hashes: WasmHashes, fee_config: FeeConfig) {
        assert!(
            fee_config.reserve_fee_rate >= 0 && fee_config.reserve_fee_rate <= MAX_RESERVE_FEE_RATE,
            "reserve_fee_rate out of range"
        );
        ownable::set_owner(&env, &owner);
        storage::set_wasm_hashes(&env, &wasm_hashes);
        storage::set_fee_config(&env, &fee_config);
    }

    /// Creates a market for `vault` at `maturity`. Permissionless: any address
    /// may create a market by authorizing as `creator` — the creator is
    /// published in `MarketCreated` so off-chain curation can distinguish who
    /// deployed what. Curve parameters are APY-denominated (1e7-scaled) and
    /// validated/derived by the AMM constructor.
    ///
    /// The vault is taken on trust: there is no on-chain way to prove it is an
    /// honest vault, and a malicious one can only harm users who opt into its
    /// market — every market gets its own YM/PT/YT/pool touching only its own
    /// vault. Which markets are surfaced to users is an off-chain concern.
    fn create_market(
        env: Env,
        creator: Address,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
        current_apy: i128,
        apy_min: i128,
        apy_max: i128,
        fee_apy: i128,
    ) -> Market {
        creator.require_auth();
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
            current_apy,
            apy_min,
            apy_max,
            fee_apy,
        );

        MarketCreated {
            creator,
            vault: vault.clone(),
            market: market.clone(),
        }
        .publish(&env);

        market
    }

    #[only_owner]
    fn set_wasm_hashes(env: Env, new_hashes: WasmHashes) {
        storage::extend_instance_ttl(&env);

        let old_hashes = storage::get_wasm_hashes(&env);
        storage::set_wasm_hashes(&env, &new_hashes);

        WasmHashesUpdated {
            old_hashes,
            new_hashes,
        }
        .publish(&env);
    }

    /// (Owner only) Updates the fee config for markets created *afterward*.
    /// Live markets are untouched: their pools snapshotted the config at
    /// creation and have no setters.
    #[only_owner]
    fn set_fee_config(env: Env, new_config: FeeConfig) {
        storage::extend_instance_ttl(&env);

        assert!(
            new_config.reserve_fee_rate >= 0 && new_config.reserve_fee_rate <= MAX_RESERVE_FEE_RATE,
            "reserve_fee_rate out of range"
        );

        let old_config = storage::get_fee_config(&env);
        storage::set_fee_config(&env, &new_config);

        FeeConfigUpdated {
            old_config,
            new_config,
        }
        .publish(&env);
    }

    #[only_owner]
    fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
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

    fn get_fee_config(env: Env) -> FeeConfig {
        storage::extend_instance_ttl(&env);
        storage::get_fee_config(&env)
    }
}

#[contractimpl(contracttrait)]
impl Ownable for Factory {}

impl Factory {
    fn deploy_yield_manager_internal(
        env: Env,
        vault: Address,
        vault_type: VaultType,
        maturity: u64,
    ) -> Address {
        let wasm_hashes = storage::get_wasm_hashes(&env);
        let vault_symbol = TokenClient::new(&env, &vault).symbol();

        // Same snapshot as the pool: the YM keeps this treasury forever.
        let treasury = storage::get_fee_config(&env).treasury;

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
                    treasury,
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
        current_apy: i128,
        apy_min: i128,
        apy_max: i128,
        fee_apy: i128,
    ) -> Market {
        let wasm_hashes = storage::get_wasm_hashes(&env);

        let ym_client = YieldManagerClient::new(&env, &ym_addr);
        let pt = ym_client.get_principal_token();
        let yt = ym_client.get_yield_token();
        let maturity = ym_client.get_maturity();

        // Snapshot the current fee config into the pool: the market keeps
        // these values forever, so later config changes are prospective only.
        let fee_config = storage::get_fee_config(&env);

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
                    current_apy,
                    apy_min,
                    apy_max,
                    fee_apy,
                    ym_addr.clone(),
                    fee_config.treasury,
                    fee_config.reserve_fee_rate,
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

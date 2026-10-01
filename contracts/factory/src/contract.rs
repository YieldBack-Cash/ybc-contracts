use crate::events::{ContractUpgraded, FeeConfigUpdated, MarketCreated, WasmHashesUpdated};
use crate::storage;
use factory_interface::{FactoryError, FactoryTrait, FeeConfig, Market, WasmHashes};
use soroban_sdk::token::TokenClient;
use soroban_sdk::{contract, contractimpl, panic_with_error, Address, Bytes, BytesN, Env, String};
use stellar_access::ownable::{self as ownable, Ownable};
use stellar_macros::only_owner;
use yield_manager_interface::YieldManagerClient;

#[contract]
pub struct Factory;

/// 10 years in seconds; see `FactoryError::MaturityTooFar`.
const MAX_MATURITY_HORIZON: u64 = 10 * 365 * 24 * 60 * 60;

// The cap on the treasury's share of the trading fee is the AMM
// constructor's own bound, checked here as well so a bad config fails at
// config time rather than on the next `create_market`.
use ybc_common::fees::MAX_RESERVE_FEE_RATE;

fn next_salt(env: &Env) -> BytesN<32> {
    let counter = storage::get_salt_counter(env);
    storage::set_salt_counter(env, counter + 1);
    let mut buf = Bytes::new(env);
    buf.extend_from_array(&counter.to_be_bytes());
    env.crypto().keccak256(&buf).into()
}

/// Stack buffer for a token name or symbol. Wallets truncate long symbols
/// anyway, and 64 leaves at least 51 bytes for a vault symbol after the
/// longest prefix and the date.
const MAX_TOKEN_STRING_LEN: usize = 64;

/// The UTC calendar date of a Unix timestamp as `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days`, integer-only, valid for every date a
/// maturity can reasonably be. A `no_std` contract has no date library.
pub(crate) fn civil_date(timestamp: u64) -> (u64, u64, u64) {
    let days = timestamp / 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

const MONTHS: [&[u8; 3]; 12] = [
    b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV", b"DEC",
];

/// Builds "<prefix><vault-symbol>-DDMMMYYYY" as a soroban_sdk::String, e.g.
/// `PT-bvXLM-23DEC2026`.
///
/// The maturity is rendered as a calendar date rather than a Unix timestamp
/// because the string is what a wallet shows: `1797984000` tells a holder
/// nothing, `23DEC2026` tells them when their PT settles. Fixed width,
/// uppercase month, no separators inside the date (the convention Pendle
/// uses), so two maturities on one vault are distinguishable by symbol alone.
///
/// Manual byte-buffer construction since this is a `#![no_std]` contract with
/// no alloc/format! available.
pub(crate) fn build_token_string(
    env: &Env,
    prefix: &str,
    vault_symbol: &String,
    maturity: u64,
) -> String {
    let mut buffer = [0u8; MAX_TOKEN_STRING_LEN];
    let mut position = 0usize;

    let prefix_bytes = prefix.as_bytes();
    buffer[position..position + prefix_bytes.len()].copy_from_slice(prefix_bytes);
    position += prefix_bytes.len();

    let symbol_len = vault_symbol.len() as usize;
    // "-DDMMMYYYY" is 10 bytes.
    if position + symbol_len + 10 > MAX_TOKEN_STRING_LEN {
        panic_with_error!(env, FactoryError::VaultSymbolTooLong);
    }
    vault_symbol.copy_into_slice(&mut buffer[position..position + symbol_len]);
    position += symbol_len;

    {
        let (year, month, day) = civil_date(maturity);
        if year > 9999 {
            panic_with_error!(env, FactoryError::MaturityYearOutOfRange);
        }

        buffer[position] = b'-';
        buffer[position + 1] = b'0' + (day / 10) as u8;
        buffer[position + 2] = b'0' + (day % 10) as u8;
        buffer[position + 3..position + 6].copy_from_slice(MONTHS[(month - 1) as usize]);
        buffer[position + 6] = b'0' + (year / 1000) as u8;
        buffer[position + 7] = b'0' + (year / 100 % 10) as u8;
        buffer[position + 8] = b'0' + (year / 10 % 10) as u8;
        buffer[position + 9] = b'0' + (year % 10) as u8;
        position += 10;
    }

    String::from_bytes(env, &buffer[..position])
}

#[contractimpl]
impl FactoryTrait for Factory {
    fn __constructor(env: Env, owner: Address, wasm_hashes: WasmHashes, fee_config: FeeConfig) {
        if !(0..=MAX_RESERVE_FEE_RATE).contains(&fee_config.reserve_fee_rate) {
            panic_with_error!(&env, FactoryError::ReserveFeeRateOutOfRange);
        }
        ownable::set_owner(&env, &owner);
        storage::set_wasm_hashes(&env, &wasm_hashes);
        storage::set_fee_config(&env, &fee_config);
    }

    fn create_market(
        env: Env,
        creator: Address,
        vault: Address,
        maturity: u64,
        current_apy: i128,
        apy_min: i128,
        apy_max: i128,
        fee_apy: i128,
    ) -> Market {
        creator.require_auth();
        storage::extend_instance_ttl(&env);

        // Fail on our own codes before the AMM constructor gets a chance to.
        let now = env.ledger().timestamp();
        if maturity <= now {
            panic_with_error!(&env, FactoryError::MaturityNotInFuture);
        }
        if maturity > now + MAX_MATURITY_HORIZON {
            panic_with_error!(&env, FactoryError::MaturityTooFar);
        }

        // Markets are never replaced (see `storage::get_market`); other maturities
        // on the same vault are independent markets.
        if storage::get_market(&env, &vault, maturity).is_some() {
            panic_with_error!(&env, FactoryError::MarketAlreadyExists);
        }

        // Read once and threaded through: the hashes every contract of the
        // market is deployed from, the fee config it keeps forever (later
        // changes are prospective only), and the vault's symbol, which names
        // the market and both tokens.
        let wasm_hashes = storage::get_wasm_hashes(&env);
        let fee_config = storage::get_fee_config(&env);
        let vault_symbol = TokenClient::new(&env, &vault).symbol();

        let (ym, pt, yt) = Self::deploy_yield_manager_internal(
            &env,
            &wasm_hashes,
            &vault,
            &vault_symbol,
            maturity,
            &fee_config.treasury,
        );
        let pool = Self::deploy_pool_internal(
            &env,
            &wasm_hashes,
            &fee_config,
            &vault,
            &ym,
            &pt,
            maturity,
            CurveParams {
                current_apy,
                apy_min,
                apy_max,
                fee_apy,
            },
        );

        let market = Market {
            name: build_token_string(&env, "", &vault_symbol, maturity),
            ym,
            pt,
            yt,
            pool,
            maturity,
            vault: vault.clone(),
        };
        storage::set_market(&env, &vault, market.clone());

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

    #[only_owner]
    fn set_fee_config(env: Env, new_config: FeeConfig) {
        storage::extend_instance_ttl(&env);

        if !(0..=MAX_RESERVE_FEE_RATE).contains(&new_config.reserve_fee_rate) {
            panic_with_error!(&env, FactoryError::ReserveFeeRateOutOfRange);
        }

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

/// The creator's curve parameters, as `create_market` received them.
struct CurveParams {
    current_apy: i128,
    apy_min: i128,
    apy_max: i128,
    fee_apy: i128,
}

impl Factory {
    /// Deploys the yield manager and its two tokens and introduces them to
    /// each other. Returns `(ym, pt, yt)`.
    fn deploy_yield_manager_internal(
        env: &Env,
        wasm_hashes: &WasmHashes,
        vault: &Address,
        vault_symbol: &String,
        maturity: u64,
        treasury: &Address,
    ) -> (Address, Address, Address) {
        let ym = env
            .deployer()
            .with_current_contract(next_salt(env))
            .deploy_v2(
                wasm_hashes.ym.clone(),
                (
                    env.current_contract_address(),
                    vault.clone(),
                    maturity,
                    // The YM keeps this treasury forever.
                    treasury.clone(),
                ),
            );

        // A token's name and symbol are the same string.
        let pt_name = build_token_string(env, "PT-", vault_symbol, maturity);
        let pt = env
            .deployer()
            .with_current_contract(next_salt(env))
            .deploy_v2(
                wasm_hashes.pt.clone(),
                (ym.clone(), pt_name.clone(), pt_name, 7u32),
            );

        let yt_name = build_token_string(env, "YT-", vault_symbol, maturity);
        let yt = env
            .deployer()
            .with_current_contract(next_salt(env))
            .deploy_v2(
                wasm_hashes.yt.clone(),
                (ym.clone(), yt_name.clone(), yt_name, 7u32),
            );

        YieldManagerClient::new(env, &ym).set_token_contracts(&pt, &yt);

        (ym, pt, yt)
    }

    /// Deploys the AMM pool for a yield manager just deployed and registers it
    /// there. Everything it needs is passed in, so nothing is asked back of
    /// the contracts this factory created a moment ago.
    #[allow(clippy::too_many_arguments)]
    fn deploy_pool_internal(
        env: &Env,
        wasm_hashes: &WasmHashes,
        fee_config: &FeeConfig,
        vault: &Address,
        ym: &Address,
        pt: &Address,
        maturity: u64,
        curve: CurveParams,
    ) -> Address {
        let pool = env
            .deployer()
            .with_current_contract(next_salt(env))
            .deploy_v2(
                wasm_hashes.amm.clone(),
                (
                    pt.clone(),
                    // The vault contract is itself the share token the AMM trades against PT.
                    vault.clone(),
                    maturity,
                    curve.current_apy,
                    curve.apy_min,
                    curve.apy_max,
                    curve.fee_apy,
                    ym.clone(),
                    fee_config.treasury.clone(),
                    fee_config.reserve_fee_rate,
                ),
            );

        YieldManagerClient::new(env, ym).set_pool(&pool);

        pool
    }
}

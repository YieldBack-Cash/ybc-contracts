#![no_std]

//! The factory's public surface, declared once.
//!
//! The factory implements [`FactoryTrait`]; the router and the test harnesses
//! decode its return values into these same types instead of keeping copies
//! that must be edited in step. Soroban encodes `contracttype` values by
//! field name, so a copy that drifted would fail at the call boundary, not at
//! compile time; sharing the declaration makes it a compile error instead.

use soroban_sdk::{contractclient, contracttype, Address, BytesN, Env, String};
use yield_manager_interface::VaultType;

/// One market: the contracts the factory deployed for a (vault, maturity).
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

/// The WASM hashes the factory deploys markets from.
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

#[contractclient(name = "FactoryClient")]
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
    // renounce_ownership) comes from the factory's stellar-access Ownable impl.
    fn set_wasm_hashes(env: Env, new_hashes: WasmHashes);
    fn set_fee_config(env: Env, new_config: FeeConfig);
    fn upgrade(env: Env, new_wasm_hashes: BytesN<32>);
}

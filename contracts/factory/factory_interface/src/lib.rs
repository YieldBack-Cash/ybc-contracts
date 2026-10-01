#![no_std]

//! The factory's public surface, declared once.
//!
//! The factory implements [`FactoryTrait`]; the router and the test harnesses
//! decode its return values into these same types instead of keeping copies
//! that must be edited in step. Soroban encodes `contracttype` values by
//! field name, so a copy that drifted would fail at the call boundary, not at
//! compile time; sharing the declaration makes it a compile error instead.

use soroban_sdk::{contractclient, contracterror, contracttype, Address, BytesN, Env, String};

/// Every way a factory call can fail on its own checks. Ownership failures
/// carry OpenZeppelin's codes (2100 and up in stellar-access 0.7.2); failures
/// while constructing a market's contracts carry those contracts' own.
///
/// Codes are part of the client-facing API: append new variants, never
/// renumber existing ones.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum FactoryError {
    /// The treasury's share of the fee is negative or above the cap.
    ReserveFeeRateOutOfRange = 1,
    /// `maturity` is not after the current ledger time.
    MaturityNotInFuture = 2,
    /// `maturity` is more than ten years out: most likely milliseconds
    /// passed for seconds.
    MaturityTooFar = 3,
    /// A market for this (vault, maturity) exists; markets are never replaced.
    MarketAlreadyExists = 4,
    /// The vault's symbol leaves no room for the prefix and the date.
    VaultSymbolTooLong = 5,
    /// The maturity's year does not fit four digits. Unreachable while
    /// `MaturityTooFar` holds; kept as a bounds guard for the byte buffer.
    MaturityYearOutOfRange = 6,
}

/// One market: the contracts the factory deployed for a (vault, maturity).
#[contracttype]
#[derive(Clone)]
pub struct Market {
    /// `<vault-symbol>-DDMMMYYYY`, e.g. `bvXLM-23DEC2026`; the PT and YT
    /// names prefix it.
    pub name: String,
    pub ym: Address,
    pub pt: Address,
    pub yt: Address,
    pub pool: Address,
    pub maturity: u64,
    /// The vault share token; also the pool's non-PT reserve.
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
    fn __constructor(env: Env, owner: Address, wasm_hashes: WasmHashes, fee_config: FeeConfig);

    /// Creates a market for `vault` at `maturity`. Permissionless: any address
    /// may create a market by authorizing as `creator`, who is published in
    /// `MarketCreated` so off-chain curation can tell who deployed what.
    /// Curve parameters are APY-denominated (1e7-scaled) and validated by the
    /// AMM constructor.
    ///
    /// The vault is taken on trust: there is no on-chain way to prove it is an
    /// honest vault, and a malicious one can only harm users who opt into its
    /// market — every market gets its own YM/PT/YT/pool touching only its own
    /// vault. Which markets are surfaced to users is an off-chain concern.
    fn create_market(
        env: Env,
        creator: Address,
        vault: Address,
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

    /// (Owner only) Sets the hashes markets created *afterward* deploy from.
    /// Live markets keep the code they were created with.
    fn set_wasm_hashes(env: Env, new_hashes: WasmHashes);
    /// (Owner only) Updates the fee config for markets created *afterward*.
    /// Live markets are untouched: their pools snapshotted the config at
    /// creation and have no setters.
    fn set_fee_config(env: Env, new_config: FeeConfig);
    /// (Owner only) Replaces the factory's own code.
    fn upgrade(env: Env, new_wasm_hash: BytesN<32>);
}

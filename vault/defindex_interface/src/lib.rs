#![no_std]

use soroban_sdk::{contractclient, Address, Env, Vec};

/// Trait defining the interface for the Defindex Vault contract.
/// This trait is used to generate the contract client for type-safe cross-contract calls.
#[contractclient(name = "DefindexVaultContractClient")]
pub trait DefindexVaultTrait {
    fn __constructor(e: Env, asset: Address, decimals_offset: u32, strategy: Address);
    fn get_asset_amounts_per_shares(e: Env, vault_shares: i128) -> Vec<i128>;
}


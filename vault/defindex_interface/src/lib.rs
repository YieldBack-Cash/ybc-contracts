#![no_std]

use soroban_sdk::{contractclient, Address, Env};

/// Trait defining the interface for the Defindex Vault contract.
/// This trait is used to generate the contract client for type-safe cross-contract calls.
#[contractclient(name = "DefindexVaultContractClient")]
pub trait DefindexVaultTrait {
    fn __constructor(e: Env, asset: Address, decimals_offset: u32, strategy: Address);
    fn exchange_rate(e: Env) -> i128;
}



// Calculates the corresponding amounts of each asset per a given number of vault shares.
// This function extends the contract's time-to-live and calculates how much of each asset corresponds
// per the provided number of vault shares (`vault_shares`). It provides proportional allocations for each asset
// in the vault relative to the specified shares.
//
// # Arguments
// * `e` - The current environment reference.
// * `vault_shares` - The number of vault shares for which the corresponding asset amounts are calculated.
//
// # Returns
// * `Result<Vec<i128>, ContractError>` - A vector of asset amounts corresponding to the vault shares, where each index
//   matches the asset index in the vault's asset list. Returns ContractError if calculation fails.
// fn get_asset_amounts_per_shares(
//     e: Env,
//     vault_shares: i128,
// ) -> Result<Vec<i128>, ContractError>;

#![no_std]

use soroban_sdk::{contractclient, Address, Env};

/// Trait defining the interface for the Vault contract.
/// This trait is used to generate the VaultContractClient for type-safe cross-contract calls.
#[contractclient(name = "VaultContractClient")] // TODO: add another interface for defindex vaults
pub trait VaultTrait {
    fn __constructor(e: Env, asset: Address, decimals_offset: u32, strategy: Address);
    fn exchange_rate(e: Env) -> i128;
}

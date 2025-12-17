#![no_std]

use soroban_sdk::{contractclient, Address, Env};

/// Trait defining the interface for the Vault contract.
/// This trait is used to generate the VaultContractClient for type-safe cross-contract calls.
#[contractclient(name = "VaultContractClient")]
pub trait VaultTrait {
    fn __constructor(e: Env, asset: Address, decimals_offset: u32, strategy: Address);
    fn convert_to_assets(e: &Env, shares: i128) -> i128;
    fn deposit(
        e: &Env,
        assets: i128,
        receiver: Address,
        from: Address,
        operator: Address,
    ) -> i128;
}

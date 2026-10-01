//! A SEP-41 share token with a settable `convert_to_assets` rate and nothing
//! else: enough for the factory, yield-manager and token tests, not for the
//! zaps, which need `query_asset`, `deposit` and `redeem` (use `standard_vault`).
#![no_std]

mod contract;
mod storage;

pub use contract::{MockVault, MockVaultClient, MockVaultTrait};

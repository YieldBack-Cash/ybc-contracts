#![no_std]

#[cfg(test)]
extern crate std;

mod contract;
mod curve;
mod events;
mod math;
mod storage;
mod transfers;
mod vault;

#[cfg(any(test, feature = "testutils"))]
pub mod fuzz_harness;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "testutils"))]
pub use amm_interface::AmmClient;
pub use amm_interface::AmmInterface;
pub use contract::LiquidityPool;
#[cfg(any(test, feature = "testutils"))]
pub use contract::LiquidityPoolClient;

use soroban_sdk::contractmeta;

contractmeta!(key = "Description", val = "YBC AMM Liquidity Pool");

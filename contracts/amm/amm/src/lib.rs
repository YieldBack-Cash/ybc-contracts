#![no_std]

#[cfg(test)]
extern crate std;

mod contract;
mod curve;
mod events;
mod math;
mod transfers;
mod storage;
mod vault;

#[cfg(any(test, feature = "testutils"))]
pub mod fuzz_harness;

#[cfg(test)]
mod tests;

pub use amm_interface::AmmInterface;
pub use contract::LiquidityPool;
#[cfg(any(test, feature = "testutils"))]
pub use contract::LiquidityPoolClient;
#[cfg(any(test, feature = "testutils"))]
pub use amm_interface::AmmClient;

pub use math::{seconds_to_years, implied_rate_to_exchange_rate, ln_fp};

use soroban_sdk::contractmeta;

contractmeta!(key = "Description", val = "YBC AMM Liquidity Pool");
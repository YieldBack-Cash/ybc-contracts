#![no_std]

mod contract;
mod curve;
mod math;
mod transfers;
mod storage;
mod vault;

#[cfg(test)]
mod tests;

pub use contract::{AmmInterface, LiquidityPool};
#[cfg(any(test, feature = "testutils"))]
pub use contract::{AmmClient, LiquidityPoolClient};

pub use math::{seconds_to_years, implied_rate_to_exchange_rate, ln_fp};

use soroban_sdk::contractmeta;

contractmeta!(key = "Description", val = "YBC AMM Liquidity Pool");
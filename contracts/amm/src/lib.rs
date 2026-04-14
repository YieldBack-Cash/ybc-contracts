#![no_std]

mod contract;
mod curve;
mod math;
mod transfers;
mod storage;
mod vault;

#[cfg(test)]
mod test;

pub use math::{seconds_to_years, implied_rate_to_exchange_rate, ln_fp};

use soroban_sdk::contractmeta;

contractmeta!(key = "Description", val = "YBC AMM Liquidity Pool");
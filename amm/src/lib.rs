#![no_std]

mod contract;
mod storage;
mod test;

pub use contract::LiquidityPool;

use soroban_sdk::contractmeta;

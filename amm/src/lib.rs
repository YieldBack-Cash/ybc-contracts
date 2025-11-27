#![no_std]

mod contract;
mod storage;
mod test;

pub use contract::LiquidityPool;

use soroban_sdk::contractmeta;

// Metadata that is added on to the WASM custom section
contractmeta!(
    key = "Description",
    val = "Constant product AMM"
);

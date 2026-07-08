#![no_std]

mod contract;
mod storage;

pub use contract::RouterContract;

use soroban_sdk::contractmeta;

contractmeta!(key = "Description", val = "YBC Router");
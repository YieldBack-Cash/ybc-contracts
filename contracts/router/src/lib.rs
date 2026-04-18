#![no_std]

mod contract;

pub use contract::RouterContract;

use soroban_sdk::contractmeta;

contractmeta!(key = "Description", val = "YBC Router");
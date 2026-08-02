#![no_std]

mod contract;
mod events;
mod storage;

pub use contract::{RouterClient, RouterContract, RouterInterface};

use soroban_sdk::contractmeta;

contractmeta!(key = "Description", val = "YBC Router");
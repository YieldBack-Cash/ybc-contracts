#![no_std]

mod contract;
mod errors;
mod events;
mod storage;

pub use contract::{RouterClient, RouterContract, RouterInterface};
pub use errors::RouterError;

use soroban_sdk::contractmeta;

contractmeta!(key = "Description", val = "YBC Router");
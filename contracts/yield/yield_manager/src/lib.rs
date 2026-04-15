#![no_std]

mod storage;
mod contract;

#[cfg(test)]
mod tests;

pub use contract::YieldManager;
pub use yield_manager_interface::{YieldManagerTrait, VaultType};

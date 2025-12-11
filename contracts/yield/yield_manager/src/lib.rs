#![no_std]

mod storage;
mod contract;

#[cfg(test)]
mod test;

pub use contract::YieldManager;
pub use yield_manager_interface::{YieldManagerTrait, VaultType};

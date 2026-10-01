#![no_std]

mod contract;
mod events;
mod storage;

pub use contract::{Factory, FactoryClient};
pub use factory_interface::{FactoryError, FactoryTrait, FeeConfig, Market, WasmHashes};

#[cfg(test)]
mod test;

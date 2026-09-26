#![no_std]

mod storage;
mod contract;
mod events;

pub use contract::{Factory, FactoryClient};
pub use factory_interface::{FactoryTrait, FeeConfig, Market, WasmHashes};

#[cfg(test)]
mod test;

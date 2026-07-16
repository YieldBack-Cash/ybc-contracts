#![no_std]

mod storage;
mod contract;

pub use contract::{Factory, FactoryClient, FactoryTrait, Market, WasmHashes};

#[cfg(test)]
mod test;

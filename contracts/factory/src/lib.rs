#![no_std]

mod storage;
mod contract;

pub use contract::{Factory, FactoryClient, FactoryTrait, WasmHashes};

#[cfg(test)]
mod test;

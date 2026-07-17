#![no_std]

mod storage;
mod contract;
mod events;

pub use contract::{Factory, FactoryClient, FactoryTrait, Market, WasmHashes};

#[cfg(test)]
mod test;

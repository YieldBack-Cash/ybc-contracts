#![no_std]

mod contract;
mod storage;

#[cfg(test)]
mod tests;

pub use contract::YieldToken;
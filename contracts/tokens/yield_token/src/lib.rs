#![no_std]

mod contract;
mod math;
mod storage;

#[cfg(test)]
mod tests;

pub use contract::YieldToken;
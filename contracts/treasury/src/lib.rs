#![no_std]

mod contract;
mod events;
mod storage;

pub use contract::{Treasury, TreasuryClient, TreasuryTrait};

#[cfg(test)]
mod test;
#![no_std]

mod contract;
mod events;
mod storage;

pub use contract::{Treasury, TreasuryClient, TreasuryError, TreasuryTrait};

#[cfg(test)]
mod test;

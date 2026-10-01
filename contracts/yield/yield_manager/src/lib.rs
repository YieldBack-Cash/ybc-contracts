//! Yield manager: custodies vault shares, mints and burns PT+YT against them,
//! high-water-marks the exchange rate and settles positions at face value
//! after maturity.
#![no_std]

mod contract;
mod events;
mod storage;

#[cfg(test)]
mod tests;

pub use contract::YieldManager;
pub use yield_manager_interface::YieldManagerTrait;

#![no_std]

#[cfg(feature = "contract")]
mod contract;
#[cfg(feature = "contract")]
mod storage;

#[cfg(all(test, feature = "contract"))]
mod test;

#[cfg(feature = "contract")]
pub use contract::{YieldToken, YieldTokenTrait};

#[cfg(feature = "contract")]
pub use contract::YieldTokenClient;

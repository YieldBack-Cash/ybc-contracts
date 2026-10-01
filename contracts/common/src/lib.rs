#![no_std]

//! What every YBC core contract shares, and nothing else: the ledger TTL
//! policy, the fixed-point scale, and the one bound two contracts both have
//! to enforce. Kept deliberately tiny; a bug here is a bug in every contract.

/// The fixed-point scale of every rate and ratio in the protocol.
pub mod scale {
    /// 1.0, to seven decimal places.
    pub const SCALAR_7: i128 = 10_000_000;
}

/// Bounds on protocol fees.
pub mod fees {
    /// Cap on the treasury's share of the trading fee (1e7-scaled fraction of
    /// the fee, not of the trade; 50%). Above it the LP cut stops being worth
    /// providing liquidity for. The factory checks it when the fee config is
    /// set and the AMM when a pool is constructed; they have to agree.
    pub const MAX_RESERVE_FEE_RATE: i128 = 5_000_000;
}

/// The TTL policy every core contract applies to its own storage.
///
/// Instance storage (a contract's own config) is bumped a week ahead once it
/// has less than six days left; per-user persistent entries a month ahead
/// once they have less than twenty-nine days left. Every entry point calls
/// [`extend_instance_ttl`] first: an expired instance bricks the contract,
/// not one user's data.
pub mod ttl {
    use soroban_sdk::{Env, IntoVal, Val};

    /// Ledgers per day at the 5-second ledger close (86_400 / 5).
    pub const DAY_IN_LEDGERS: u32 = 17280;

    /// Instance entries are bumped to seven days ahead…
    pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
    /// …once fewer than six days remain.
    pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

    /// Persistent entries are bumped to thirty days ahead…
    pub const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
    /// …once fewer than twenty-nine days remain.
    pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

    /// Keeps the contract's instance alive. Call once per entry point.
    pub fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Keeps one persistent entry alive; call after reading or writing it.
    pub fn extend_persistent_ttl<K: IntoVal<Env, Val>>(env: &Env, key: &K) {
        env.storage().persistent().extend_ttl(
            key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }
}

#![no_std]

//! What every YBC core contract shares, and nothing else: the ledger TTL
//! policy. Kept deliberately tiny; a bug here is a bug in every contract.

/// The TTL policy every core contract applies to its own storage.
///
/// Instance storage (a contract's own config) is bumped a week ahead once it
/// has less than six days left; per-user persistent entries a month ahead
/// once they have less than twenty-nine days left. Every entry point calls
/// [`extend_instance_ttl`] first: an expired instance bricks the contract,
/// not one user's data.
pub mod ttl {
    use soroban_sdk::{Env, IntoVal, Val};

    pub const DAY_IN_LEDGERS: u32 = 17280;

    pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
    pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

    pub const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
    pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - DAY_IN_LEDGERS;

    /// Keeps the contract's instance alive. Call once per entry point.
    pub fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    /// Keeps one persistent entry alive; call after reading or writing it.
    pub fn extend_persistent_ttl<K: IntoVal<Env, Val>>(env: &Env, key: &K) {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_LIFETIME_THRESHOLD, PERSISTENT_BUMP_AMOUNT);
    }
}

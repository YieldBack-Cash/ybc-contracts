use soroban_sdk::Env;

pub const DAY_IN_LEDGERS: u32 = 17280;
pub const INSTANCE_BUMP_AMOUNT: u32 = 7 * DAY_IN_LEDGERS;
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

/// Extends the instance TTL (owner entry, managed by stellar-access Ownable).
/// Call once per entrypoint. Token balances live in each token contract's
/// storage, so an expired treasury instance strands nothing permanently — but
/// withdrawals stall until the entry is restored, so keep it alive.
pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}
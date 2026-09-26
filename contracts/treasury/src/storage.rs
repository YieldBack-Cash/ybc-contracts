/// Instance TTL (owner entry, managed by stellar-access Ownable). Call once
/// per entrypoint. Token balances live in each token contract's storage, so an
/// expired treasury instance strands nothing permanently -- but withdrawals
/// stall until the entry is restored, so keep it alive.
pub use ybc_common::ttl::extend_instance_ttl;

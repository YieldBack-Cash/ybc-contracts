#![no_std]

use soroban_sdk::{contractclient, Address, Env};

/// The slice of SEP-56 (`TokenizedVault`) the protocol calls on a vault.
///
/// The protocol calls nothing else on a vault, so any compliant vault can
/// back a market without a per-vault adapter. SEP-56 declares
/// `TokenizedVault: TokenInterface`, so the share token is itself SEP-41 —
/// which is how the yield manager custodies shares and how the AMM holds them
/// as a reserve.
///
/// This is a strict subset, and deliberately an exact one: every method here
/// is called somewhere, and nothing that is called is missing. Declaring a
/// method the protocol never invokes reads as a requirement, and a vault that
/// omits it looks incompatible when it is not.
///
/// SEP-56 also declares `mint`, `withdraw`, `convert_to_shares`, `total_supply`,
/// `total_assets`, the four `max_*` and the four `preview_*` functions. Beyond
/// simply being uncalled, two families are worth naming:
///
///   * `preview_*` would be the natural way to quote a trade, but a quote from
///     the vault is only advisory — settlement here measures actual balance
///     deltas and enforces the caller's min/max bound, which is strictly more
///     trustworthy. Depending on them would also mean passing against a fully
///     compliant mock while failing against a production vault that omits them.
///     Frontends should call them directly for display.
///   * `max_*` cannot be relied on to detect a vault that has no liquidity
///     right now: the reference implementations just convert the owner's
///     balance and say nothing about availability.
///
/// Two properties the protocol depends on that SEP-56 does *not* guarantee —
/// see docs/SECURITY.md:
///   * share value may fall (the yield manager assumes a non-decreasing rate),
///   * fees are out of scope, so `convert_to_assets` may overstate what
///     `redeem` actually pays out.
#[contractclient(name = "VaultContractClient")]
pub trait VaultTrait {
    /// Address of the underlying asset the vault holds.
    ///
    /// Read live on every call rather than snapshotted into the factory's
    /// market record: there is nothing to migrate and nothing that can go
    /// stale. Callers must resolve it once per invocation and reuse the result,
    /// so a vault cannot name one asset on the way in and another on the way
    /// out of the same transaction.
    fn query_asset(e: &Env) -> Address;

    /// Assets per share. Read only by the yield manager, which high-water-marks
    /// it into the exchange rate the rest of the protocol prices and settles
    /// against — the AMM reads that rate from the yield manager rather than
    /// calling this itself.
    fn convert_to_assets(e: &Env, shares: i128) -> i128;

    /// Asset-denominated entry. Shares go to `receiver`; the router passes the
    /// user for `receiver`, `from` and `operator` alike so it takes no custody,
    /// and the yield manager names itself as `receiver` so shares land in its
    /// custody directly.
    fn deposit(e: &Env, assets: i128, receiver: Address, from: Address, operator: Address) -> i128;

    /// Share-denominated exit. Preferred over SEP-56's asset-denominated
    /// `withdraw` for unwinding a position: "convert exactly these shares"
    /// needs no preview round-trip and leaves no dust behind.
    fn redeem(e: &Env, shares: i128, receiver: Address, owner: Address, operator: Address) -> i128;
}

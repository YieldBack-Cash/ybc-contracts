#![no_std]
use soroban_sdk::{contractclient, Address, Env, String};

/// What the yield token exposes beyond SEP-41.
///
/// The token surface itself (balance, transfer, allowances, `burn`,
/// `burn_from`, `total_supply`, metadata) is OpenZeppelin's `Base`, exposed
/// through its `FungibleToken` and `FungibleBurnable` traits with the accrual
/// hook applied before every balance movement; callers reach it with
/// `soroban_sdk::token::Client`.
#[contractclient(name = "YieldTokenClient")]
pub trait YieldTokenTrait {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32);

    /// Admin (yield manager) only. `exchange_rate` is the rate the YM already
    /// holds, so the token never calls back into it mid-flow.
    fn mint(env: Env, to: Address, amount: i128, exchange_rate: i128);

    /// Admin only, and deliberately not holder-gated: the YM authenticates
    /// the holder at its own entry point, and a holder signature over a live
    /// rate would drift between simulation and execution.
    fn burn_with_rate(env: Env, from: Address, amount: i128, exchange_rate: i128);

    /// The vault exchange rate `address` last settled at.
    fn user_index(env: Env, address: Address) -> i128;

    /// Yield accrued since, in vault shares, awaiting `claim_yield`.
    fn accrued_yield(env: Env, address: Address) -> i128;

    /// Settles and pays out `user`'s accrued yield in vault shares. After
    /// maturity the position is closed: the remaining YT is burned.
    fn claim_yield(env: Env, user: Address) -> i128;
}

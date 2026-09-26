#![no_std]
use soroban_sdk::{contractclient, Address, Env, String};

/// What the principal token exposes beyond SEP-41.
///
/// The token surface itself (balance, transfer, allowances, `burn`,
/// `burn_from`, `total_supply`, metadata) is OpenZeppelin's `Base`, exposed
/// through its `FungibleToken` and `FungibleBurnable` traits; callers reach it
/// with `soroban_sdk::token::Client`. Burns are admin-gated on top of the
/// standard's own auth, so only the yield manager retires PT.
#[contractclient(name = "PrincipalTokenClient")]
pub trait PrincipalTokenTrait {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32);

    /// Admin (yield manager) only.
    fn mint(env: Env, to: Address, amount: i128);
}

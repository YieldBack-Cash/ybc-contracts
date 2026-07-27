use crate::events::{ContractUpgraded, Withdrawal};
use crate::storage;
use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, BytesN, Env};
use stellar_access::ownable::{self as ownable, Ownable};
use stellar_macros::only_owner;

/// Passive sink for protocol fees.
///
/// Fee-charging contracts (AMM reserve fee, YM surplus sweep) deliver value
/// here as plain token transfers to this contract's address — the treasury
/// itself does no fee accounting and exposes no rate. Rates belong to the
/// contracts that charge them, snapshotted per market at creation, so the
/// treasury owner holds no lever over live markets.
///
/// The address is meant to be baked immutably into markets at creation;
/// control rotates by transferring ownership, never the address. Ownership
/// uses OpenZeppelin's `stellar-access` Ownable: two-step transfer (propose +
/// accept, with an acceptance deadline), because a mistaken one-step handoff
/// would strand all held fees forever. `renounce_ownership` exists but is
/// self-defeating here — it permanently bricks `withdraw`.
#[contract]
pub struct Treasury;

pub trait TreasuryTrait {
    fn __constructor(env: Env, owner: Address);

    /// (Owner only) Withdraws `amount` of `token` to `to`. The treasury never
    /// custodies user funds, so everything it holds is the protocol's.
    fn withdraw(env: Env, token: Address, to: Address, amount: i128);

    /// (Owner only) Upgrades the treasury Wasm. The address markets point at
    /// is immutable, so upgrading in place is the only way to evolve the
    /// treasury; it grants the owner no power withdraw doesn't already imply.
    fn upgrade(env: Env, new_wasm_hash: BytesN<32>);
}

#[contractimpl]
impl TreasuryTrait for Treasury {
    fn __constructor(env: Env, owner: Address) {
        ownable::set_owner(&env, &owner);
    }

    #[only_owner]
    fn withdraw(env: Env, token: Address, to: Address, amount: i128) {
        storage::extend_instance_ttl(&env);

        assert!(amount > 0, "amount must be positive");

        TokenClient::new(&env, &token).transfer(&env.current_contract_address(), &to, &amount);

        Withdrawal { token, to, amount }.publish(&env);
    }

    #[only_owner]
    fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        storage::extend_instance_ttl(&env);

        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());

        ContractUpgraded { new_wasm_hash }.publish(&env);
    }
}

#[contractimpl(contracttrait)]
impl Ownable for Treasury {}
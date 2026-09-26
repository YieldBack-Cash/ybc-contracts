use principal_token_interface::PrincipalTokenTrait;
use soroban_sdk::{contract, contractimpl, Address, Env, MuxedAddress, String};
use stellar_tokens::fungible::{burnable::FungibleBurnable, Base, FungibleToken};

use crate::storage;

/// The principal token: a plain SEP-41 token whose supply only the yield
/// manager (its admin) may change.
///
/// Balances, allowances, total supply and metadata are OpenZeppelin's
/// `Base`, which also owns the amount and balance checks (`LessThanZero`,
/// `InsufficientBalance`, `InsufficientAllowance`, codes 100–105). The
/// hand-rolled ledger this replaced is where the repo's confirmed critical
/// bug lived: a negative `transfer` credited `from` and debited `to`.
#[contract]
pub struct PrincipalToken;

#[contractimpl]
impl FungibleToken for PrincipalToken {
    type ContractType = Base;

    fn total_supply(e: &Env) -> i128 {
        storage::extend_instance_ttl(e);
        Base::total_supply(e)
    }

    fn balance(e: &Env, account: Address) -> i128 {
        storage::extend_instance_ttl(e);
        Base::balance(e, &account)
    }

    fn allowance(e: &Env, owner: Address, spender: Address) -> i128 {
        storage::extend_instance_ttl(e);
        Base::allowance(e, &owner, &spender)
    }

    fn transfer(e: &Env, from: Address, to: MuxedAddress, amount: i128) {
        storage::extend_instance_ttl(e);
        Base::transfer(e, &from, &to, amount)
    }

    fn transfer_from(e: &Env, spender: Address, from: Address, to: Address, amount: i128) {
        storage::extend_instance_ttl(e);
        Base::transfer_from(e, &spender, &from, &to, amount)
    }

    fn approve(e: &Env, owner: Address, spender: Address, amount: i128, live_until_ledger: u32) {
        storage::extend_instance_ttl(e);
        Base::approve(e, &owner, &spender, amount, live_until_ledger)
    }

    fn decimals(e: &Env) -> u32 {
        Base::decimals(e)
    }

    fn name(e: &Env) -> String {
        Base::name(e)
    }

    fn symbol(e: &Env) -> String {
        Base::symbol(e)
    }
}

/// Burns are admin-gated on top of the standard's own auth: PT is only ever
/// retired by the yield manager, at redemption. `Base::burn` authenticates
/// the holder and `Base::burn_from` the spender and allowance, as SEP-41
/// requires; the admin check is what stops a holder burning on their own.
#[contractimpl]
impl FungibleBurnable for PrincipalToken {
    fn burn(e: &Env, from: Address, amount: i128) {
        storage::get_admin(e).require_auth();
        storage::extend_instance_ttl(e);
        Base::burn(e, &from, amount)
    }

    fn burn_from(e: &Env, spender: Address, from: Address, amount: i128) {
        storage::get_admin(e).require_auth();
        storage::extend_instance_ttl(e);
        Base::burn_from(e, &spender, &from, amount)
    }
}

#[contractimpl]
impl PrincipalTokenTrait for PrincipalToken {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32) {
        if decimals > 18 {
            panic!("Decimal must not be greater than 18");
        }
        storage::set_admin(&env, &admin);
        Base::set_metadata(&env, decimals, name, symbol);
    }

    fn mint(env: Env, to: Address, amount: i128) {
        storage::get_admin(&env).require_auth();
        storage::extend_instance_ttl(&env);
        Base::mint(&env, &to, amount);
    }
}

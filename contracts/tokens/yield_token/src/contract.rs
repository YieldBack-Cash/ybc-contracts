use soroban_sdk::{contract, contractimpl, Address, Env, MuxedAddress, String};
use stellar_tokens::fungible::{
    burnable::{emit_burn, FungibleBurnable},
    Base, FungibleToken,
};
use yield_manager_interface::YieldManagerClient;
use yield_token_interface::YieldTokenTrait;

use crate::{math, storage};

/// The yield token: a SEP-41 token whose holders accrue the vault's yield.
///
/// Two layers. The ledger (balances, allowances, total supply, metadata) is
/// OpenZeppelin's `Base`, which owns the amount and balance checks. On top of
/// it this contract keeps, per holder, the exchange rate they last settled at
/// and the yield accrued since, and settles both parties before every balance
/// movement. That hook is the whole of what is YBC's here.
#[contract]
pub struct YieldToken;

impl YieldToken {
    fn get_exchange_rate(env: &Env) -> i128 {
        let yield_manager = storage::get_admin(env);
        YieldManagerClient::new(env, &yield_manager).get_exchange_rate()
    }

    /// Settles `user`'s pending yield at the current rate and moves their
    /// index up to it. Must run before any change to their balance.
    fn accrue_yield(env: &Env, user: &Address, rate_hint: Option<i128>) -> i128 {
        let balance = Base::balance(env, user);
        let old_index = storage::get_user_index(env, user);

        // The YM mints and burns with the rate in hand and cannot be re-entered
        // for it, so it passes the rate down; every other path reads it.
        let current_rate: i128 = if let Some(rate) = rate_hint {
            rate
        } else {
            Self::get_exchange_rate(env)
        };

        // Initialize index for new users (even if they have no balance yet)
        if old_index == 0 {
            storage::set_user_index(env, user, current_rate);
            return current_rate;
        }

        // No balance: nothing to accrue, but the index must still track the live
        // rate. Leaving it parked lets a holder who was empty across a rate rise
        // carry a stale index into their next acquisition — `mint` and `transfer`
        // both accrue BEFORE raising the balance, so this branch is what runs at
        // acquisition time — and then claim yield for growth that predates their
        // ownership, which is paid out of other holders' principal backing.
        // Guarded so an unchanged rate still costs no storage write, and so a
        // (never-expected) lower rate can never move the index backwards.
        if balance == 0 {
            if current_rate > old_index {
                storage::set_user_index(env, user, current_rate);
            }
            return current_rate;
        }

        // The yield manager guarantees the exchange rate never decreases
        // So current_rate >= old_index is always true
        // This contract only update if rate increased to avoid unnecessary storage writes
        if current_rate > old_index {
            // Pending yield in vault SHARES; see `math::pending_yield`.
            let pending_yield = math::pending_yield(balance, old_index, current_rate);
            let current_accrued = storage::get_accrued_yield(env, user);
            storage::set_accrued_yield(env, user, current_accrued + pending_yield);
            storage::set_user_index(env, user, current_rate);
        }

        current_rate
    }

    /// Settles both parties of a transfer with one rate lookup. Left to
    /// itself each `accrue_yield` walks YT → YM → vault → the underlying
    /// lending pool for the current rate, so an unhinted pair costs two full
    /// round trips into Blend for a value that cannot change within a single
    /// transaction.
    fn accrue_pair(env: &Env, from: &Address, to: &Address) {
        let rate = Self::get_exchange_rate(env);
        Self::accrue_yield(env, from, Some(rate));
        Self::accrue_yield(env, to, Some(rate));
    }

    /// Retires `amount` of `from`'s YT without the holder's signature: the
    /// yield manager has authenticated the holder at its own entry point, or
    /// the position is being closed at maturity.
    fn burn_unsigned(env: &Env, from: &Address, amount: i128) {
        Base::update(env, Some(from), None, amount);
        emit_burn(env, from, amount);
    }
}

#[contractimpl]
impl FungibleToken for YieldToken {
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

    /// Settles both parties, then moves the balance. `Base::transfer`
    /// authenticates `from` and debits before it reads the credit side, so a
    /// self-transfer cannot mint YT out of nothing.
    fn transfer(e: &Env, from: Address, to: MuxedAddress, amount: i128) {
        storage::extend_instance_ttl(e);
        Self::accrue_pair(e, &from, &to.address());
        Base::transfer(e, &from, &to, amount)
    }

    fn transfer_from(e: &Env, spender: Address, from: Address, to: Address, amount: i128) {
        storage::extend_instance_ttl(e);
        Self::accrue_pair(e, &from, &to);
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

/// A holder may retire their own YT; the accrued yield stays claimable.
#[contractimpl]
impl FungibleBurnable for YieldToken {
    fn burn(e: &Env, from: Address, amount: i128) {
        storage::extend_instance_ttl(e);
        Self::accrue_yield(e, &from, None);
        Base::burn(e, &from, amount)
    }

    fn burn_from(e: &Env, spender: Address, from: Address, amount: i128) {
        storage::extend_instance_ttl(e);
        Self::accrue_yield(e, &from, None);
        Base::burn_from(e, &spender, &from, amount)
    }
}

#[contractimpl]
impl YieldTokenTrait for YieldToken {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32) {
        if decimals > 18 {
            panic!("Decimal must not be greater than 18");
        }
        storage::set_admin(&env, &admin);
        Base::set_metadata(&env, decimals, name, symbol);
    }

    fn mint(env: Env, to: Address, amount: i128, exchange_rate: i128) {
        storage::get_admin(&env).require_auth();
        storage::extend_instance_ttl(&env);

        Self::accrue_yield(&env, &to, Some(exchange_rate));
        Base::mint(&env, &to, amount);
    }

    /// Burns `from`'s YT at a rate the yield manager supplies.
    ///
    /// Admin-gated only — deliberately NOT `from.require_auth()`. The YM is the
    /// sole caller (that is what the admin check enforces) and every path it
    /// calls this from has already authenticated `from` at its own entrypoint,
    /// so a second check adds no authority.
    ///
    /// It actively broke things: `exchange_rate` is read live and moves with the
    /// vault every ledger, so requiring the holder's signature over an argument
    /// list containing it meant the wallet signed one rate during simulation and
    /// the chain executed with another. Observed on testnet as Auth/InvalidAction
    /// on every `redeem_combined` — a pre-existing bug, not one the asset-
    /// denominated entrypoints introduced.
    fn burn_with_rate(env: Env, from: Address, amount: i128, exchange_rate: i128) {
        storage::get_admin(&env).require_auth();
        storage::extend_instance_ttl(&env);

        Self::accrue_yield(&env, &from, Some(exchange_rate));
        Self::burn_unsigned(&env, &from, amount);
    }

    fn user_index(env: Env, address: Address) -> i128 {
        storage::extend_instance_ttl(&env);
        storage::get_user_index(&env, &address)
    }

    fn accrued_yield(env: Env, address: Address) -> i128 {
        storage::extend_instance_ttl(&env);
        storage::get_accrued_yield(&env, &address)
    }

    fn claim_yield(env: Env, user: Address) -> i128 {
        user.require_auth();
        storage::extend_instance_ttl(&env);

        let yield_manager = storage::get_admin(&env);
        let yield_manager_client = YieldManagerClient::new(&env, &yield_manager);

        Self::accrue_yield(&env, &user, None);

        let claimable = storage::get_accrued_yield(&env, &user);
        // The YM re-denominates locked claims to their locked-rate asset
        // value, so the shares actually paid can be fewer than `claimable` —
        // report what the user really received.
        let mut paid = 0;
        if claimable > 0 {
            storage::set_accrued_yield(&env, &user, 0);

            paid = yield_manager_client.distribute_yield(&user, &claimable);
        }

        // Past maturity the exchange rate is locked, so the accrual above was
        // the position's final one — burn the now-worthless YT so it doesn't
        // linger as dust.
        if env.ledger().timestamp() >= yield_manager_client.get_maturity() {
            let balance = Base::balance(&env, &user);
            if balance > 0 {
                Self::burn_unsigned(&env, &user, balance);
            }
        }

        paid
    }
}

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
/// movement. The hook is the only YBC-specific logic in this contract.
///
/// Units: balances are asset-denominated (7 decimals, like PT); `UserIndex`
/// and the exchange rate are `SCALAR_7`-scaled assets per share; `AccruedYield`
/// is vault shares.
#[contract]
pub struct YieldToken;

impl YieldToken {
    fn get_exchange_rate(env: &Env) -> i128 {
        let yield_manager = storage::get_admin(env);
        YieldManagerClient::new(env, &yield_manager).get_exchange_rate()
    }

    /// Settles `user`'s pending yield at the current rate and moves their
    /// index up to it. `math::pending_yield` prices the growth from the old
    /// index to the current rate on the balance held over that interval, so
    /// this must run before the balance changes; `mint`, `transfer*`, `burn*`
    /// and `claim_yield` all do.
    fn accrue_yield(env: &Env, user: &Address, rate_hint: Option<i128>) {
        let balance = Base::balance(env, user);
        let old_index = storage::get_user_index(env, user);

        // The YM mints and burns with the rate in hand and cannot be re-entered
        // for it, so it passes the rate down; every other path reads it.
        let current_rate: i128 = if let Some(rate) = rate_hint {
            rate
        } else {
            Self::get_exchange_rate(env)
        };

        // First touch: index 0 means never settled (see `DataKey::UserIndex`).
        if old_index == 0 {
            storage::set_user_index(env, user, current_rate);
            return;
        }

        // Empty holders still track the rate: `mint` and `transfer` accrue
        // before crediting, so this branch is what runs at acquisition. A parked
        // index would let a returning holder claim growth from before they owned
        // anything, paid from other holders' backing. Strict comparison so an
        // unchanged (or lower) rate costs no write.
        if balance == 0 {
            if current_rate > old_index {
                storage::set_user_index(env, user, current_rate);
            }
            return;
        }

        // The rate is expected non-decreasing (the YM locks it at maturity); a
        // lower rate accrues nothing and leaves the index alone.
        if current_rate > old_index {
            // Pending yield is in vault shares; see `math::pending_yield`.
            let pending_yield = math::pending_yield(balance, old_index, current_rate);
            let current_accrued = storage::get_accrued_yield(env, user);
            storage::set_accrued_yield(
                env,
                user,
                current_accrued
                    .checked_add(pending_yield)
                    .expect("accrued yield overflow"),
            );
            storage::set_user_index(env, user, current_rate);
        }
    }

    /// Settles both parties with one rate lookup; each unhinted `accrue_yield`
    /// would otherwise make its own YT → YM → vault call for a value that
    /// cannot change inside one transaction.
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

    /// As `transfer`, through an allowance; `Base::transfer_from`
    /// authenticates `spender`.
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
    // `decimals` is unchecked for the same reason as in `PrincipalToken`: the
    // factory passes 7.
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32) {
        storage::set_admin(&env, &admin);
        Base::set_metadata(&env, decimals, name, symbol);
    }

    fn mint(env: Env, to: Address, amount: i128, exchange_rate: i128) {
        storage::get_admin(&env).require_auth();
        storage::extend_instance_ttl(&env);

        Self::accrue_yield(&env, &to, Some(exchange_rate));
        Base::mint(&env, &to, amount);
    }

    /// Burns `from`'s YT at the rate the YM supplies. Admin-gated and
    /// deliberately not `from.require_auth()`: the YM has already authenticated
    /// `from` at its own entry point, and a holder signature over the live rate
    /// argument would drift between simulation and execution (see
    /// `YieldTokenTrait::burn_with_rate`).
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
            // Zero the claim before paying it: the YM call is external.
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

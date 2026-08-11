use soroban_sdk::{
    contract, contractimpl, token::TokenInterface, Address, Env, MuxedAddress, String,
};
use soroban_token_sdk::events::{Burn, Mint, Transfer};
use yield_manager_interface::YieldManagerClient;
use crate::storage;

const SCALAR_7: i128 = 1_0000000;

fn check_nonnegative_amount(amount: i128) {
    if amount < 0 {
        panic!("negative amount is not allowed: {}", amount)
    }
}

#[contract]
pub struct YieldToken;

impl YieldToken {
    fn get_exchange_rate(env: &Env) -> i128 {
        let yield_manager = storage::get_admin(env);
        YieldManagerClient::new(env, &yield_manager).get_exchange_rate()
    }

    fn accrue_yield(env: &Env, user: &Address, rate_hint: Option<i128>) -> i128 {
        let balance = storage::get_balance(env, user);
        let old_index = storage::get_user_index(env, user);

        //YM contract mints, but it cant re-enter. Rate is provided by the YM contract
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
            // Pending yield, converted to vault SHARES. `balance` is asset-
            // denominated (minted as shares * rate / SCALAR_7), so the accrued
            // amount `balance * (current_rate - old_index) / old_index` is in
            // ASSET units. It is paid out as shares by the yield manager, so
            // divide by `current_rate` (rescaled by SCALAR_7) to convert assets
            // to shares at the current price. The trailing division floors, so
            // the payout rounds down and the yield manager keeps a dust surplus.
            let pending_yield = balance
                .checked_mul(current_rate - old_index)
                .and_then(|v| v.checked_mul(SCALAR_7))
                .expect("overflow computing pending yield")
                / old_index
                    .checked_mul(current_rate)
                    .expect("overflow computing yield denominator");
            let current_accrued = storage::get_accrued_yield(env, user);
            storage::set_accrued_yield(env, user, current_accrued + pending_yield);
            storage::set_user_index(env, user, current_rate);
        }

        current_rate
    }
}

#[contractimpl]
impl TokenInterface for YieldToken {
    fn allowance(_env: Env, _from: Address, _spender: Address) -> i128 {
        // YieldToken doesn't support this function
        0
    }

    fn approve(
        _env: Env,
        _from: Address,
        _spender: Address,
        _amount: i128,
        _expiration_ledger: u32,
    ) {
        panic!("approve not supported for YieldToken");
    }

    fn balance(env: Env, id: Address) -> i128 {
        storage::extend_instance_ttl(&env);
        storage::get_balance(&env, &id)
    }

    fn transfer(env: Env, from: Address, to_muxed: MuxedAddress, amount: i128) {
        from.require_auth();
        storage::extend_instance_ttl(&env);
        check_nonnegative_amount(amount);

        let to: Address = to_muxed.address();

        let from_balance = storage::get_balance(&env, &from);
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        // One rate lookup for the pair. Left to itself each `accrue_yield` walks
        // YT → YM → vault → the underlying lending pool for the current rate,
        // so an unhinted transfer costs two full round trips into Blend for a
        // value that cannot change within a single transaction.
        let rate = Self::get_exchange_rate(&env);
        Self::accrue_yield(&env, &from, Some(rate));
        Self::accrue_yield(&env, &to, Some(rate));

        // Debit first, then read the credit side. When `from == to` both sides
        // are the same storage key, so reading `to_balance` up front would let
        // the credit overwrite the debit and mint `amount` YT out of nothing —
        // along with a claim on yield the manager holds no backing for.
        storage::set_balance(&env, &from, from_balance - amount);
        let to_balance = storage::get_balance(&env, &to);
        storage::set_balance(&env, &to, to_balance + amount);

        Transfer {
            from,
            to,
            to_muxed_id: to_muxed.id(),
            amount,
        }
        .publish(&env);
    }

    fn transfer_from(
        _env: Env,
        _spender: Address,
        _from: Address,
        _to: Address,
        _amount: i128,
    ) {
        panic!("transfer_from not supported for YieldToken");
    }

    fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        storage::extend_instance_ttl(&env);
        check_nonnegative_amount(amount);

        let balance = storage::get_balance(&env, &from);
        if balance < amount {
            panic!("Insufficient balance");
        }

        Self::accrue_yield(&env, &from, None);

        storage::set_balance(&env, &from, balance - amount);

        let total_supply = storage::get_total_supply(&env);
        storage::set_total_supply(&env, total_supply - amount);

        Burn { from, amount }.publish(&env);
    }

    fn burn_from(_env: Env, _spender: Address, _from: Address, _amount: i128) {
        panic!("burn_from not supported for YieldToken");
    }

    fn decimals(env: Env) -> u32 {
        storage::extend_instance_ttl(&env);
        storage::get_metadata(&env).decimal
    }

    fn name(env: Env) -> String {
        storage::extend_instance_ttl(&env);
        storage::get_metadata(&env).name
    }

    fn symbol(env: Env) -> String {
        storage::extend_instance_ttl(&env);
        storage::get_metadata(&env).symbol
    }
}

use yield_token_interface::YieldTokenTrait;

#[contractimpl]
impl YieldTokenTrait for YieldToken {
    fn __constructor(
        env: Env,
        admin: Address,
        name: String,
        symbol: String,
        decimals: u32,
    ) {
        if decimals > 18 {
            panic!("Decimal must not be greater than 18");
        }
        storage::set_admin(&env, &admin);
        storage::set_metadata(&env, name, symbol, decimals);
    }

    fn mint(env: Env, to: Address, amount: i128, exchange_rate: i128) {
        let admin = storage::get_admin(&env);
        admin.require_auth();
        storage::extend_instance_ttl(&env);
        check_nonnegative_amount(amount);

        Self::accrue_yield(&env, &to, Some(exchange_rate));

        let balance = storage::get_balance(&env, &to);
        storage::set_balance(&env, &to, balance + amount);

        let total_supply = storage::get_total_supply(&env);
        storage::set_total_supply(&env, total_supply + amount);

        Mint { to, to_muxed_id: None, amount }.publish(&env);
    }

    /// Admin-gated only, for the same reason as `burn_with_rate`: the YM is the
    /// sole caller and authenticates `from` itself, and `exchange_rate` is a
    /// live value that would make any holder signature drift between simulation
    /// and execution. Currently unused by the protocol; kept for symmetry.
    fn transfer_with_rate(env: Env, from: Address, to: Address, amount: i128, exchange_rate: i128) {
        storage::get_admin(&env).require_auth();
        storage::extend_instance_ttl(&env);
        check_nonnegative_amount(amount);

        let from_balance = storage::get_balance(&env, &from);
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        Self::accrue_yield(&env, &from, Some(exchange_rate));
        Self::accrue_yield(&env, &to, Some(exchange_rate));

        // Debit before reading the credit side — see `transfer` for why a
        // self-transfer would otherwise mint YT.
        storage::set_balance(&env, &from, from_balance - amount);
        let to_balance = storage::get_balance(&env, &to);
        storage::set_balance(&env, &to, to_balance + amount);

        Transfer { from, to, to_muxed_id: None, amount }.publish(&env);
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
        check_nonnegative_amount(amount);

        let balance = storage::get_balance(&env, &from);
        if balance < amount {
            panic!("Insufficient balance");
        }

        Self::accrue_yield(&env, &from, Some(exchange_rate));

        storage::set_balance(&env, &from, balance - amount);

        let total_supply = storage::get_total_supply(&env);
        storage::set_total_supply(&env, total_supply - amount);

        Burn { from, amount }.publish(&env);
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
            let balance = storage::get_balance(&env, &user);
            if balance > 0 {
                storage::set_balance(&env, &user, 0);

                let total_supply = storage::get_total_supply(&env);
                storage::set_total_supply(&env, total_supply - balance);

                Burn { from: user, amount: balance }.publish(&env);
            }
        }

        paid
    }

    fn total_supply(env: Env) -> i128 {
        storage::extend_instance_ttl(&env);
        storage::get_total_supply(&env)
    }
}
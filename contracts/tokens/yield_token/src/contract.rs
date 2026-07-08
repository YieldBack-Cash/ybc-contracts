use soroban_sdk::{
    contract, contractimpl, token::TokenInterface, Address, Env, MuxedAddress, String,
};
use soroban_token_sdk::events::{Burn, Mint, Transfer};
use yield_manager_interface::YieldManagerClient;
use crate::storage;

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

        // Early return if no balance (but index is already initialized above)
        if balance == 0 {
            return current_rate;
        }

        // The yield manager guarantees the exchange rate never decreases
        // So current_rate >= old_index is always true
        // This contract only update if rate increased to avoid unnecessary storage writes
        if current_rate > old_index {
            // Calculate pending yield in vault shares
            // balance is in token units (scaled by 1e7), rate ratio gives fractional yield
            let pending_yield = (balance * (current_rate - old_index)) / old_index;
            let current_accrued = storage::get_accrued_yield(env, user);
            storage::set_accrued_yield(env, user, current_accrued + pending_yield);
            storage::set_user_index(env, user, current_rate);
        }

        // If the rate hasn't gone up no yield to accrue, no storage update needed
        current_rate
    }
}

// SEP-41 TokenInterface implementation
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
        // YieldToken doesn't support this function
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

        Self::accrue_yield(&env, &from, None);
        Self::accrue_yield(&env, &to, None);

        let to_balance = storage::get_balance(&env, &to);

        storage::set_balance(&env, &from, from_balance - amount);
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
        // YieldToken doesn't support this function
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
        // YieldToken doesn't support this function
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

    fn transfer_with_rate(env: Env, from: Address, to: Address, amount: i128, exchange_rate: i128) {
        from.require_auth();
        storage::get_admin(&env).require_auth();
        storage::extend_instance_ttl(&env);
        check_nonnegative_amount(amount);

        let from_balance = storage::get_balance(&env, &from);
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        Self::accrue_yield(&env, &from, Some(exchange_rate));
        Self::accrue_yield(&env, &to, Some(exchange_rate));

        let to_balance = storage::get_balance(&env, &to);
        storage::set_balance(&env, &from, from_balance - amount);
        storage::set_balance(&env, &to, to_balance + amount);

        Transfer { from, to, to_muxed_id: None, amount }.publish(&env);
    }

    fn burn_with_rate(env: Env, from: Address, amount: i128, exchange_rate: i128) {
        from.require_auth();
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

        Self::accrue_yield(&env, &user, None);

        let claimable = storage::get_accrued_yield(&env, &user);
        if claimable == 0 {
            return 0;
        }

        storage::set_accrued_yield(&env, &user, 0);

        // Call yield manager (admin) to distribute vault shares
        let yield_manager = storage::get_admin(&env);
        let yield_manager_client = YieldManagerClient::new(&env, &yield_manager);
        yield_manager_client.distribute_yield(&user, &claimable);

        claimable
    }

    fn total_supply(env: Env) -> i128 {
        storage::extend_instance_ttl(&env);
        storage::get_total_supply(&env)
    }
}
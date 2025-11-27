use soroban_sdk::{contract, contractimpl, Address, Env, String};
use yield_manager_interface::YieldManagerClient;
use crate::storage;

pub trait YieldTokenTrait {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String);
    fn mint(env: Env, to: Address, amount: i128, exchange_rate: i128);
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
    fn burn(env: Env, from: Address, amount: i128);
    fn balance(env: Env, address: Address) -> i128;
    fn user_index(env: Env, address: Address) -> i128;
    fn accrued_yield(env: Env, address: Address) -> i128;
    fn total_supply(env: Env) -> i128;
    fn name(env: Env) -> String;
    fn symbol(env: Env) -> String;
    fn claim_yield(env: Env, user: Address) -> i128;
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
            // balance and rates are scaled by 1e6
            let pending_yield = (balance * (current_rate - old_index)) / old_index / 1_000_000;
            let current_accrued = storage::get_accrued_yield(env, user);
            storage::set_accrued_yield(env, user, current_accrued + pending_yield);
            storage::set_user_index(env, user, current_rate);
        }

        // If the rate hasn't gone up no yield to accrue, no storage update needed
        current_rate
    }
}

#[contractimpl]
impl YieldTokenTrait for YieldToken {
    fn __constructor(
        env: Env,
        admin: Address,
        name: String,
        symbol: String,
    ) {
        storage::set_admin(&env, &admin);
        storage::set_metadata(&env, name, symbol);
    }

    fn mint(env: Env, to: Address, amount: i128, exchange_rate: i128) {
        let admin = storage::get_admin(&env);
        admin.require_auth();

        Self::accrue_yield(&env, &to, Some(exchange_rate));

        let balance = storage::get_balance(&env, &to);
        storage::set_balance(&env, &to, balance + amount);

        let total_supply = storage::get_total_supply(&env);
        storage::set_total_supply(&env, total_supply + amount);
    }

    fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        let from_balance = storage::get_balance(&env, &from);
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        Self::accrue_yield(&env, &from, None);
        Self::accrue_yield(&env, &to, None);

        let to_balance = storage::get_balance(&env, &to);

        storage::set_balance(&env, &from, from_balance - amount);
        storage::set_balance(&env, &to, to_balance + amount);
    }

    fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();

        let balance = storage::get_balance(&env, &from);
        if balance < amount {
            panic!("Insufficient balance");
        }

        Self::accrue_yield(&env, &from, None);

        storage::set_balance(&env, &from, balance - amount);

        let total_supply = storage::get_total_supply(&env);
        storage::set_total_supply(&env, total_supply - amount);
    }

    fn balance(env: Env, address: Address) -> i128 {
        storage::get_balance(&env, &address)
    }

    fn user_index(env: Env, address: Address) -> i128 {
        storage::get_user_index(&env, &address)
    }

    fn accrued_yield(env: Env, address: Address) -> i128 {
        storage::get_accrued_yield(&env, &address)
    }

    fn total_supply(env: Env) -> i128 {
        storage::get_total_supply(&env)
    }

    fn name(env: Env) -> String {
        storage::get_metadata(&env).name
    }

    fn symbol(env: Env) -> String {
        storage::get_metadata(&env).symbol
    }

    fn claim_yield(env: Env, user: Address) -> i128 {
        user.require_auth();

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
}

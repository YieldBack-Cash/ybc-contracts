#![no_std]

mod storage;

use soroban_sdk::{contract, contractimpl, token::TokenInterface, Address, Env, MuxedAddress, String};
use soroban_token_sdk::events::{Approve, Burn, Mint, Transfer};
use storage::{
    read_administrator, read_allowance, read_balance, read_decimal, read_name, read_symbol,
    receive_balance, spend_allowance, spend_balance, write_administrator, write_allowance,
    write_metadata, increase_total_supply, decrease_total_supply, TokenMetadata,
    INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD,
};

pub trait PrincipalTokenTrait {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String, decimals: u32);
    fn mint(env: Env, to: Address, amount: i128);
}

#[contract]
pub struct PrincipalToken;

#[contractimpl]
impl TokenInterface for PrincipalToken {
    fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        read_allowance(&env, &from, &spender)
    }

    fn approve(env: Env, from: Address, spender: Address, amount: i128, expiration_ledger: u32) {
        from.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        write_allowance(&env, &from, &spender, amount, expiration_ledger);

        Approve {
            from,
            spender,
            amount,
            expiration_ledger,
        }
        .publish(&env);
    }

    fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
        read_balance(&env, &id)
    }

    fn transfer(env: Env, from: Address, to: MuxedAddress, amount: i128) {
        from.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        let to_addr = to.address();
        spend_balance(&env, &from, amount);
        receive_balance(&env, &to_addr, amount);

        Transfer {
            from,
            to: to_addr,
            to_muxed_id: to.id(),
            amount,
        }
        .publish(&env);
    }

    fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        spend_allowance(&env, &from, &spender, amount);
        spend_balance(&env, &from, amount);
        receive_balance(&env, &to, amount);

        Transfer {
            from,
            to,
            to_muxed_id: None,
            amount,
        }
        .publish(&env);
    }

    fn burn(env: Env, from: Address, amount: i128) {
        let admin = read_administrator(&env);
        admin.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        spend_balance(&env, &from, amount);
        decrease_total_supply(&env, amount);

        Burn { from, amount }.publish(&env);
    }

    fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        spend_allowance(&env, &from, &spender, amount);
        spend_balance(&env, &from, amount);
        decrease_total_supply(&env, amount);

        Burn { from, amount }.publish(&env);
    }

    fn decimals(env: Env) -> u32 {
        read_decimal(&env)
    }

    fn name(env: Env) -> String {
        read_name(&env)
    }

    fn symbol(env: Env) -> String {
        read_symbol(&env)
    }
}

#[contractimpl]
impl PrincipalTokenTrait for PrincipalToken {
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

        write_administrator(&env, &admin);
        write_metadata(
            &env,
            TokenMetadata {
                name,
                symbol,
                decimals,
            },
        );
    }

    fn mint(env: Env, to: Address, amount: i128) {
        let admin = read_administrator(&env);
        admin.require_auth();

        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);

        receive_balance(&env, &to, amount);
        increase_total_supply(&env, amount);

        Mint {
            to,
            to_muxed_id: None,
            amount,
        }
        .publish(&env);
    }
}

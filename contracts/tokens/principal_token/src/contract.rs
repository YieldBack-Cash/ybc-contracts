use soroban_sdk::{contract, contractimpl, token::TokenInterface, Address, Env, MuxedAddress, String};
use soroban_token_sdk::events::{Approve, Burn, Mint, Transfer};
use soroban_token_sdk::metadata::TokenMetadata;
use principal_token_interface::PrincipalTokenTrait;

use crate::storage::{
    extend_instance_ttl, read_administrator, read_allowance, read_balance, read_decimal,
    read_name, read_symbol, receive_balance, spend_allowance, spend_balance,
    write_administrator, write_allowance, write_metadata, increase_total_supply,
    decrease_total_supply, read_total_supply,
};

#[contract]
pub struct PrincipalToken;

#[contractimpl]
impl TokenInterface for PrincipalToken {
    fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        extend_instance_ttl(&env);
        read_allowance(&env, &from, &spender)
    }

    fn approve(env: Env, from: Address, spender: Address, amount: i128, expiration_ledger: u32) {
        from.require_auth();

        extend_instance_ttl(&env);

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
        extend_instance_ttl(&env);
        read_balance(&env, &id)
    }

    fn transfer(env: Env, from: Address, to: MuxedAddress, amount: i128) {
        from.require_auth();

        extend_instance_ttl(&env);

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

        extend_instance_ttl(&env);

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
        from.require_auth();
        let admin = read_administrator(&env);
        admin.require_auth();

        extend_instance_ttl(&env);

        spend_balance(&env, &from, amount);
        decrease_total_supply(&env, amount);

        Burn { from, amount }.publish(&env);
    }

    fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        let admin = read_administrator(&env);
        admin.require_auth();

        extend_instance_ttl(&env);

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
                decimal: decimals,
            },
        );
    }

    fn mint(env: Env, to: Address, amount: i128) {
        let admin = read_administrator(&env);
        admin.require_auth();

        extend_instance_ttl(&env);

        receive_balance(&env, &to, amount);
        increase_total_supply(&env, amount);

        Mint {
            to,
            to_muxed_id: None,
            amount,
        }
        .publish(&env);
    }

    fn total_supply(env: Env) -> i128 {
        extend_instance_ttl(&env);
        read_total_supply(&env)
    }
}
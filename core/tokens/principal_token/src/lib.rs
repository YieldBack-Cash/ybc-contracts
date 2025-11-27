#![no_std]
use soroban_sdk::{contracttype, Address, Env, String};

#[cfg(feature = "contract")]
use soroban_sdk::{contract, contractimpl};

#[contracttype]
#[derive(Clone)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
}

pub trait PrincipalTokenTrait {
    fn __constructor(env: Env, admin: Address, name: String, symbol: String);
    fn mint(env: Env, to: Address, amount: i128);
    fn burn(env: Env, from: Address, amount: i128);
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
    fn balance(env: Env, address: Address) -> i128;
    fn total_supply(env: Env) -> i128;
    fn name(env: Env) -> String;
    fn symbol(env: Env) -> String;
}

#[cfg(feature = "contract")]
#[contract]
pub struct PrincipalToken;

#[cfg(feature = "contract")]
#[contractimpl]
impl PrincipalTokenTrait for PrincipalToken {
     fn __constructor(
        env: Env,
        admin: Address,
        name: String,
        symbol: String,
    ) {
        let metadata = TokenMetadata {
            name,
            symbol,
        };

        env.storage().instance().set(&"admin", &admin);
        env.storage().instance().set(&"metadata", &metadata);
    }

     fn mint(env: Env, to: Address, amount: i128) {
        let admin: Address = env.storage().instance().get(&"admin").unwrap();
        admin.require_auth();

        let balance = Self::balance(env.clone(), to.clone());
        env.storage().persistent().set(&to, &(balance + amount));

        let total_supply: i128 = env.storage().instance().get(&"total_supply").unwrap_or(0);
        env.storage().instance().set(&"total_supply", &(total_supply + amount));
    }

     fn burn(env: Env, from: Address, amount: i128) {
        let admin: Address = env.storage().instance().get(&"admin").unwrap();
        admin.require_auth();

        let balance = Self::balance(env.clone(), from.clone());
        if balance < amount {
            panic!("Insufficient balance");
        }

        env.storage().persistent().set(&from, &(balance - amount));

        let total_supply: i128 = env.storage().instance().get(&"total_supply").unwrap_or(0);
        env.storage().instance().set(&"total_supply", &(total_supply - amount));
    }

     fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            panic!("Insufficient balance");
        }

        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage().persistent().set(&from, &(from_balance - amount));
        env.storage().persistent().set(&to, &(to_balance + amount));
    }

     fn balance(env: Env, address: Address) -> i128 {
        env.storage().persistent().get(&address).unwrap_or(0)
    }

     fn total_supply(env: Env) -> i128 {
        env.storage().instance().get(&"total_supply").unwrap_or(0)
    }

     fn name(env: Env) -> String {
        let metadata: TokenMetadata = env.storage().instance().get(&"metadata").unwrap();
        metadata.name
    }

     fn symbol(env: Env) -> String {
        let metadata: TokenMetadata = env.storage().instance().get(&"metadata").unwrap();
        metadata.symbol
    }
}

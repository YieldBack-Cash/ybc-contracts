use soroban_sdk::{contracttype, Address, Env};

/// Instance TTL (the factory address this router resolves markets through).
/// Call once per entrypoint so the router doesn't expire from inactivity.
pub use ybc_common::ttl::extend_instance_ttl;

#[contracttype]
pub enum DataKey {
    Factory,
}

pub fn set_factory(env: &Env, factory: &Address) {
    env.storage().instance().set(&DataKey::Factory, factory);
}

pub fn get_factory(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Factory).unwrap()
}

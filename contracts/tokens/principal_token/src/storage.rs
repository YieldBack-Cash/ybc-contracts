use soroban_sdk::{contracttype, Address, Env};

// Balances, allowances, total supply and metadata are OpenZeppelin's
// (`stellar_tokens::fungible::Base`) and never appear here.

pub use ybc_common::ttl::extend_instance_ttl;

#[contracttype]
pub enum DataKey {
    /// The yield manager: the only address that may mint or burn.
    Admin,
}

pub fn set_admin(e: &Env, admin: &Address) {
    e.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_admin(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::Admin).expect("Admin not set")
}

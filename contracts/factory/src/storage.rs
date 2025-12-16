use soroban_sdk::{Address, Env};

// Storage keys
const ADMIN_KEY: &str = "admin";
const CURRENT_YIELD_MANAGER_KEY: &str = "cur_ym";
const CURRENT_PT_TOKEN_KEY: &str = "cur_pt";
const CURRENT_YT_TOKEN_KEY: &str = "cur_yt";
const CURRENT_PT_POOL_KEY: &str = "cur_pt_pool";
const CURRENT_YT_POOL_KEY: &str = "cur_yt_pool";

// Admin functions
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&ADMIN_KEY, admin);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&ADMIN_KEY)
        .expect("Admin not set")
}

// Current yield manager
pub fn set_current_yield_manager(env: &Env, yield_manager: &Address) {
    env.storage().instance().set(&CURRENT_YIELD_MANAGER_KEY, yield_manager);
}

pub fn get_current_yield_manager(env: &Env) -> Option<Address> {
    env.storage().instance().get(&CURRENT_YIELD_MANAGER_KEY)
}

// Current PT token
pub fn set_current_pt_token(env: &Env, pt_token: &Address) {
    env.storage().instance().set(&CURRENT_PT_TOKEN_KEY, pt_token);
}

pub fn get_current_pt_token(env: &Env) -> Option<Address> {
    env.storage().instance().get(&CURRENT_PT_TOKEN_KEY)
}

// Current YT token
pub fn set_current_yt_token(env: &Env, yt_token: &Address) {
    env.storage().instance().set(&CURRENT_YT_TOKEN_KEY, yt_token);
}

pub fn get_current_yt_token(env: &Env) -> Option<Address> {
    env.storage().instance().get(&CURRENT_YT_TOKEN_KEY)
}

// Current PT pool
pub fn set_current_pt_pool(env: &Env, pt_pool: &Address) {
    env.storage().instance().set(&CURRENT_PT_POOL_KEY, pt_pool);
}

pub fn get_current_pt_pool(env: &Env) -> Option<Address> {
    env.storage().instance().get(&CURRENT_PT_POOL_KEY)
}

// Current YT pool
pub fn set_current_yt_pool(env: &Env, yt_pool: &Address) {
    env.storage().instance().set(&CURRENT_YT_POOL_KEY, yt_pool);
}

pub fn get_current_yt_pool(env: &Env) -> Option<Address> {
    env.storage().instance().get(&CURRENT_YT_POOL_KEY)
}

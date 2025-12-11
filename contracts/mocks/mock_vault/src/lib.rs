#![no_std]
use soroban_sdk::{contract, contractimpl, token, Address, Env, String, Symbol};

/// Mock Vault Contract
///
/// Simplified vault that simulates yield by linearly increasing the exchange rate over time.
/// Instead of using a strategy, yield accrues based on time elapsed and a configurable yield rate.
/// Vault shares are fully transferable fungible tokens.
#[contract]
pub struct MockVault;

/// Storage keys
const ASSET: &str = "asset";
const TOTAL_SHARES: &str = "total_shares";
const LAST_UPDATE_TIME: &str = "last_update_time";
const YIELD_RATE: &str = "yield_rate"; // Basis points per second (10000 = 1% per second)
const INITIAL_VIRTUAL_BALANCE: &str = "initial_virtual_balance"; // Virtual assets to bootstrap exchange rate

const BASIS_POINTS_SCALE: i128 = 10_000; // 1 basis point = 0.01%

#[contractimpl]
impl MockVault {
    /// Initialize the mock vault
    /// yield_rate_bps: yield rate in basis points per second (e.g., 1 = 0.01% per second)
    pub fn __constructor(e: Env, asset: Address, yield_rate_bps: i128) {
        e.storage().instance().set(&ASSET, &asset);
        e.storage().instance().set(&YIELD_RATE, &yield_rate_bps);

        // Initialize with virtual balance to enable exchange rate calculations
        // Set 1 billion units (with 7 decimals = 100 tokens) as virtual initial balance
        let initial_balance = 1_000_000_000i128;
        e.storage().instance().set(&INITIAL_VIRTUAL_BALANCE, &initial_balance);

        // Initialize with matching shares (1:1 ratio initially)
        e.storage().instance().set(&TOTAL_SHARES, &initial_balance);

        // Set initial timestamp
        let current_time = e.ledger().timestamp();
        e.storage().instance().set(&LAST_UPDATE_TIME, &current_time);
    }

    /// Get the current exchange rate (assets per share)
    pub fn exchange_rate(e: Env) -> i128 {
        let total_shares = Self::get_total_shares(&e);
        if total_shares == 0 {
            return 1_000_000; // 1:1 initially (scaled by 1e6)
        }

        let total_assets = Self::total_assets(e);
        // exchange_rate = total_assets / total_shares * 1e6
        total_assets
            .checked_mul(1_000_000)
            .and_then(|v| v.checked_div(total_shares))
            .unwrap_or(1_000_000)
    }

    /// Deposit assets and receive shares
    pub fn deposit(e: Env, from: Address, assets: i128) -> i128 {
        from.require_auth();

        if assets <= 0 {
            panic!("deposit amount must be positive");
        }

        // Update the timestamp before calculating shares
        Self::update_timestamp(&e);

        // Calculate shares to mint
        let shares = Self::convert_to_shares(&e, assets);

        // Transfer assets from user to vault
        let asset_addr = Self::get_asset(&e);
        let asset_client = token::Client::new(&e, &asset_addr);
        asset_client.transfer(&from, &e.current_contract_address(), &assets);

        // Mint shares to user
        Self::mint_shares(&e, &from, shares);

        shares
    }

    /// Withdraw assets by burning shares
    pub fn withdraw(e: Env, to: Address, shares: i128) -> i128 {
        to.require_auth();

        if shares <= 0 {
            panic!("withdraw amount must be positive");
        }

        // Update the timestamp before calculating assets
        Self::update_timestamp(&e);

        // Check user has enough shares
        let user_balance = Self::get_balance(&e, &to);
        if user_balance < shares {
            panic!("insufficient shares");
        }

        // Calculate assets to return
        let assets = Self::convert_to_assets(&e, shares);

        // Get actual balance (not simulated total_assets)
        let asset_addr = Self::get_asset(&e);
        let asset_client = token::Client::new(&e, &asset_addr);
        let vault_balance = asset_client.balance(&e.current_contract_address());

        if vault_balance < assets {
            panic!("insufficient vault balance");
        }

        // Burn shares
        Self::burn_shares(&e, &to, shares);

        // Transfer assets to user
        asset_client.transfer(&e.current_contract_address(), &to, &assets);

        assets
    }

    /// Get share balance for an address
    pub fn balance(e: Env, account: Address) -> i128 {
        Self::get_balance(&e, &account)
    }

    /// Get total shares outstanding
    pub fn total_shares(e: Env) -> i128 {
        Self::get_total_shares(&e)
    }

    /// Get total assets (simulated - includes time-based yield)
    pub fn total_assets(e: Env) -> i128 {
        // Get actual asset balance
        let asset_addr = Self::get_asset(&e);
        let asset_client = token::Client::new(&e, &asset_addr);
        let actual_balance = asset_client.balance(&e.current_contract_address());

        // Add virtual initial balance
        let virtual_balance = Self::get_initial_virtual_balance(&e);
        let total_principal = actual_balance
            .checked_add(virtual_balance)
            .unwrap_or(actual_balance);

        // Calculate yield based on time elapsed
        let yield_accrued = Self::calculate_yield(&e, total_principal);

        total_principal
            .checked_add(yield_accrued)
            .unwrap_or(total_principal)
    }

    /// Set the yield rate (in basis points per second)
    pub fn set_yield_rate(e: Env, yield_rate_bps: i128) {
        e.storage().instance().set(&YIELD_RATE, &yield_rate_bps);
    }

    /// Get the yield rate
    pub fn get_yield_rate(e: Env) -> i128 {
        e.storage()
            .instance()
            .get(&YIELD_RATE)
            .unwrap_or(0)
    }

    /// Get time elapsed since last update
    pub fn time_elapsed(e: Env) -> u64 {
        let current_time = e.ledger().timestamp();
        let last_update = Self::get_last_update_time(&e);
        current_time.saturating_sub(last_update)
    }

    // ========== Token Standard Functions ==========

    /// Transfer shares from one account to another
    pub fn transfer(e: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        if amount < 0 {
            panic!("negative amount");
        }

        let from_balance = Self::get_balance(&e, &from);
        if from_balance < amount {
            panic!("insufficient balance");
        }

        Self::set_balance(&e, &from, from_balance - amount);
        let to_balance = Self::get_balance(&e, &to);
        Self::set_balance(&e, &to, to_balance + amount);
    }

    /// Transfer shares from one account to another using an allowance
    pub fn transfer_from(e: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();

        if amount < 0 {
            panic!("negative amount");
        }

        // Check allowance
        let allowance = Self::get_allowance(&e, &from, &spender);
        if allowance < amount {
            panic!("insufficient allowance");
        }

        // Decrease allowance
        Self::set_allowance(&e, &from, &spender, allowance - amount);

        // Transfer
        let from_balance = Self::get_balance(&e, &from);
        if from_balance < amount {
            panic!("insufficient balance");
        }

        Self::set_balance(&e, &from, from_balance - amount);
        let to_balance = Self::get_balance(&e, &to);
        Self::set_balance(&e, &to, to_balance + amount);
    }

    /// Approve an allowance for a spender
    pub fn approve(e: Env, from: Address, spender: Address, amount: i128) {
        from.require_auth();

        if amount < 0 {
            panic!("negative amount");
        }

        Self::set_allowance(&e, &from, &spender, amount);
    }

    /// Get the allowance for a spender
    pub fn allowance(e: Env, from: Address, spender: Address) -> i128 {
        Self::get_allowance(&e, &from, &spender)
    }

    /// Get the token name
    pub fn name(e: Env) -> String {
        String::from_str(&e, "Vault Share Token")
    }

    /// Get the token symbol
    pub fn symbol(e: Env) -> Symbol {
        Symbol::new(&e, "SHARE")
    }

    /// Get the token decimals
    pub fn decimals(_e: Env) -> u32 {
        7
    }

    // ========== Internal Helper Functions ==========

    /// Update the last update timestamp
    fn update_timestamp(e: &Env) {
        let current_time = e.ledger().timestamp();
        e.storage().instance().set(&LAST_UPDATE_TIME, &current_time);
    }

    /// Calculate yield accrued since last update
    fn calculate_yield(e: &Env, principal: i128) -> i128 {
        let current_time = e.ledger().timestamp();
        let last_update = Self::get_last_update_time(e);
        let time_elapsed = current_time.saturating_sub(last_update) as i128;

        let yield_rate = Self::get_yield_rate_internal(e);

        // yield = principal * yield_rate * time_elapsed / BASIS_POINTS_SCALE
        principal
            .checked_mul(yield_rate)
            .and_then(|v| v.checked_mul(time_elapsed))
            .and_then(|v| v.checked_div(BASIS_POINTS_SCALE))
            .unwrap_or(0)
    }

    /// Get the asset address
    fn get_asset(e: &Env) -> Address {
        e.storage()
            .instance()
            .get(&ASSET)
            .expect("asset not initialized")
    }

    /// Get total shares
    fn get_total_shares(e: &Env) -> i128 {
        e.storage().instance().get(&TOTAL_SHARES).unwrap_or(0)
    }

    /// Get yield rate (internal)
    fn get_yield_rate_internal(e: &Env) -> i128 {
        e.storage()
            .instance()
            .get(&YIELD_RATE)
            .unwrap_or(0)
    }

    /// Get last update time
    fn get_last_update_time(e: &Env) -> u64 {
        e.storage()
            .instance()
            .get(&LAST_UPDATE_TIME)
            .unwrap_or(0)
    }

    /// Get initial virtual balance
    fn get_initial_virtual_balance(e: &Env) -> i128 {
        e.storage()
            .instance()
            .get(&INITIAL_VIRTUAL_BALANCE)
            .unwrap_or(0)
    }

    /// Get balance for an account
    fn get_balance(e: &Env, account: &Address) -> i128 {
        let key = ("balance", account);
        e.storage().instance().get(&key).unwrap_or(0)
    }

    /// Set balance for an account
    fn set_balance(e: &Env, account: &Address, amount: i128) {
        let key = ("balance", account);
        e.storage().instance().set(&key, &amount);
    }

    /// Get allowance for a spender
    fn get_allowance(e: &Env, from: &Address, spender: &Address) -> i128 {
        let key = ("allowance", from, spender);
        e.storage().instance().get(&key).unwrap_or(0)
    }

    /// Set allowance for a spender
    fn set_allowance(e: &Env, from: &Address, spender: &Address, amount: i128) {
        let key = ("allowance", from, spender);
        e.storage().instance().set(&key, &amount);
    }

    /// Convert assets to shares based on current exchange rate
    fn convert_to_shares(e: &Env, assets: i128) -> i128 {
        let total_shares = Self::get_total_shares(e);
        if total_shares == 0 {
            // First deposit: 1:1 ratio
            return assets;
        }

        let total_assets = MockVault::total_assets(e.clone());
        if total_assets == 0 {
            return assets;
        }

        // shares = assets * total_shares / total_assets
        assets
            .checked_mul(total_shares)
            .and_then(|v| v.checked_div(total_assets))
            .unwrap_or(0)
    }

    /// Convert shares to assets based on current exchange rate
    fn convert_to_assets(e: &Env, shares: i128) -> i128 {
        let total_shares = Self::get_total_shares(e);
        if total_shares == 0 {
            return 0;
        }

        let total_assets = MockVault::total_assets(e.clone());

        // assets = shares * total_assets / total_shares
        shares
            .checked_mul(total_assets)
            .and_then(|v| v.checked_div(total_shares))
            .unwrap_or(0)
    }

    /// Mint shares to an account
    fn mint_shares(e: &Env, to: &Address, amount: i128) {
        let current_balance = Self::get_balance(e, to);
        let new_balance = current_balance
            .checked_add(amount)
            .expect("balance overflow");
        Self::set_balance(e, to, new_balance);

        let total_shares = Self::get_total_shares(e);
        let new_total = total_shares.checked_add(amount).expect("total overflow");
        e.storage().instance().set(&TOTAL_SHARES, &new_total);
    }

    /// Burn shares from an account
    fn burn_shares(e: &Env, from: &Address, amount: i128) {
        let current_balance = Self::get_balance(e, from);
        if current_balance < amount {
            panic!("insufficient balance to burn");
        }

        let new_balance = current_balance - amount;
        Self::set_balance(e, from, new_balance);

        let total_shares = Self::get_total_shares(e);
        let new_total = total_shares - amount;
        e.storage().instance().set(&TOTAL_SHARES, &new_total);
    }
}

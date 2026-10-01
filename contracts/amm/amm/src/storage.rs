use amm_interface::AmmError;
use soroban_sdk::{contracttype, token, Address, Env};

#[derive(Clone)]
#[contracttype]
pub struct MarketState {
    /// The principal token.
    pub token_a: Address,
    /// The vault contract, which is itself the share token (V).
    pub token_b: Address,
    /// PT held by the pool, in PT units.
    pub reserve_a: i128,
    /// Vault shares held by the pool; the curve converts to assets at trade time.
    pub reserve_b: i128,
    pub expiry_ts: u64,
    /// The implied rate after the last trade, ln-space, 1e7-scaled.
    pub last_implied_rate: i128,
    /// Time-independent curve scalar; divided by time-to-expiry per trade.
    pub scalar_root: i128,
    /// Time-independent fee rate; exponentiated with time-to-expiry per trade.
    pub fee_rate_root: i128,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    TotalShares,
    Shares(Address),
    MarketState,
    /// Trusted flash-swap receiver (the yield manager). Only this address may be
    /// passed as `receiver` to `flash_swap_pt` / `flash_swap_v`.
    Ym,
    /// Protocol fee sink. The reserve cut of each trade's fee is transferred
    /// here inline, so fees never accumulate in the pool's own balance.
    Treasury,
    /// Fraction of each trade's fee remitted to the treasury (1e7-scaled,
    /// e.g. 1_000_000 = 10% of the fee). Snapshotted at construction — no
    /// one can change it on a live market.
    ReserveFeeRate,
}

/// Instance TTL (market state, total shares). Call once per entrypoint so the
/// pool's own config doesn't expire from inactivity.
pub use ybc_common::ttl::extend_instance_ttl;
pub use ybc_common::ttl::{PERSISTENT_BUMP_AMOUNT, PERSISTENT_LIFETIME_THRESHOLD};

pub fn get_market_state(e: &Env) -> MarketState {
    e.storage().instance().get(&DataKey::MarketState).unwrap()
}

pub fn put_market_state(e: &Env, state: &MarketState) {
    e.storage().instance().set(&DataKey::MarketState, state);
}

pub fn set_ym(e: &Env, ym: &Address) {
    e.storage().instance().set(&DataKey::Ym, ym);
}

pub fn get_ym(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::Ym).unwrap()
}

pub fn set_treasury(e: &Env, treasury: &Address) {
    e.storage().instance().set(&DataKey::Treasury, treasury);
}

pub fn get_treasury(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::Treasury).unwrap()
}

pub fn set_reserve_fee_rate(e: &Env, rate: i128) {
    e.storage().instance().set(&DataKey::ReserveFeeRate, &rate);
}

pub fn get_reserve_fee_rate(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&DataKey::ReserveFeeRate)
        .unwrap()
}

pub fn get_total_shares(e: &Env) -> i128 {
    e.storage().instance().get(&DataKey::TotalShares).unwrap()
}

/// The pool's own balance of `token`.
pub fn get_balance(e: &Env, token: &Address) -> i128 {
    token::TokenClient::new(e, token).balance(&e.current_contract_address())
}

pub fn get_shares(e: &Env, user: &Address) -> i128 {
    let key = DataKey::Shares(user.clone());
    if let Some(shares) = e.storage().persistent().get(&key) {
        e.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
        shares
    } else {
        0
    }
}

pub fn put_shares(e: &Env, user: &Address, amount: i128) {
    let key = DataKey::Shares(user.clone());
    e.storage().persistent().set(&key, &amount);
    e.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );
}

pub fn put_total_shares(e: &Env, amount: i128) {
    e.storage().instance().set(&DataKey::TotalShares, &amount)
}

pub fn burn_shares(e: &Env, from: &Address, amount: i128) -> Result<(), AmmError> {
    let current_shares = get_shares(e, from);
    if current_shares < amount {
        return Err(AmmError::InsufficientShares);
    }
    let total = get_total_shares(e);
    put_shares(e, from, current_shares - amount);
    put_total_shares(e, total - amount);
    Ok(())
}

pub fn mint_shares(e: &Env, to: &Address, amount: i128) {
    let current_shares = get_shares(e, to);
    let total = get_total_shares(e);
    put_shares(e, to, current_shares + amount);
    put_total_shares(e, total + amount);
}

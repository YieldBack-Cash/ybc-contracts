use crate::storage::*;
use soroban_sdk::{token, Address, Env};

/// Transfers tokens from the contract to a recipient.
///
/// # Arguments
/// * `token` - Token contract address
/// * `to` - Recipient address
/// * `amount` - Amount to transfer
fn transfer(e: &Env, token: Address, to: Address, amount: i128) {
    token::TokenClient::new(e, &token).transfer(&e.current_contract_address(), &to, &amount);
}

/// Transfers token A from the contract to a recipient.
///
/// # Arguments
/// * `to` - Recipient address
/// * `amount` - Amount of token A to transfer
pub(crate) fn transfer_a(e: &Env, to: Address, amount: i128) {
    transfer(e, get_token_a(e), to, amount);
}

/// Transfers token B from the contract to a recipient.
///
/// # Arguments
/// * `to` - Recipient address
/// * `amount` - Amount of token B to transfer
pub(crate) fn transfer_b(e: &Env, to: Address, amount: i128) {
    transfer(e, get_token_b(e), to, amount);
}

/// Calculates optimal deposit amounts that maintain the constant product ratio.
///
/// # Arguments
/// * `desired_a` - Desired amount of token A
/// * `min_a` - Minimum acceptable amount of token A
/// * `desired_b` - Desired amount of token B
/// * `min_b` - Minimum acceptable amount of token B
/// * `reserve_a` - Current reserve of token A
/// * `reserve_b` - Current reserve of token B
///
/// # Returns
/// `(amount_a, amount_b)` to deposit
pub(crate) fn transfer_v_from_user_to_pool(e: &Env, from: &Address, v_in: i128) {
    let market = get_market_state(e);
    token::TokenClient::new(e, &market.token_b).transfer(from, &e.current_contract_address(), &v_in);
}

pub(crate) fn transfer_pt_from_pool_to_user(e: &Env, to: &Address, pt_out: i128) {
    transfer_a(e, to.clone(), pt_out);
}

pub(crate) fn transfer_pt_from_user_to_pool(e: &Env, from: &Address, pt_in: i128) {
    let market = get_market_state(e);
    token::TokenClient::new(e, &market.token_a).transfer(from, &e.current_contract_address(), &pt_in);
}

pub(crate) fn transfer_v_from_pool_to_user(e: &Env, to: &Address, v_out: i128) {
    transfer_b(e, to.clone(), v_out);
}

pub(crate) fn get_deposit_amounts(
    desired_a: i128,
    min_a: i128,
    desired_b: i128,
    min_b: i128,
    reserve_a: i128,
    reserve_b: i128,
) -> (i128, i128) {
    if reserve_a == 0 && reserve_b == 0 {
        return (desired_a, desired_b);
    }

    let amount_b = desired_a * reserve_b / reserve_a;
    if amount_b <= desired_b {
        if amount_b < min_b {
            panic!("amount_b less than min")
        }
        (desired_a, amount_b)
    } else {
        let amount_a = desired_b * reserve_a / reserve_b;
        if amount_a > desired_a || amount_a < min_a {
            panic!("amount_a invalid")
        }
        (amount_a, desired_b)
    }
}
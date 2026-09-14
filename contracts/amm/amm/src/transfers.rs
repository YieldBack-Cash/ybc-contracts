use amm_interface::AmmError;
use soroban_sdk::{token, Address, Env};

/// Transfers tokens from the contract to a recipient.
///
/// # Arguments
/// * `token` - Token contract address
/// * `to` - Recipient address
/// * `amount` - Amount to transfer
fn transfer(e: &Env, token: &Address, to: &Address, amount: i128) {
    token::TokenClient::new(e, token).transfer(&e.current_contract_address(), to, &amount);
}

/// Transfers tokens from a user into the contract.
fn transfer_in(e: &Env, token: &Address, from: &Address, amount: i128) {
    token::TokenClient::new(e, token).transfer(from, &e.current_contract_address(), &amount);
}

// Token addresses are passed in by callers (who already hold MarketState in scope)
// to avoid re-reading market state from storage on every transfer.

pub(crate) fn transfer_v_from_user_to_pool(e: &Env, token_v: &Address, from: &Address, v_in: i128) {
    transfer_in(e, token_v, from, v_in);
}

pub(crate) fn transfer_pt_from_pool_to_user(e: &Env, token_pt: &Address, to: &Address, pt_out: i128) {
    transfer(e, token_pt, to, pt_out);
}

pub(crate) fn transfer_pt_from_user_to_pool(e: &Env, token_pt: &Address, from: &Address, pt_in: i128) {
    transfer_in(e, token_pt, from, pt_in);
}

pub(crate) fn transfer_v_from_pool_to_user(e: &Env, token_v: &Address, to: &Address, v_out: i128) {
    transfer(e, token_v, to, v_out);
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
pub(crate) fn get_deposit_amounts(
    desired_a: i128,
    min_a: i128,
    desired_b: i128,
    min_b: i128,
    reserve_a: i128,
    reserve_b: i128,
) -> Result<(i128, i128), AmmError> {
    if reserve_a == 0 && reserve_b == 0 {
        return Ok((desired_a, desired_b));
    }

    // Reserves both positive or both zero; anything else is corrupt state.
    if reserve_a <= 0 || reserve_b <= 0 {
        return Err(AmmError::InvalidPoolState);
    }

    let amount_b = desired_a
        .checked_mul(reserve_b)
        .ok_or(AmmError::MathOverflow)?
        / reserve_a;
    if amount_b <= desired_b {
        if amount_b < min_b {
            return Err(AmmError::DepositMinNotMet);
        }
        Ok((desired_a, amount_b))
    } else {
        let amount_a = desired_b
            .checked_mul(reserve_a)
            .ok_or(AmmError::MathOverflow)?
            / reserve_b;
        if amount_a > desired_a || amount_a < min_a {
            return Err(AmmError::DepositMinNotMet);
        }
        Ok((amount_a, desired_b))
    }
}
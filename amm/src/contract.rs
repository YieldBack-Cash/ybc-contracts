use crate::storage::*;
use num_integer::Roots;
use soroban_sdk::{contract, contractimpl, token, Address, Env};

/// Transfers tokens from the contract to a recipient address
///
/// # Arguments
/// * `e` - The environment
/// * `token` - The token contract address to transfer
/// * `to` - The recipient address
/// * `amount` - The amount to transfer
fn transfer(e: &Env, token: Address, to: Address, amount: i128) {
    token::Client::new(e, &token).transfer(&e.current_contract_address(), &to, &amount);
}

/// Transfers token A from the contract to a recipient address
///
/// # Arguments
/// * `e` - The environment
/// * `to` - The recipient address
/// * `amount` - The amount of token A to transfer
fn transfer_a(e: &Env, to: Address, amount: i128) {
    transfer(e, get_token_a(e), to, amount);
}

/// Transfers token B from the contract to a recipient address
///
/// # Arguments
/// * `e` - The environment
/// * `to` - The recipient address
/// * `amount` - The amount of token B to transfer
fn transfer_b(e: &Env, to: Address, amount: i128) {
    transfer(e, get_token_b(e), to, amount);
}

/// Calculates the optimal deposit amounts based on current pool reserves
/// Maintains the constant product ratio (x * y = k) for balanced deposits
///
/// # Arguments
/// * `desired_a` - Desired amount of token A to deposit
/// * `min_a` - Minimum acceptable amount of token A
/// * `desired_b` - Desired amount of token B to deposit
/// * `min_b` - Minimum acceptable amount of token B
/// * `reserve_a` - Current reserve of token A in the pool
/// * `reserve_b` - Current reserve of token B in the pool
///
/// # Returns
/// A tuple (amount_a, amount_b) representing the actual deposit amounts
fn get_deposit_amounts(
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

#[contract]
pub struct LiquidityPool;

#[contractimpl]
impl LiquidityPool {
    /// Initializes the liquidity pool with two token addresses
    /// Token A must have an address less than Token B for deterministic ordering
    ///
    /// # Arguments
    /// * `e` - The environment
    /// * `token_a` - The first token contract address (must be < token_b)
    /// * `token_b` - The second token contract address (must be > token_a)
    ///
    /// # Panics
    /// Panics if token_a >= token_b
    pub fn __constructor(e: Env, token_a: Address, token_b: Address) {
        if token_a >= token_b {
            panic!("token_a must be less than token_b");
        }

        put_token_a(&e, token_a);
        put_token_b(&e, token_b);
        put_total_shares(&e, 0);
        put_reserve_a(&e, 0);
        put_reserve_b(&e, 0);
    }

    /// Returns the liquidity pool share balance for a given user
    ///
    /// # Arguments
    /// * `e` - The environment
    /// * `user` - The user address to query
    ///
    /// # Returns
    /// The amount of pool shares owned by the user
    pub fn balance_shares(e: Env, user: Address) -> i128 {
        get_shares(&e, &user)
    }

    /// Deposits tokens into the liquidity pool and mints pool shares
    /// The deposit ratio must match the current pool ratio to maintain balance
    /// For the first deposit (empty pool), any ratio is accepted
    ///
    /// # Arguments
    /// * `e` - The environment
    /// * `to` - The address depositing tokens (must authorize)
    /// * `desired_a` - Desired amount of token A to deposit
    /// * `min_a` - Minimum acceptable amount of token A
    /// * `desired_b` - Desired amount of token B to deposit
    /// * `min_b` - Minimum acceptable amount of token B
    ///
    /// # Panics
    /// * If calculated amounts are below minimum thresholds
    /// * If either deposit amount would be zero or negative
    pub fn deposit(
        e: Env,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    ) {
        // Depositor needs to authorize the deposit
        to.require_auth();

        let (reserve_a, reserve_b) = (get_reserve_a(&e), get_reserve_b(&e));

        // Calculate deposit amounts
        let (amount_a, amount_b) =
            get_deposit_amounts(desired_a, min_a, desired_b, min_b, reserve_a, reserve_b);

        if amount_a <= 0 || amount_b <= 0 {
            // If one of the amounts can be zero, we can get into a situation
            // where one of the reserves is 0, which leads to a divide by zero.
            panic!("both amounts must be strictly positive");
        }

        let token_a_client = token::Client::new(&e, &get_token_a(&e));
        let token_b_client = token::Client::new(&e, &get_token_b(&e));

        token_a_client.transfer(&to, &e.current_contract_address(), &amount_a);
        token_b_client.transfer(&to, &e.current_contract_address(), &amount_b);

        // Now calculate how many new pool shares to mint
        let (balance_a, balance_b) = (get_balance_a(&e), get_balance_b(&e));
        let total_shares = get_total_shares(&e);

        let zero = 0;
        let new_total_shares = if reserve_a > zero && reserve_b > zero {
            let shares_a = (balance_a * total_shares) / reserve_a;
            let shares_b = (balance_b * total_shares) / reserve_b;
            shares_a.min(shares_b)
        } else {
            (balance_a * balance_b).sqrt()
        };

        mint_shares(&e, &to, new_total_shares - total_shares);
        put_reserve_a(&e, balance_a);
        put_reserve_b(&e, balance_b);
    }

    /// Swaps tokens in the liquidity pool using a constant product formula with 0.3% fee
    /// The swap maintains the invariant (x * y = k) after accounting for fees
    ///
    /// # Arguments
    /// * `e` - The environment
    /// * `to` - The address executing the swap (must authorize)
    /// * `buy_a` - If true, buys token A and sells token B; if false, buys token B and sells token A
    /// * `out` - The exact amount of tokens to receive
    /// * `in_max` - Maximum amount of tokens willing to sell (slippage protection)
    ///
    /// # How it works
    /// 1. Calculates required sell amount based on constant product formula
    /// 2. Transfers sell tokens from user to contract
    /// 3. Validates the constant product invariant holds (accounting for 0.3% fee)
    /// 4. Transfers buy tokens from contract to user
    /// 5. Updates reserves
    ///
    /// # Panics
    /// * If there aren't enough tokens in the pool to buy
    /// * If the required sell amount exceeds in_max
    /// * If the constant product invariant doesn't hold
    /// * If resulting reserves would be zero or negative
    pub fn swap(e: Env, to: Address, buy_a: bool, out: i128, in_max: i128) {
        to.require_auth();

        let (reserve_a, reserve_b) = (get_reserve_a(&e), get_reserve_b(&e));
        let (reserve_sell, reserve_buy) = if buy_a {
            (reserve_b, reserve_a)
        } else {
            (reserve_a, reserve_b)
        };

        if reserve_buy < out {
            panic!("not enough token to buy");
        }

        // First calculate how much needs to be sold to buy amount out from the pool
        let n = reserve_sell * out * 1000;
        let d = (reserve_buy - out) * 997;
        let sell_amount = (n / d) + 1;
        if sell_amount > in_max {
            panic!("in amount is over max")
        }

        // Transfer the amount being sold to the contract
        let sell_token = if buy_a {
            get_token_b(&e)
        } else {
            get_token_a(&e)
        };
        let sell_token_client = token::Client::new(&e, &sell_token);
        sell_token_client.transfer(&to, &e.current_contract_address(), &sell_amount);

        let (balance_a, balance_b) = (get_balance_a(&e), get_balance_b(&e));

        // residue_numerator and residue_denominator are the amount that the invariant considers after
        // deducting the fee, scaled up by 1000 to avoid fractions
        let residue_numerator = 997;
        let residue_denominator = 1000;
        let zero = 0;

        let new_invariant_factor = |balance: i128, reserve: i128, out: i128| {
            let delta = balance - reserve - out;
            let adj_delta = if delta > zero {
                residue_numerator * delta
            } else {
                residue_denominator * delta
            };
            residue_denominator * reserve + adj_delta
        };

        let (out_a, out_b) = if buy_a { (out, 0) } else { (0, out) };

        let new_inv_a = new_invariant_factor(balance_a, reserve_a, out_a);
        let new_inv_b = new_invariant_factor(balance_b, reserve_b, out_b);
        let old_inv_a = residue_denominator * reserve_a;
        let old_inv_b = residue_denominator * reserve_b;

        if new_inv_a * new_inv_b < old_inv_a * old_inv_b {
            panic!("constant product invariant does not hold");
        }

        if buy_a {
            transfer_a(&e, to, out_a);
        } else {
            transfer_b(&e, to, out_b);
        }

        let new_reserve_a = balance_a - out_a;
        let new_reserve_b = balance_b - out_b;

        if new_reserve_a <= 0 || new_reserve_b <= 0 {
            panic!("new reserves must be strictly positive");
        }

        put_reserve_a(&e, new_reserve_a);
        put_reserve_b(&e, new_reserve_b);
    }

    /// Withdraws tokens from the liquidity pool by burning pool shares
    /// Returns a proportional amount of both tokens based on the share percentage
    ///
    /// # Arguments
    /// * `e` - The environment
    /// * `to` - The address withdrawing tokens (must authorize and own the shares)
    /// * `share_amount` - The number of pool shares to burn
    /// * `min_a` - Minimum acceptable amount of token A to receive
    /// * `min_b` - Minimum acceptable amount of token B to receive
    ///
    /// # Returns
    /// A tuple (amount_a, amount_b) representing the actual amounts withdrawn
    ///
    /// # How it works
    /// 1. Validates user has sufficient shares
    /// 2. Calculates proportional withdrawal amounts: (balance * shares) / total_shares
    /// 3. Validates amounts meet minimum thresholds
    /// 4. Burns the pool shares
    /// 5. Transfers both tokens to the user
    /// 6. Updates reserves
    ///
    /// # Panics
    /// * If user has insufficient shares
    /// * If withdrawal amounts are below minimum thresholds
    pub fn withdraw(
        e: Env,
        to: Address,
        share_amount: i128,
        min_a: i128,
        min_b: i128,
    ) -> (i128, i128) {
        to.require_auth();

        let current_shares = get_shares(&e, &to);
        if current_shares < share_amount {
            panic!("insufficient shares");
        }

        let (balance_a, balance_b) = (get_balance_a(&e), get_balance_b(&e));
        let total_shares = get_total_shares(&e);

        // Calculate withdrawal amounts
        let out_a = (balance_a * share_amount) / total_shares;
        let out_b = (balance_b * share_amount) / total_shares;

        if out_a < min_a || out_b < min_b {
            panic!("min not satisfied");
        }

        burn_shares(&e, &to, share_amount);
        transfer_a(&e, to.clone(), out_a);
        transfer_b(&e, to, out_b);
        put_reserve_a(&e, balance_a - out_a);
        put_reserve_b(&e, balance_b - out_b);

        (out_a, out_b)
    }

    /// Returns the current reserves of both tokens in the liquidity pool
    ///
    /// # Arguments
    /// * `e` - The environment
    ///
    /// # Returns
    /// A tuple (reserve_a, reserve_b) containing the current reserve amounts
    pub fn get_rsrvs(e: Env) -> (i128, i128) {
        (get_reserve_a(&e), get_reserve_b(&e))
    }
}

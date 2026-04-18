use crate::curve::{calc_trade, compute_rate_anchor, get_exchange_rate_from_trade};
use crate::transfers::{get_deposit_amounts, transfer_a, transfer_b, transfer_pt_from_pool_to_user, transfer_v_from_user_to_pool};
use crate::vault::{convert_assets_to_vault_shares, convert_vault_shares_to_assets};
use crate::storage::*;
use num_integer::Roots;
use amm_interface::AmmInterface;
use soroban_sdk::{contract, contractimpl, token, Address, Env};

const MINIMUM_LIQUIDITY: i128 = 100;
const BURN_ADDRESS: &str = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

#[contract]
pub struct LiquidityPool;

#[contractimpl]
impl LiquidityPool {
    /// Initializes the pool.
    ///
    /// # Arguments
    /// * `token_a` - First token address (must be < `token_b`)
    /// * `token_b` - Second token address (vault share token)
    /// * `expiry_ts` - Unix timestamp at which the market expires
    /// * `scalar_root` - Controls curve steepness (1e7-scaled)
    /// * `initial_anchor` - Initial curve anchor (1e7-scaled)
    /// * `fee_rate_root` - Fee rate root (1e7-scaled)
    /// * `last_implied_rate` - Initial implied rate (1e7-scaled)
    pub fn __constructor(
        e: Env,
        token_a: Address,
        token_b: Address,
        expiry_ts: u64,
        scalar_root: i128,
        initial_anchor: i128,
        fee_rate_root: i128,
        last_implied_rate: i128,
    ) {
let now = e.ledger().timestamp();
        assert!(expiry_ts > now, "expiry must be in the future");
        assert!(scalar_root > 0, "scalar_root must be positive");
        assert!(fee_rate_root > 0, "fee_rate_root must be positive");
        assert!(initial_anchor > 0, "initial_anchor must be positive");

        put_market_state(&e, &MarketState {
            token_a,
            token_b,
            reserve_a: 0,
            reserve_b: 0,
            expiry_ts,
            last_implied_rate,
            scalar_root,
            initial_anchor,
            fee_rate_root,
        });
        put_total_shares(&e, 0);
    }
}

#[contractimpl]
impl AmmInterface for LiquidityPool {
    /// Sell vault shares to receive an exact amount of PT from the pool.
    ///
    /// # Arguments
    /// * `to`       - Swapper address (must authorize)
    /// * `pt_out`   - Exact amount of PT to receive from the pool
    /// * `v_in_max` - Maximum vault shares willing to pay (slippage protection)
    fn swap_v_for_pt(e: Env, to: Address, pt_out: i128, v_in_max: i128) {
        to.require_auth();
        assert!(pt_out > 0);
        assert!(v_in_max > 0);

        let mut market = get_market_state(&e);
        let now = e.ledger().timestamp();
        assert!(now < market.expiry_ts);
        assert!(market.reserve_a >= pt_out); // TODO: > is safer since post-trade PT must remain positive

        let time_to_expiry = market.expiry_ts - now;
        let years = crate::math::seconds_to_years(time_to_expiry);

        // Convert vault share reserve to underlying assets for AMM pricing math.
        let reserve_b_assets = convert_vault_shares_to_assets(&e, market.reserve_b);

        let rate_scalar = crate::math::div_down(market.scalar_root, years);
        let fee_factor = crate::math::implied_rate_to_exchange_rate(market.fee_rate_root, time_to_expiry as i128);
        let rate_anchor = compute_rate_anchor(
            market.reserve_a,
            reserve_b_assets,
            market.last_implied_rate,
            rate_scalar,
            time_to_expiry as i128,
        );

        let (net_v_to_account, _fee, _reserve_fee) = calc_trade(
            market.reserve_a,
            reserve_b_assets,
            rate_scalar,
            rate_anchor,
            fee_factor,
            0, // reserve fee off for now
            pt_out,
        );

        // net_v_to_account is in asset units; convert to shares for the actual transfer.
        assert!(net_v_to_account < 0, "expected user to pay V in");
        let v_in_assets = net_v_to_account
            .checked_neg()
            .expect("overflow converting signed V flow");
        let v_in_shares = convert_assets_to_vault_shares(&e, v_in_assets);
        assert!(v_in_shares <= v_in_max, "in amount is over max");

        transfer_v_from_user_to_pool(&e, &to, v_in_shares);
        transfer_pt_from_pool_to_user(&e, &to, pt_out);

        // reserve_b stays in vault shares for LP accounting.
        market.reserve_b += v_in_shares;
        market.reserve_a -= pt_out;

        let new_exchange_rate = get_exchange_rate_from_trade(
            market.reserve_a,
            convert_vault_shares_to_assets(&e, market.reserve_b),
            rate_scalar,
            rate_anchor,
            0,
        );

        market.last_implied_rate =
            crate::math::exchange_rate_to_implied_rate(new_exchange_rate, years);

        put_market_state(&e, &market);
    }

    /// Sell an exact amount of PT into the pool and receive vault shares.
    ///
    /// # Arguments
    /// * `to`        - Swapper address (must authorize)
    /// * `pt_in`     - Exact amount of PT to sell into the pool
    /// * `min_v_out` - Minimum vault shares to receive (slippage protection)
    fn swap_pt_for_v(e: Env, to: Address, pt_in: i128, min_v_out: i128) {
        to.require_auth();
        assert!(pt_in > 0, "pt_in must be positive");
        assert!(min_v_out > 0, "min_v_out must be positive");

        let mut market = get_market_state(&e);
        let now = e.ledger().timestamp();
        assert!(now < market.expiry_ts, "market expired");

        let t_secs = (market.expiry_ts - now) as i128;
        assert!(t_secs > 0, "time to expiry must be positive");

        let t_years = crate::math::seconds_to_years(market.expiry_ts - now);
        assert!(t_years > 0, "time to expiry in years must be positive");

        // Convert vault share reserve to underlying assets for AMM pricing math.
        let reserve_b_assets = convert_vault_shares_to_assets(&e, market.reserve_b);

        let rate_scalar = crate::math::div_down(market.scalar_root, t_years);
        let fee_factor = crate::math::implied_rate_to_exchange_rate(market.fee_rate_root, t_secs);

        let rate_anchor = compute_rate_anchor(
            market.reserve_a,
            reserve_b_assets,
            market.last_implied_rate,
            rate_scalar,
            t_secs,
        );

        // Pendle sign convention: negative means PT comes FROM the user INTO the pool
        let net_pt_to_account = -pt_in;

        let (net_v_to_account, _net_v_fee, _net_v_to_reserve) = calc_trade(
            market.reserve_a,
            reserve_b_assets,
            rate_scalar,
            rate_anchor,
            fee_factor,
            0, // reserve_fee_percent — replace when treasury is wired up
            net_pt_to_account,
        );

        assert!(net_v_to_account > 0, "expected positive V flow to account for PT-in trade");

        // net_v_to_account is in asset units; convert to shares for transfer and slippage check.
        let v_out_assets = net_v_to_account;
        let v_out_shares = convert_assets_to_vault_shares(&e, v_out_assets);
        assert!(v_out_shares >= min_v_out, "out amount below minimum");
        assert!(market.reserve_b > v_out_shares, "insufficient V liquidity");

        let new_reserve_a = market.reserve_a
            .checked_add(pt_in)
            .expect("overflow updating PT reserve");
        // reserve_b stays in vault shares for LP accounting.
        let new_reserve_b = market.reserve_b
            .checked_sub(v_out_shares)
            .expect("underflow updating V reserve");

        assert!(new_reserve_a > 0 && new_reserve_b > 0, "new reserves must be strictly positive");

        let pt_client = token::TokenClient::new(&e, &market.token_a);
        pt_client.transfer(&to, &e.current_contract_address(), &pt_in);
        transfer_b(&e, to, v_out_shares);

        market.reserve_a = new_reserve_a;
        market.reserve_b = new_reserve_b;

        let ex_rate = get_exchange_rate_from_trade(
            market.reserve_a,
            convert_vault_shares_to_assets(&e, market.reserve_b),
            rate_scalar,
            rate_anchor,
            0,
        );

        market.last_implied_rate =
            crate::math::exchange_rate_to_implied_rate(ex_rate, t_years);

        put_market_state(&e, &market);
    }

    /// Deposits tokens into the pool and mints shares. Deposit ratio must match
    /// the current pool ratio (any ratio accepted if pool is empty).
    ///
    /// # Arguments
    /// * `to` - Depositor address (must authorize)
    /// * `desired_a` - Desired amount of token A (PT)
    /// * `min_a` - Minimum acceptable amount of token A
    /// * `desired_b` - Desired amount of token B (vault shares)
    /// * `min_b` - Minimum acceptable amount of token B
    fn deposit(
        e: Env,
        to: Address,
        desired_a: i128,
        min_a: i128,
        desired_b: i128,
        min_b: i128,
    ) {
        to.require_auth();

        let mut market = get_market_state(&e);

        let (amount_a, amount_b) =
            get_deposit_amounts(desired_a, min_a, desired_b, min_b, market.reserve_a, market.reserve_b);

        if amount_a <= 0 || amount_b <= 0 {
            panic!("both amounts must be strictly positive");
        }

        let token_a_client = token::TokenClient::new(&e, &market.token_a);
        let token_b_client = token::TokenClient::new(&e, &market.token_b);

        token_a_client.transfer(&to, &e.current_contract_address(), &amount_a);
        token_b_client.transfer(&to, &e.current_contract_address(), &amount_b);

        let (balance_a, balance_b) = (get_balance_a(&e), get_balance_b(&e));
        let total_shares = get_total_shares(&e);

        let zero = 0;
        let new_total_shares = if total_shares == zero {
            (amount_a * amount_b).sqrt()
        } else if market.reserve_a > zero && market.reserve_b > zero {
            let shares_a = (balance_a * total_shares) / market.reserve_a;
            let shares_b = (balance_b * total_shares) / market.reserve_b;
            shares_a.min(shares_b)
        } else {
            panic!("reserves are empty but shares exist");
        };

        let shares_to_mint = new_total_shares - total_shares;
        if total_shares == zero {
            let burn_address = Address::from_str(&e, BURN_ADDRESS);
            mint_shares(&e, &burn_address, MINIMUM_LIQUIDITY);
            mint_shares(&e, &to, shares_to_mint - MINIMUM_LIQUIDITY);
        } else {
            mint_shares(&e, &to, shares_to_mint);
        }

        market.reserve_a = balance_a;
        market.reserve_b = balance_b;
        put_market_state(&e, &market);
    }

    /// Burns pool shares and withdraws a proportional amount of both tokens.
    ///
    /// # Arguments
    /// * `to`           - Withdrawer address (must authorize and own the shares)
    /// * `share_amount` - Number of pool shares to burn
    /// * `min_a`        - Minimum acceptable amount of token A (PT)
    /// * `min_b`        - Minimum acceptable amount of token B (vault shares)
    ///
    /// # Returns
    /// `(amount_a, amount_b)` actually withdrawn
    fn withdraw(
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

        let mut market = get_market_state(&e);
        let (balance_a, balance_b) = (get_balance_a(&e), get_balance_b(&e));
        let total_shares = get_total_shares(&e);

        let out_a = (balance_a * share_amount) / total_shares;
        let out_b = (balance_b * share_amount) / total_shares;

        if out_a < min_a || out_b < min_b {
            panic!("min not satisfied");
        }

        burn_shares(&e, &to, share_amount);
        transfer_a(&e, to.clone(), out_a);
        transfer_b(&e, to, out_b);

        market.reserve_a = balance_a - out_a;
        market.reserve_b = balance_b - out_b;
        put_market_state(&e, &market);

        (out_a, out_b)
    }

    /// Returns the current reserves of both tokens.
    ///
    /// # Returns
    /// `(reserve_pt, reserve_v)` — PT reserve and vault share reserve
    fn get_reserves(e: Env) -> (i128, i128) {
        let market = get_market_state(&e);
        (market.reserve_a, market.reserve_b)
    }

    /// Returns the pool share balance for a given user.
    fn balance_shares(e: Env, user: Address) -> i128 {
        get_shares(&e, &user)
    }
}
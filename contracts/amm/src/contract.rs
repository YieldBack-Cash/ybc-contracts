use crate::math::implied_rate_to_exchange_rate;
use crate::transfers::{get_deposit_amounts, transfer_a, transfer_b};
use crate::storage::*;
use num_integer::Roots;
use soroban_fixed_point_math::SorobanFixedPoint;
use soroban_sdk::{contract, contractimpl, token, Address, Env};
use vault_interface::VaultContractClient;

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
    /// * `token_b` - Second token address
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
        if token_a >= token_b {
            panic!("token_a must be less than token_b");
        }
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

    /// Returns the pool share balance for a given user.
    ///
    /// # Arguments
    /// * `user` - Address to query
    ///
    /// # Returns
    /// Pool shares owned by the user
    pub fn balance_shares(e: Env, user: Address) -> i128 {
        get_shares(&e, &user)
    }

    /// Deposits tokens into the pool and mints shares. Deposit ratio must match
    /// the current pool ratio (any ratio accepted if pool is empty).
    ///
    /// # Arguments
    /// * `to` - Depositor address (must authorize)
    /// * `desired_a` - Desired amount of token A
    /// * `min_a` - Minimum acceptable amount of token A
    /// * `desired_b` - Desired amount of token B
    /// * `min_b` - Minimum acceptable amount of token B
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

        let mut market = get_market_state(&e);

        // Calculate deposit amounts
        let (amount_a, amount_b) =
            get_deposit_amounts(desired_a, min_a, desired_b, min_b, market.reserve_a, market.reserve_b);

        if amount_a <= 0 || amount_b <= 0 {
            // If one of the amounts can be zero, we can get into a situation
            // where one of the reserves is 0, which leads to a divide by zero.
            panic!("both amounts must be strictly positive");
        }

        let token_a_client = token::Client::new(&e, &market.token_a);
        let token_b_client = token::Client::new(&e, &market.token_b);

        token_a_client.transfer(&to, &e.current_contract_address(), &amount_a);
        token_b_client.transfer(&to, &e.current_contract_address(), &amount_b);

        // Now calculate how many new pool shares to mint
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

    /// Sell vault shares to receive an exact amount of PT from the pool.
    /// Uses Pendle-style signed trade math (`calc_trade` with positive `net_pt_to_account`).
    ///
    /// # Arguments
    /// * `to`       - Swapper address (must authorize)
    /// * `pt_out`   - Exact amount of PT to receive from the pool
    /// * `v_in_max` - Maximum vault shares willing to pay (slippage protection)
    pub fn swap_v_for_pt(e: Env, to: Address, pt_out: i128, v_in_max: i128) {
        to.require_auth();
        assert!(pt_out > 0);
        assert!(v_in_max > 0);

        let mut market = get_market_state(&e);
        let now = e.ledger().timestamp();
        assert!(now < market.expiry_ts);
        assert!(market.reserve_a >= pt_out); //todo: using > might be saver since the math needs the post-trade PT to remain positive

        let time_to_expiry = market.expiry_ts - now;
        let years = crate::math::seconds_to_years(time_to_expiry);

        // Convert vault share reserve to underlying assets for AMM pricing math.
        let reserve_b_assets = convert_vault_shares_to_assets(&e, market.reserve_b);

        let rate_scalar = crate::math::div_down(market.scalar_root, years);
        let fee_rate = crate::math::div_down(market.fee_rate_root, years);
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
            fee_rate,
            0,      // reserve fee off for now
            pt_out, // positive => PT to user
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
    /// Uses Pendle-style signed trade math (`calc_trade` with negative `net_pt_to_account`).
    ///
    /// # Arguments
    /// * `to`         - Swapper address (must authorize)
    /// * `pt_in`      - Exact amount of PT to sell into the pool
    /// * `min_v_out`  - Minimum vault shares to receive (slippage protection)
    pub fn swap_pt_for_v(e: Env, to: Address, pt_in: i128, min_v_out: i128) {
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
        let fee_rate = crate::math::div_down(market.fee_rate_root, t_years);

        let rate_anchor = compute_rate_anchor(
            market.reserve_a,
            reserve_b_assets,
            market.last_implied_rate,
            rate_scalar,
            t_secs,
        );

        // Pendle sign convention:
        // negative means PT comes FROM the user INTO the pool
        let net_pt_to_account = -pt_in;

        let (net_v_to_account, _net_v_fee, _net_v_to_reserve) = calc_trade(
            market.reserve_a,
            reserve_b_assets,
            rate_scalar,
            rate_anchor,
            fee_rate,
            0, // reserve_fee_percent for now; replace later if you add treasury cut
            net_pt_to_account,
        );

        assert!(
            net_v_to_account > 0,
            "expected positive V flow to account for PT-in trade"
        );

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

        assert!(
            new_reserve_a > 0 && new_reserve_b > 0,
            "new reserves must be strictly positive"
        );

        let pt_client = token::Client::new(&e, &market.token_a);
        pt_client.transfer(&to, &e.current_contract_address(), &pt_in);
        transfer_b(&e, to, v_out_shares);

        market.reserve_a = new_reserve_a;
        market.reserve_b = new_reserve_b;

        let ex_rate = get_exchange_rate_from_trade(
            market.reserve_a,
            convert_vault_shares_to_assets(&e, market.reserve_b),
            rate_scalar,
            rate_anchor,
            0, // final state quote
        );

        market.last_implied_rate =
            crate::math::exchange_rate_to_implied_rate(ex_rate, t_years);

        put_market_state(&e, &market);
    }
    

    /// Burns pool shares and withdraws a proportional amount of both tokens.
    ///
    /// # Arguments
    /// * `to` - Withdrawer address (must authorize and own the shares)
    /// * `share_amount` - Number of pool shares to burn
    /// * `min_a` - Minimum acceptable amount of token A
    /// * `min_b` - Minimum acceptable amount of token B
    ///
    /// # Returns
    /// `(amount_a, amount_b)` actually withdrawn
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

        let mut market = get_market_state(&e);
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

        market.reserve_a = balance_a - out_a;
        market.reserve_b = balance_b - out_b;
        put_market_state(&e, &market);

        (out_a, out_b)
    }

    /// Returns the current reserves of both tokens.
    ///
    /// # Returns
    /// `(reserve_pt, reserve_v)` — PT reserve and vault share reserve
    pub fn get_rsrvs(e: Env) -> (i128, i128) {
        let market = get_market_state(&e);
        (market.reserve_a, market.reserve_b)
    }
}



/// Computes the curve anchor such that the AMM prices at `last_implied_rate` at the current reserves.
///
/// Mirrors Pendle's `_getRateAnchor`:
///   `rate_anchor = exchange_rate(implied_rate, t) - ln(proportion / (1 - proportion)) / rate_scalar`
///
/// # Arguments
/// - `reserve_pt`          — current PT reserve
/// - `reserve_v`           — current V reserve
/// - `last_implied_rate`   — current stored implied rate (1e7-scaled)
/// - `rate_scalar`         — time-adjusted curve steepness (1e7-scaled)
/// - `time_to_expiry_secs` — seconds until market expiry (plain, not scaled)
fn compute_rate_anchor(
    reserve_pt: i128,
    reserve_v: i128,
    last_implied_rate: i128,
    rate_scalar: i128,
    time_to_expiry_secs: i128,
) -> i128 {
    assert!(reserve_pt > 0, "reserve_pt must be positive");
    assert!(reserve_v > 0, "reserve_v must be positive");
    assert!(last_implied_rate >= 0, "last_implied_rate must be non-negative");
    assert!(rate_scalar > 0, "rate_scalar must be positive");
    assert!(time_to_expiry_secs > 0, "time_to_expiry_secs must be positive");

    // Pendle-style:
    // new_exchange_rate = exp(implied_rate * t)
    let new_exchange_rate =
        crate::math::implied_rate_to_exchange_rate(last_implied_rate, time_to_expiry_secs);

    assert!(new_exchange_rate > 0, "exchange rate must be positive");

    // proportion = PT / (PT + V)
    let total = reserve_pt
        .checked_add(reserve_v)
        .expect("overflow computing total reserves");

    let proportion = reserve_pt
        .checked_mul(crate::math::FP_SCALE)
        .expect("overflow scaling reserve_pt")
        / total;

    assert!(
        proportion > 0 && proportion < crate::math::FP_SCALE,
        "proportion must be between 0 and 1"
    );

    // ln( proportion / (1 - proportion) )
    // This is equivalent to ln(reserve_pt / reserve_v), but written
    // the same way as Pendle conceptually does it.
    let one_minus_p = crate::math::FP_SCALE - proportion;

    let ratio = proportion
        .checked_mul(crate::math::FP_SCALE)
        .expect("overflow computing proportion ratio")
        / one_minus_p;

    let ln_proportion = crate::math::ln_fp(ratio, crate::math::FP_SCALE);

    // rate_anchor = new_exchange_rate - lnProportion / rateScalar
    let adjustment = ln_proportion
        .checked_mul(crate::math::FP_SCALE)
        .expect("overflow computing anchor adjustment")
        / rate_scalar;

    let rate_anchor = new_exchange_rate
        .checked_sub(adjustment)
        .expect("underflow computing rate anchor");

    assert!(rate_anchor > 0, "rate anchor must be positive");

    rate_anchor
}


/// Computes the exchange rate at the post-trade PT reserve position.
///
/// Mirrors Pendle's `_getExchangeRate(netPtToAccount)`:
///   post-trade PT = `total_pt - net_pt_to_account`
///   exchange_rate = `ln(proportion / (1 - proportion)) / rate_scalar + rate_anchor`
///
/// Pass `net_pt_to_account = 0` to get the exchange rate at the current reserve state
/// (used for updating `last_implied_rate` after a trade settles).
///
/// # Arguments
/// - `total_pt`           — current PT reserve (pre-trade)
/// - `total_v`            — current V reserve
/// - `rate_scalar`        — time-adjusted curve steepness (1e7-scaled)
/// - `rate_anchor`        — curve anchor (1e7-scaled)
/// - `net_pt_to_account`  — signed PT flow to user; positive = PT out, negative = PT in
fn get_exchange_rate_from_trade(
    total_pt: i128,
    total_v: i128,
    rate_scalar: i128,
    rate_anchor: i128,
    net_pt_to_account: i128,
) -> i128 {
    assert!(total_pt > 0, "total_pt must be positive");
    assert!(total_v > 0, "total_v must be positive");
    assert!(rate_scalar > 0, "rate_scalar must be positive");
    assert!(rate_anchor > 0, "rate_anchor must be positive");

    // Post-trade PT reserve:
    let numerator = total_pt
        .checked_sub(net_pt_to_account)
        .expect("PT reserve underflow");

    assert!(numerator > 0, "post-trade PT reserve must be positive");

    // proportion = post_trade_pt / (post_trade_pt + total_v)
    let denom = numerator
        .checked_add(total_v)
        .expect("overflow computing total pool value");

    let proportion = numerator
        .checked_mul(crate::math::FP_SCALE)
        .expect("overflow scaling proportion")
        / denom;

    assert!(
        proportion > 0 && proportion < crate::math::FP_SCALE,
        "proportion must be between 0 and 1"
    );

    let one_minus_p = crate::math::FP_SCALE - proportion;

    let ratio = proportion
        .checked_mul(crate::math::FP_SCALE)
        .expect("overflow computing ratio")
        / one_minus_p;

    let ln_proportion = crate::math::ln_fp(ratio, crate::math::FP_SCALE);

    let adjustment = ln_proportion
        .checked_mul(crate::math::FP_SCALE)
        .expect("overflow computing adjustment")
        / rate_scalar;

    let exchange_rate = rate_anchor
        .checked_add(adjustment)
        .expect("overflow computing exchange rate");

    assert!(exchange_rate > 0, "exchange rate must be positive");
    exchange_rate
}

/// Core trade pricing, mirroring Pendle's `calcTrade`.
///
/// # Arguments
/// - `reserve_pt`          — current PT reserve
/// - `reserve_v`           — current V (vault share) reserve
/// - `rate_scalar`         — time-adjusted curve steepness (1e7-scaled)
/// - `rate_anchor`         — curve anchor, derived from current implied rate (1e7-scaled)
/// - `fee_rate`            — time-adjusted fee rate (1e7-scaled)
/// - `reserve_fee_percent` — fraction of fee that goes to the reserve, as a whole-number percent (0–100)
/// - `net_pt_to_account`   — signed PT flow to the user: positive = PT out to user, negative = PT in from user
///
/// # Returns
/// `(net_v_to_account, net_v_fee, net_v_to_reserve)`
/// - `net_v_to_account` — signed V flow to the user: negative = user pays V in, positive = user receives V out
/// - `net_v_fee`        — fee magnitude in V units (always non-negative)
/// - `net_v_to_reserve` — portion of the fee credited to the reserve (always non-negative)
fn calc_trade(
    reserve_pt: i128,
    reserve_v: i128,
    rate_scalar: i128,
    rate_anchor: i128,
    fee_rate: i128,
    reserve_fee_percent: i128,
    net_pt_to_account: i128,
) -> (i128, i128, i128) {
    assert!(reserve_pt > 0, "reserve_pt must be positive");
    assert!(reserve_v > 0, "reserve_v must be positive");
    assert!(rate_scalar > 0, "rate_scalar must be positive");
    assert!(rate_anchor > 0, "rate_anchor must be positive");
    assert!(fee_rate > 0, "fee_rate must be positive");
    assert!(
        fee_rate <= crate::math::FP_SCALE,
        "fee_rate must be <= 1.0 in fixed-point scale"
    );
    assert!(
        reserve_fee_percent >= 0 && reserve_fee_percent <= 100,
        "reserve_fee_percent must be between 0 and 100"
    );

    // Pendle-style liquidity check:
    // if PT is going to the account, pool must still have PT left after the trade
    if net_pt_to_account > 0 {
        assert!(
            reserve_pt > net_pt_to_account,
            "insufficient PT liquidity for trade"
        );
    }

    // 1) Pre-fee exchange rate from the POST-trade PT position.
    let pre_fee_exchange_rate = get_exchange_rate_from_trade(
        reserve_pt,
        reserve_v,
        rate_scalar,
        rate_anchor,
        net_pt_to_account,
    );

    assert!(
        pre_fee_exchange_rate > 0,
        "pre_fee_exchange_rate must be positive"
    );

    // 2) Signed pre-fee V flow to the account.
    //
    // Pendle:
    // preFeeAssetToAccount = netPtToAccount.divDown(preFeeExchangeRate).neg()
    //
    // Sign meaning:
    //   > 0 : V to user
    //   < 0 : V from user into pool
    let pre_fee_v_to_account = crate::math::div_down(net_pt_to_account, pre_fee_exchange_rate)
        .checked_neg()
        .expect("overflow negating pre-fee V flow");

    let one = crate::math::FP_SCALE;
    let one_minus_fee = one
        .checked_sub(fee_rate)
        .expect("fee_rate exceeds FP_SCALE");

    // 3) Direction-sensitive fee logic.
    //
    // Keep the same shape as Pendle:
    // - PT out branch: user pays V in
    // - PT in branch: user receives V out
    let net_v_fee = if net_pt_to_account > 0 {
        // PT out to user => user pays V in => pre_fee_v_to_account is negative.
        //
        // Pendle branch:
        // fee = preFeeAssetToAccount.mulDown(1 - feeRate)
        //
        // We return fee as a positive magnitude in V units.
        let raw_v_in = pre_fee_v_to_account
            .checked_neg()
            .expect("overflow converting raw_v_in");

        crate::math::mul_down(raw_v_in, one_minus_fee)
    } else {
        // PT in from user => user gets V out => pre_fee_v_to_account is positive.
        //
        // Pendle branch:
        // fee = ((preFeeAssetToAccount * (1 - feeRate)) / feeRate).neg()
        //
        // We return fee as a positive magnitude in V units.
        let numerator = crate::math::mul_down(pre_fee_v_to_account, one_minus_fee);
        crate::math::div_down(numerator, fee_rate)
    };

    assert!(net_v_fee >= 0, "net_v_fee must be non-negative");

    // 4) Reserve fee split.
    // Pendle uses reserveFeePercent base-100.
    let net_v_to_reserve = (net_v_fee * reserve_fee_percent) / 100;
    assert!(net_v_to_reserve >= 0, "net_v_to_reserve must be non-negative");

    // 5) Final signed V flow to account.
    //
    // Pendle:
    // netAssetToAccount = preFeeAssetToAccount - fee
    //
    // Since fee is treated here as a positive magnitude, subtracting it works naturally
    // in the PT-in branch (user receives less V), but for PT-out we want the user to pay
    // MORE V in, which means the signed amount becomes more negative.
    let net_v_to_account = if net_pt_to_account > 0 {
        // user pays V in, so make the signed flow more negative by fee
        pre_fee_v_to_account
            .checked_sub(net_v_fee)
            .expect("overflow computing net_v_to_account in PT-out branch")
    } else {
        // user receives V out, so reduce the positive amount by fee
        pre_fee_v_to_account
            .checked_sub(net_v_fee)
            .expect("overflow computing net_v_to_account in PT-in branch")
    };

    (net_v_to_account, net_v_fee, net_v_to_reserve)
}


/// Returns the value of `shares` vault shares denominated in the underlying asset.
fn convert_vault_shares_to_assets(e: &Env, shares: i128) -> i128 {
    let market = get_market_state(e);
    let client = VaultContractClient::new(e, &market.token_b);
    client.convert_to_assets(&shares)
}

/// Returns the number of vault shares equivalent to `assets` units of the underlying asset.
/// Uses the per-share rate from `convert_to_assets(1)` and performs integer division.
fn convert_assets_to_vault_shares(e: &Env, assets: i128) -> i128 {
    let market = get_market_state(e);
    let client = VaultContractClient::new(e, &market.token_b);
    let rate = client.convert_to_assets(&1i128);
    assets / rate
}

/// Returns the current vault share balance of the pool converted to underlying asset units.
fn get_asset_balance_b(e: &Env) -> i128 {
    convert_vault_shares_to_assets(e, get_balance_b(e))
}

fn transfer_v_from_user_to_pool(e: &Env, to: &Address, v_in: i128) {
    let market = get_market_state(e);
    token::Client::new(e, &market.token_b).transfer(to, &e.current_contract_address(), &v_in);
}

fn transfer_pt_from_pool_to_user(e: &Env, to: &Address, pt_out: i128) {
    transfer_a(e, to.clone(), pt_out);
}
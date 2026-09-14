use crate::math;
use amm_interface::AmmError;

/// Bounds on the post-trade pool proportion PT / (PT + V).
///
/// The upper bound mirrors Pendle's `MAX_MARKET_PROPORTION` (96%): near p = 1 the
/// `ln(p / (1 - p))` term diverges and PT prices above face value. The lower bound
/// guards the V-heavy side, where integer truncation drives `proportion` (and then
/// the ratio fed to `ln_fp`) to zero, which would panic and brick the pool.
pub(crate) const MIN_PROPORTION: i128 = math::FP_SCALE / 100; // 0.01
pub(crate) const MAX_PROPORTION: i128 = 96 * math::FP_SCALE / 100; // 0.96

/// Computes the curve anchor such that the AMM prices at `last_implied_rate` at the current reserves.
///
/// Mirrors Pendle's `_getRateAnchor`:
///   `rate_anchor = exchange_rate(implied_rate, t) - ln(proportion / (1 - proportion)) / rate_scalar`
///
/// # Arguments
/// - `reserve_pt`          — current PT reserve
/// - `reserve_v`           — current V reserve (in underlying asset units)
/// - `last_implied_rate`   — current stored implied rate (1e7-scaled)
/// - `rate_scalar`         — time-adjusted curve steepness (1e7-scaled)
/// - `time_to_expiry_secs` — seconds until market expiry (plain, not scaled)
pub(crate) fn compute_rate_anchor(
    reserve_pt: i128,
    reserve_v: i128,
    last_implied_rate: i128,
    rate_scalar: i128,
    time_to_expiry_secs: i128,
) -> Result<i128, AmmError> {
    if reserve_pt <= 0 || reserve_v <= 0 {
        return Err(AmmError::EmptyPool);
    }
    if last_implied_rate < 0 || rate_scalar <= 0 || time_to_expiry_secs <= 0 {
        return Err(AmmError::InvalidPoolState);
    }

    // new_exchange_rate = exp(implied_rate * t)
    let new_exchange_rate =
        math::implied_rate_to_exchange_rate(last_implied_rate, time_to_expiry_secs);

    if new_exchange_rate <= 0 {
        return Err(AmmError::InvalidPoolState);
    }

    // proportion = PT / (PT + V)
    let total = reserve_pt
        .checked_add(reserve_v)
        .ok_or(AmmError::MathOverflow)?;

    let proportion = reserve_pt
        .checked_mul(math::FP_SCALE)
        .ok_or(AmmError::MathOverflow)?
        / total;

    if proportion <= 0 || proportion >= math::FP_SCALE {
        return Err(AmmError::ProportionOutOfBounds);
    }

    let one_minus_p = math::FP_SCALE - proportion;

    let ratio = proportion
        .checked_mul(math::FP_SCALE)
        .ok_or(AmmError::MathOverflow)?
        / one_minus_p;

    let ln_proportion = math::ln_fp(ratio, math::FP_SCALE);

    let adjustment = ln_proportion
        .checked_mul(math::FP_SCALE)
        .ok_or(AmmError::MathOverflow)?
        / rate_scalar;

    let rate_anchor = new_exchange_rate
        .checked_sub(adjustment)
        .ok_or(AmmError::MathOverflow)?;

    if rate_anchor <= 0 {
        return Err(AmmError::InvalidPoolState);
    }

    Ok(rate_anchor)
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
/// - `total_v`            — current V reserve (in underlying asset units)
/// - `rate_scalar`        — time-adjusted curve steepness (1e7-scaled)
/// - `rate_anchor`        — curve anchor (1e7-scaled)
/// - `net_pt_to_account`  — signed PT flow to user; positive = PT out, negative = PT in
pub(crate) fn get_exchange_rate_from_trade(
    total_pt: i128,
    total_v: i128,
    rate_scalar: i128,
    rate_anchor: i128,
    net_pt_to_account: i128,
) -> Result<i128, AmmError> {
    if total_pt <= 0 || total_v <= 0 {
        return Err(AmmError::EmptyPool);
    }
    if rate_scalar <= 0 || rate_anchor <= 0 {
        return Err(AmmError::InvalidPoolState);
    }

    // Post-trade PT reserve:
    let numerator = total_pt
        .checked_sub(net_pt_to_account)
        .ok_or(AmmError::MathOverflow)?;

    if numerator <= 0 {
        return Err(AmmError::InsufficientPtLiquidity);
    }

    // proportion = post_trade_pt / (total_pt + total_v)
    // The denominator is the pre-trade reserve sum, so buy and sell legs price
    // against the same total. Mirrors Pendle's _getExchangeRate; using the
    // post-trade PT here instead leaks value to round-trip trades.
    let denom = total_pt
        .checked_add(total_v)
        .ok_or(AmmError::MathOverflow)?;

    let proportion = numerator
        .checked_mul(math::FP_SCALE)
        .ok_or(AmmError::MathOverflow)?
        / denom;

    // With the pre-trade denominator, proportion is not bounded below 1.0 by
    // construction: a PT-in trade larger than the pool total pushes it past
    // FP_SCALE, which would make one_minus_p negative. The MAX_PROPORTION bound
    // is what excludes that state — do not loosen it.
    if proportion < MIN_PROPORTION || proportion > MAX_PROPORTION {
        return Err(AmmError::ProportionOutOfBounds);
    }

    let one_minus_p = math::FP_SCALE - proportion;

    let ratio = proportion
        .checked_mul(math::FP_SCALE)
        .ok_or(AmmError::MathOverflow)?
        / one_minus_p;

    let ln_proportion = math::ln_fp(ratio, math::FP_SCALE);

    let adjustment = ln_proportion
        .checked_mul(math::FP_SCALE)
        .ok_or(AmmError::MathOverflow)?
        / rate_scalar;

    let exchange_rate = rate_anchor
        .checked_add(adjustment)
        .ok_or(AmmError::MathOverflow)?;

    // A rate below 1.0 would price PT above face value (negative yield) and,
    // worse, store a negative implied rate that bricks the pool: every later
    // compute_rate_anchor call rejects last_implied_rate < 0. Mirrors
    // Pendle's MarketExchangeRateBelowOne revert.
    if exchange_rate < math::FP_SCALE {
        return Err(AmmError::ExchangeRateBelowOne);
    }
    Ok(exchange_rate)
}

/// Core trade pricing for the yield AMM curve.
///
/// # Arguments
/// - `reserve_pt`          — current PT reserve
/// - `reserve_v`           — current V reserve (in underlying asset units)
/// - `rate_scalar`         — time-adjusted curve steepness (1e7-scaled)
/// - `rate_anchor`         — curve anchor, derived from current implied rate (1e7-scaled)
/// - `fee_factor`          — time-aware fee multiplier: `e^(fee_rate_root * t)`, always >= 1.0 (1e7-scaled)
/// - `reserve_fee_rate`    — fraction of the fee that goes to the reserve, 1e7-scaled (e.g. 1_000_000 = 10%)
/// - `net_pt_to_account`   — signed PT flow to the user: positive = PT out to user, negative = PT in from user
///
/// # Returns
/// `(net_v_to_account, net_v_fee, net_v_to_reserve)` — all V amounts in underlying asset units
/// - `net_v_to_account` — signed V flow to the user: negative = user pays V in, positive = user receives V out
/// - `net_v_fee`        — fee magnitude in V units (always non-negative)
/// - `net_v_to_reserve` — portion of the fee credited to the reserve (always non-negative)
pub(crate) fn calc_trade(
    reserve_pt: i128,
    reserve_v: i128,
    rate_scalar: i128,
    rate_anchor: i128,
    fee_factor: i128,
    reserve_fee_rate: i128,
    net_pt_to_account: i128,
) -> Result<(i128, i128, i128), AmmError> {
    if reserve_pt <= 0 || reserve_v <= 0 {
        return Err(AmmError::EmptyPool);
    }
    if rate_scalar <= 0
        || rate_anchor <= 0
        || fee_factor < math::FP_SCALE
        || reserve_fee_rate < 0
        || reserve_fee_rate > math::FP_SCALE
    {
        return Err(AmmError::InvalidPoolState);
    }

    // Pendle-style liquidity check:
    // if PT is going to the account, pool must still have PT left after the trade
    if net_pt_to_account > 0 && reserve_pt <= net_pt_to_account {
        return Err(AmmError::InsufficientPtLiquidity);
    }

    // 1) Pre-fee exchange rate from the POST-trade PT position.
    let pre_fee_exchange_rate = get_exchange_rate_from_trade(
        reserve_pt,
        reserve_v,
        rate_scalar,
        rate_anchor,
        net_pt_to_account,
    )?;

    // 2) Signed pre-fee V flow to the account.
    //
    // preFeeAssetToAccount = netPtToAccount.divDown(preFeeExchangeRate).neg()
    //
    // Sign meaning:
    //   > 0 : V to user
    //   < 0 : V from user into pool
    let pre_fee_v_to_account = math::div_down(net_pt_to_account, pre_fee_exchange_rate)
        .checked_neg()
        .ok_or(AmmError::MathOverflow)?;

    // 3) Direction-sensitive fee logic using fee_factor (>= 1.0).
    //    fee_factor shrinks toward 1.0 as expiry approaches, so fees decay to zero.
    //
    // PT out: user pays MORE V in  — multiply raw V cost by fee_factor.
    // PT in:  user receives LESS V — divide raw V payout by fee_factor.
    let (net_v_to_account, net_v_fee) = if net_pt_to_account > 0 {
        // pre_fee_v_to_account < 0 (user pays V in).
        let raw_v_in = pre_fee_v_to_account
            .checked_neg()
            .ok_or(AmmError::MathOverflow)?;
        let actual_v_in = math::mul_down(raw_v_in, fee_factor);
        let fee = actual_v_in
            .checked_sub(raw_v_in)
            .ok_or(AmmError::MathOverflow)?;
        (actual_v_in.checked_neg().ok_or(AmmError::MathOverflow)?, fee)
    } else {
        // pre_fee_v_to_account > 0 (user receives V out).
        let raw_v_out = pre_fee_v_to_account;
        let actual_v_out = math::div_down(raw_v_out, fee_factor);
        let fee = raw_v_out
            .checked_sub(actual_v_out)
            .ok_or(AmmError::MathOverflow)?;
        (actual_v_out, fee)
    };

    if net_v_fee < 0 {
        return Err(AmmError::InvalidPoolState);
    }

    // 4) Reserve fee split (1e7-scaled fraction of the fee), floored so the
    //    rounding dust stays with the LPs.
    let net_v_to_reserve = math::mul_down(net_v_fee, reserve_fee_rate);

    Ok((net_v_to_account, net_v_fee, net_v_to_reserve))
}

#[cfg(test)]
mod proportion_bounds_tests {
    use super::*;

    const RESERVE: i128 = 100_000_000; // 10 units at 1e7 scale
    const RATE_SCALAR: i128 = 5 * math::FP_SCALE;
    const RATE_ANCHOR: i128 = 11_000_000; // 1.1

    #[test]
    fn balanced_trade_within_bounds_succeeds() {
        let rate =
            get_exchange_rate_from_trade(RESERVE, RESERVE, RATE_SCALAR, RATE_ANCHOR, 1_000_000).unwrap();
        assert!(rate > 0);
    }

    #[test]
    fn trade_draining_pt_below_min_proportion_fails() {
        // Post-trade PT = 500_000 vs V = 100_000_000 → proportion ≈ 0.5%, below the 1% floor.
        // Without the bound this proportion truncates toward zero and panics inside ln_fp instead.
        assert_eq!(
            get_exchange_rate_from_trade(RESERVE, RESERVE, RATE_SCALAR, RATE_ANCHOR, RESERVE - 500_000),
            Err(AmmError::ProportionOutOfBounds)
        );
    }

    #[test]
    fn trade_pushing_pt_above_max_proportion_fails() {
        // User sells PT in: post-trade PT = 2.5e9 vs V = 1e8 → proportion ≈ 96.2%, above the 96% cap.
        assert_eq!(
            get_exchange_rate_from_trade(RESERVE, RESERVE, RATE_SCALAR, RATE_ANCHOR, -2_400_000_000),
            Err(AmmError::ProportionOutOfBounds)
        );
    }

    #[test]
    fn trade_near_min_proportion_boundary_succeeds() {
        // Post-trade PT = 2_200_000 over the fixed 200_000_000 denominator → proportion
        // 1.1%, just above the floor. Uses a flat curve (scalar 50): the ln adjustment
        // at this proportion is ≈ -0.09, keeping the exchange rate above 1.0.
        let rate = get_exchange_rate_from_trade(
            RESERVE,
            RESERVE,
            RATE_SCALAR * 10,
            RATE_ANCHOR,
            RESERVE - 2_200_000,
        )
        .unwrap();
        assert!(rate >= math::FP_SCALE);
    }

    #[test]
    fn trade_pushing_exchange_rate_below_one_fails() {
        // Post-trade PT = 20_000_000 → proportion 10%, well within bounds, but on the
        // steep curve (scalar 5) ln(p/(1-p))/scalar ≈ -0.44 pulls the rate to ~0.66.
        // Unguarded, that would store a negative implied rate and brick the pool;
        // the below-one guard must reject the trade instead.
        assert_eq!(
            get_exchange_rate_from_trade(RESERVE, RESERVE, RATE_SCALAR, RATE_ANCHOR, RESERVE - 20_000_000),
            Err(AmmError::ExchangeRateBelowOne)
        );
    }
}
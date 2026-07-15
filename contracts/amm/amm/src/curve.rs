use crate::math;

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
) -> i128 {
    assert!(reserve_pt > 0, "reserve_pt must be positive");
    assert!(reserve_v > 0, "reserve_v must be positive");
    assert!(last_implied_rate >= 0, "last_implied_rate must be non-negative");
    assert!(rate_scalar > 0, "rate_scalar must be positive");
    assert!(time_to_expiry_secs > 0, "time_to_expiry_secs must be positive");

    // new_exchange_rate = exp(implied_rate * t)
    let new_exchange_rate =
        math::implied_rate_to_exchange_rate(last_implied_rate, time_to_expiry_secs);

    assert!(new_exchange_rate > 0, "exchange rate must be positive");

    // proportion = PT / (PT + V)
    let total = reserve_pt
        .checked_add(reserve_v)
        .expect("overflow computing total reserves");

    let proportion = reserve_pt
        .checked_mul(math::FP_SCALE)
        .expect("overflow scaling reserve_pt")
        / total;

    assert!(
        proportion > 0 && proportion < math::FP_SCALE,
        "proportion must be between 0 and 1"
    );

    // ln( proportion / (1 - proportion) )
    let one_minus_p = math::FP_SCALE - proportion;

    let ratio = proportion
        .checked_mul(math::FP_SCALE)
        .expect("overflow computing proportion ratio")
        / one_minus_p;

    let ln_proportion = math::ln_fp(ratio, math::FP_SCALE);

    // rate_anchor = new_exchange_rate - lnProportion / rateScalar
    let adjustment = ln_proportion
        .checked_mul(math::FP_SCALE)
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
        .checked_mul(math::FP_SCALE)
        .expect("overflow scaling proportion")
        / denom;

    assert!(
        proportion >= MIN_PROPORTION && proportion <= MAX_PROPORTION,
        "trade pushes pool proportion out of bounds"
    );

    let one_minus_p = math::FP_SCALE - proportion;

    let ratio = proportion
        .checked_mul(math::FP_SCALE)
        .expect("overflow computing ratio")
        / one_minus_p;

    let ln_proportion = math::ln_fp(ratio, math::FP_SCALE);

    let adjustment = ln_proportion
        .checked_mul(math::FP_SCALE)
        .expect("overflow computing adjustment")
        / rate_scalar;

    let exchange_rate = rate_anchor
        .checked_add(adjustment)
        .expect("overflow computing exchange rate");

    // A rate below 1.0 would price PT above face value (negative yield) and,
    // worse, store a negative implied rate that bricks the pool: every later
    // compute_rate_anchor call asserts last_implied_rate >= 0. Mirrors
    // Pendle's MarketExchangeRateBelowOne revert.
    assert!(exchange_rate >= math::FP_SCALE, "exchange rate must not fall below one");
    exchange_rate
}

/// Core trade pricing for the yield AMM curve.
///
/// # Arguments
/// - `reserve_pt`          — current PT reserve
/// - `reserve_v`           — current V reserve (in underlying asset units)
/// - `rate_scalar`         — time-adjusted curve steepness (1e7-scaled)
/// - `rate_anchor`         — curve anchor, derived from current implied rate (1e7-scaled)
/// - `fee_factor`          — time-aware fee multiplier: `e^(fee_rate_root * t)`, always >= 1.0 (1e7-scaled)
/// - `reserve_fee_percent` — fraction of fee that goes to the reserve, as a whole-number percent (0–100)
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
    reserve_fee_percent: i128,
    net_pt_to_account: i128,
) -> (i128, i128, i128) {
    assert!(reserve_pt > 0, "reserve_pt must be positive");
    assert!(reserve_v > 0, "reserve_v must be positive");
    assert!(rate_scalar > 0, "rate_scalar must be positive");
    assert!(rate_anchor > 0, "rate_anchor must be positive");
    assert!(fee_factor >= math::FP_SCALE, "fee_factor must be >= 1.0");
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

    assert!(pre_fee_exchange_rate > 0, "pre_fee_exchange_rate must be positive");

    // 2) Signed pre-fee V flow to the account.
    //
    // preFeeAssetToAccount = netPtToAccount.divDown(preFeeExchangeRate).neg()
    //
    // Sign meaning:
    //   > 0 : V to user
    //   < 0 : V from user into pool
    let pre_fee_v_to_account = math::div_down(net_pt_to_account, pre_fee_exchange_rate)
        .checked_neg()
        .expect("overflow negating pre-fee V flow");

    // 3) Direction-sensitive fee logic using fee_factor (>= 1.0).
    //    fee_factor shrinks toward 1.0 as expiry approaches, so fees decay to zero.
    //
    // PT out: user pays MORE V in  — multiply raw V cost by fee_factor.
    // PT in:  user receives LESS V — divide raw V payout by fee_factor.
    let (net_v_to_account, net_v_fee) = if net_pt_to_account > 0 {
        // pre_fee_v_to_account < 0 (user pays V in).
        let raw_v_in = pre_fee_v_to_account
            .checked_neg()
            .expect("overflow converting raw_v_in");
        let actual_v_in = math::mul_down(raw_v_in, fee_factor);
        let fee = actual_v_in
            .checked_sub(raw_v_in)
            .expect("underflow computing PT-out fee");
        (actual_v_in.checked_neg().expect("overflow negating actual_v_in"), fee)
    } else {
        // pre_fee_v_to_account > 0 (user receives V out).
        let raw_v_out = pre_fee_v_to_account;
        let actual_v_out = math::div_down(raw_v_out, fee_factor);
        let fee = raw_v_out
            .checked_sub(actual_v_out)
            .expect("underflow computing PT-in fee");
        (actual_v_out, fee)
    };

    assert!(net_v_fee >= 0, "net_v_fee must be non-negative");

    // 4) Reserve fee split (base-100 percent).
    let net_v_to_reserve = (net_v_fee * reserve_fee_percent) / 100;

    (net_v_to_account, net_v_fee, net_v_to_reserve)
}

#[cfg(test)]
mod proportion_bounds_tests {
    use super::*;

    const RESERVE: i128 = 100_000_000; // 10 units at 1e7 scale
    const RATE_SCALAR: i128 = 5 * math::FP_SCALE;
    const RATE_ANCHOR: i128 = 11_000_000; // 1.1

    #[test]
    fn balanced_trade_within_bounds_succeeds() {
        let rate = get_exchange_rate_from_trade(RESERVE, RESERVE, RATE_SCALAR, RATE_ANCHOR, 1_000_000);
        assert!(rate > 0);
    }

    #[test]
    #[should_panic(expected = "trade pushes pool proportion out of bounds")]
    fn trade_draining_pt_below_min_proportion_panics() {
        // Post-trade PT = 500_000 vs V = 100_000_000 → proportion ≈ 0.5%, below the 1% floor.
        // Without the bound this proportion truncates toward zero and panics inside ln_fp instead.
        get_exchange_rate_from_trade(RESERVE, RESERVE, RATE_SCALAR, RATE_ANCHOR, RESERVE - 500_000);
    }

    #[test]
    #[should_panic(expected = "trade pushes pool proportion out of bounds")]
    fn trade_pushing_pt_above_max_proportion_panics() {
        // User sells PT in: post-trade PT = 2.5e9 vs V = 1e8 → proportion ≈ 96.2%, above the 96% cap.
        get_exchange_rate_from_trade(RESERVE, RESERVE, RATE_SCALAR, RATE_ANCHOR, -2_400_000_000);
    }

    #[test]
    fn trade_near_min_proportion_boundary_succeeds() {
        // Post-trade PT = 1_100_000 vs V = 100_000_000 → proportion ≈ 1.09%, just above the floor.
        // Uses a flat curve (scalar 50 ≈ default market ~6 months out): with a steep one
        // the ln adjustment at this proportion drags the exchange rate below 1.0, which
        // now correctly reverts before the proportion floor is ever reached.
        let rate = get_exchange_rate_from_trade(
            RESERVE,
            RESERVE,
            RATE_SCALAR * 10,
            RATE_ANCHOR,
            RESERVE - 1_100_000,
        );
        assert!(rate >= math::FP_SCALE);
    }

    #[test]
    #[should_panic(expected = "exchange rate must not fall below one")]
    fn trade_pushing_exchange_rate_below_one_panics() {
        // Same near-floor drain on the steep curve: ln(p/(1-p))/scalar ≈ -0.9 pulls the
        // rate to ~0.2. Unguarded, that would store a negative implied rate and brick
        // the pool; the below-one guard must reject the trade instead.
        get_exchange_rate_from_trade(RESERVE, RESERVE, RATE_SCALAR, RATE_ANCHOR, RESERVE - 1_100_000);
    }
}

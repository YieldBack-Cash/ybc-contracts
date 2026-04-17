use crate::math;

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
        proportion > 0 && proportion < math::FP_SCALE,
        "proportion must be between 0 and 1"
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

    assert!(exchange_rate > 0, "exchange rate must be positive");
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
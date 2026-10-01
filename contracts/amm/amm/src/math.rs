use amm_interface::AmmError;

/// The protocol's fixed-point scale (1e7), under the name the curve uses.
pub const FP_SCALE: i128 = ybc_common::scale::SCALAR_7;

// The fallible functions return a typed error for an input outside their
// domain, so a client sees `Error(Contract, #n)` rather than an opaque trap
// (the release profile strips panic messages). The callers guard these
// domains upstream, so none of the errors is reachable through a well-formed
// trade. `mul_down` and `div_down` are unchecked and only ever fed bounded,
// non-zero inputs.

/// Seconds in the year the curve's rates are annualised over.
pub const SECONDS_PER_YEAR: i128 = 365 * 24 * 3600;

/// Fixed-point natural log.
///
/// # Arguments
/// - `x`: input value scaled by `scale` (i.e., real value × scale)
/// - `scale`: precision base; always `FP_SCALE` in this crate
///
/// # Returns
/// `ln(x / scale)` scaled by `scale`, or `InvalidPoolState` for `x <= 0`
/// (only a degenerate pool proportion could produce one; curve.rs bounds the
/// proportion so the ratio fed here never truncates to zero).
pub fn ln_fp(x: i128, scale: i128) -> Result<i128, AmmError> {
    if x <= 0 {
        return Err(AmmError::InvalidPoolState);
    }

    // ln(2) × scale, to six places (0.693147). The last digit is below the
    // curve's tolerance; kept as is so pricing bytes do not change.
    let ln2 = 693_147i128 * scale / 1_000_000i128;

    // Normalize: find k such that x / 2^k ∈ [scale, 2*scale)
    // This lets us compute ln(x) = k*ln(2) + ln(x / 2^k)
    let mut normalized = x;
    let mut k: i128 = 0;

    while normalized >= 2 * scale {
        normalized /= 2;
        k += 1;
    }
    while normalized < scale {
        normalized *= 2;
        k -= 1;
    }

    // Artanh series: ln(y) = 2·artanh(z) = 2·(z + z³/3 + z⁵/5 + ...) where
    // z = (y - 1)/(y + 1). With y ∈ [1, 2) from normalization, z ≤ 1/3, so each
    // term shrinks by z² ≤ 1/9 — five terms leave an error under 1e-5, versus
    // ~6e-2 for a plain 8-term Taylor series near y → 2.
    let z = (normalized - scale) * scale / (normalized + scale); // scaled
    let z2 = z * z / scale;

    let mut result: i128 = 0;
    let mut term = z; // z^n for odd n, scaled

    for n in [1i128, 3, 5, 7, 9] {
        result += term / n;
        term = term * z2 / scale;
    }
    result *= 2;

    // Add back the normalization shift: k * ln(2)
    Ok(result + k * ln2)
}

/// Converts a duration in seconds to years as a fixed-point value scaled by `FP_SCALE`.
pub fn seconds_to_years(seconds: u64) -> i128 {
    (seconds as i128 * FP_SCALE) / SECONDS_PER_YEAR
}

/// `a * b / c`, floored, for the share arithmetic of liquidity events.
/// `MathOverflow` when the product does not fit, `InvalidPoolState` for a
/// zero divisor; neither is reachable with amounts a token can hold.
pub fn mul_div(a: i128, b: i128, c: i128) -> Result<i128, AmmError> {
    if c == 0 {
        return Err(AmmError::InvalidPoolState);
    }
    Ok(a.checked_mul(b).ok_or(AmmError::MathOverflow)? / c)
}

/// Fixed-point multiply, rounded down: (a * b) / FP_SCALE
pub fn mul_down(a: i128, b: i128) -> i128 {
    (a * b) / FP_SCALE
}

/// Fixed-point divide, rounded down: (a * FP_SCALE) / b
pub fn div_down(a: i128, b: i128) -> i128 {
    (a * FP_SCALE) / b
}

/// Fixed-point e^x via a 20-term Taylor series, for non-negative 1e7-scaled x.
/// Relative error below 1e-6 for x ≤ 5 (a 100% APY market about seven years
/// out) and about 0.1% at x = 10. A negative exponent is a negative rate-time
/// product, which no valid pool state produces: `InvalidPoolState`.
pub fn exp_fp(x: i128) -> Result<i128, AmmError> {
    if x < 0 {
        return Err(AmmError::InvalidPoolState);
    }
    let mut result = FP_SCALE; // 1.0
    let mut term = FP_SCALE; // current term
    for n in 1i128..=20 {
        term = term * x / (n * FP_SCALE);
        if term == 0 {
            break;
        }
        result += term;
    }
    Ok(result)
}

/// Converts a 1e7-scaled exchange rate and 1e7-scaled time in years
/// back to a 1e7-scaled ln implied rate: ln_implied_rate = ln(exchange_rate) / t
///
/// A non-positive time means the market has expired (`MarketExpired`); a
/// non-positive rate means the curve produced something it never should
/// (`InvalidPoolState`).
pub fn exchange_rate_to_implied_rate(exchange_rate: i128, t_years: i128) -> Result<i128, AmmError> {
    if t_years <= 0 {
        return Err(AmmError::MarketExpired);
    }
    Ok(div_down(ln_fp(exchange_rate, FP_SCALE)?, t_years))
}

/// Converts a 1e7-scaled ln implied rate and a time to expiry in seconds
/// to a 1e7-scaled exchange rate: exchange_rate = e^(ln_implied_rate * t / 1_year)
///
/// `MarketExpired` for `time_to_expiry_secs <= 0`, `InvalidPoolState` for a
/// negative rate, and `MathOverflow` should the series ever return a rate
/// below 1.0, which e^x for x >= 0 cannot.
pub fn implied_rate_to_exchange_rate(
    ln_implied_rate: i128,
    time_to_expiry_secs: i128,
) -> Result<i128, AmmError> {
    if ln_implied_rate < 0 {
        return Err(AmmError::InvalidPoolState);
    }
    if time_to_expiry_secs <= 0 {
        return Err(AmmError::MarketExpired);
    }

    let rt = div_down(
        mul_down(ln_implied_rate, time_to_expiry_secs),
        SECONDS_PER_YEAR,
    );
    let exchange_rate = exp_fp(rt)?;

    if exchange_rate < FP_SCALE {
        return Err(AmmError::MathOverflow);
    }
    Ok(exchange_rate)
}

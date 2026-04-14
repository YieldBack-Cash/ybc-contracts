pub const FP_SCALE: i128 = 10_000_000; // 1e7

/// Fixed-point natural log.
///
/// # Arguments
/// - `x`: input value scaled by `SCALE` (i.e., real value × SCALE)
/// - `SCALE`: precision base, e.g. 1_000_000 for 6 decimal places
///
/// # Returns
/// `ln(x / SCALE)` scaled by `SCALE`
///
/// # Panics
/// Panics if `x <= 0`.
pub fn ln_fp(x: i128, scale: i128) -> i128 { // todo: as the pool becomes more V heavy, this function will break
    assert!(x > 0, "ln undefined for non-positive values");

    // ln(2) × SCALE, precomputed: 0.693147... × scale
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

    // Taylor series for ln(1 + t) where t = (normalized - scale) / scale
    // ln(1+t) = t - t²/2 + t³/3 - t⁴/4 + ...
    // t ∈ [0, 1) here, so series converges quickly (6–8 terms is plenty)
    let t = normalized - scale; // t scaled by `scale`
    let mut result: i128 = 0;
    let mut term = t;           // t^n / scale^(n-1), scaled

    for n in 1i128..=8 {
        if n % 2 == 1 {
            result += term / n;
        } else {
            result -= term / n;
        }
        term = term / scale * t; // advance: term *= t/scale
    }

    // Add back the normalization shift: k * ln(2)
    result + k * ln2
}
const IMPLIED_RATE_TIME: i128 = 365 * 86_400;

/// Converts a duration in seconds to years as a fixed-point value scaled by `FP_SCALE`.
pub fn seconds_to_years(seconds: u64) -> i128 {
    const SECONDS_PER_YEAR: u64 = 365 * 24 * 3600;
    (seconds as i128 * FP_SCALE) / SECONDS_PER_YEAR as i128
}

/// Fixed-point multiply, rounded down: (a * b) / FP_SCALE
pub fn mul_down(a: i128, b: i128) -> i128 {
    (a * b) / FP_SCALE
}

/// Fixed-point divide, rounded down: (a * FP_SCALE) / b
pub fn div_down(a: i128, b: i128) -> i128 {
    (a * FP_SCALE) / b
}

/// Fixed-point divide, rounded up: ceil(a * FP_SCALE / b)
pub fn div_up(a: i128, b: i128) -> i128 {
    (a * FP_SCALE + b - 1) / b
}

/// Fixed-point e^x via Taylor series, for non-negative 1e7-scaled x.
/// Accurate for x up to ~2.0 (20_000_000); sufficient for typical implied rates.
pub fn exp_fp(x: i128) -> i128 {
    assert!(x >= 0);
    let mut result = FP_SCALE; // 1.0
    let mut term = FP_SCALE;   // current term
    for n in 1i128..=20 {
        term = term * x / (n * FP_SCALE);
        if term == 0 { break; }
        result += term;
    }
    result
}

/// Converts a 1e7-scaled exchange rate and 1e7-scaled time in years
/// back to a 1e7-scaled ln implied rate: ln_implied_rate = ln(exchange_rate) / t
pub fn exchange_rate_to_implied_rate(exchange_rate: i128, t_years: i128) -> i128 {
    assert!(exchange_rate > 0);
    assert!(t_years > 0);
    div_down(ln_fp(exchange_rate, FP_SCALE), t_years)
}

/// Converts a 1e7-scaled ln implied rate and a time to expiry in seconds
/// to a 1e7-scaled exchange rate: exchange_rate = e^(ln_implied_rate * t / 1_year)
pub fn implied_rate_to_exchange_rate(ln_implied_rate: i128, time_to_expiry_secs: i128) -> i128 {
    assert!(ln_implied_rate >= 0);
    assert!(time_to_expiry_secs > 0);

    let rt = div_down(mul_down(ln_implied_rate, time_to_expiry_secs), IMPLIED_RATE_TIME);
    let exchange_rate = exp_fp(rt);

    assert!(exchange_rate >= FP_SCALE); // exchange_rate >= 1.0

    exchange_rate
}
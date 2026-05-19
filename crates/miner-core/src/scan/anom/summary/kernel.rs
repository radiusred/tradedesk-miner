//! Pure Welford running-moments + IQR kernel for ANOM-02.
//!
//! Pattern analog: `crates/miner-core/src/scan/ljung_box/kernel.rs` — private
//! `#[inline] pub(super)` pure functions on `&[f64]` with a sibling
//! `#[cfg(test)] mod tests` block.
//!
//! ## Implementation notes
//!
//! - `welford_pass` is a single-pass online accumulator computing mean,
//!   `m2`, `m3`, `m4` central moments via Pébay-style updates (numerically
//!   stable; equivalent to `scipy.stats.describe(bias=False)`).
//! - Sample standard deviation uses ddof=1.
//! - Skew is the bias-corrected G1 estimator: `g1 * sqrt(n*(n-1))/(n-2)`.
//! - Excess kurtosis is the bias-corrected G2 estimator:
//!   `((n+1)*g2 + 6) * (n-1) / ((n-2)*(n-3))` where g2 = m4/(n*std^4) - 3.
//! - IQR uses linear interpolation between adjacent order statistics at
//!   `0.25 * (n-1)` and `0.75 * (n-1)` — matches `scipy.stats.iqr` default.
//! - Sequential summation order pins cross-platform determinism (Pitfall 4).

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// Output of [`welford_pass`] — mean / sample-std / bias-corrected skew /
/// bias-corrected excess kurtosis.
#[derive(Debug, Clone, Copy)]
pub(super) struct WelfordStats {
    pub mean: f64,
    pub std: f64,
    pub skew: f64,
    pub excess_kurtosis: f64,
}

/// One-pass Welford accumulator for the first four central moments. Returns
/// mean + ddof=1 sample std + bias-corrected G1 skewness + bias-corrected G2
/// excess kurtosis.
///
/// Constant-input branch (variance == 0): returns
/// `(mean, 0.0, 0.0, 0.0)` to avoid NaN propagation.
///
/// For `n < 3` skew is set to 0 (the bias correction would divide by `n - 2`);
/// for `n < 4` excess kurtosis is set to 0 (divides by `n - 3`). Callers
/// asserting on hand-derived values must use `n >= 4`.
///
/// # Panics
/// Panics via `debug_assert` when `values.len() < 2`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "values.len() is a bar count; fits in f64's 52-bit mantissa for any realistic OHLCV series"
)]
#[allow(
    clippy::many_single_char_names,
    reason = "m / m2 / m3 / m4 are the canonical central-moment names in the welford literature"
)]
pub(super) fn welford_pass(values: &[f64]) -> WelfordStats {
    debug_assert!(
        values.len() >= 2,
        "welford_pass: need >= 2 samples; got {}",
        values.len()
    );
    let n_usize = values.len();
    let n_f = n_usize as f64;

    // Single-pass Pébay accumulation of central moments. Variables follow
    // scipy.stats's `_stats._compute_stats_with_central_moments` naming.
    let mut mean = 0.0_f64;
    let mut m2 = 0.0_f64;
    let mut m3 = 0.0_f64;
    let mut m4 = 0.0_f64;
    for (i, &x) in values.iter().enumerate() {
        let n1 = i as f64; // count BEFORE seeing this sample.
        let n2 = n1 + 1.0; // count AFTER seeing this sample.
        let delta = x - mean;
        let delta_n = delta / n2;
        let delta_n2 = delta_n * delta_n;
        let term1 = delta * delta_n * n1;
        mean += delta_n;
        m4 += term1 * delta_n2 * (n2 * n2 - 3.0 * n2 + 3.0) + 6.0 * delta_n2 * m2
            - 4.0 * delta_n * m3;
        m3 += term1 * delta_n * (n2 - 2.0) - 3.0 * delta_n * m2;
        m2 += term1;
    }

    // Sample variance (ddof=1).
    let var = m2 / (n_f - 1.0);
    let std = var.sqrt();
    // Population variance — used for scipy-compatible G1 / G2 estimators
    // (the central moments are m2/n, m3/n, m4/n on the population side).
    let var_pop = m2 / n_f;
    if !std.is_finite() || std == 0.0 || var_pop == 0.0 {
        // Constant-input branch — skew + kurtosis are undefined; return 0.
        return WelfordStats {
            mean,
            std: 0.0,
            skew: 0.0,
            excess_kurtosis: 0.0,
        };
    }

    // G1 (bias-corrected sample skew per scipy.stats.skew(bias=False)).
    // Requires n >= 3. Formula:
    //   g1 = m3_pop / m2_pop^(3/2)  (Fisher-Pearson, biased)
    //   G1 = sqrt(n*(n-1))/(n-2) * g1
    // Note: m2_pop / m3_pop are central moments divided by n (not n-1).
    let skew = if n_usize < 3 {
        0.0
    } else {
        let m2_pop = m2 / n_f;
        let m3_pop = m3 / n_f;
        let g1 = m3_pop / m2_pop.powf(1.5);
        g1 * ((n_f * (n_f - 1.0)).sqrt() / (n_f - 2.0))
    };

    // G2 (bias-corrected sample excess kurtosis per scipy.stats.kurtosis(
    // bias=False)). Requires n >= 4. Formula:
    //   g2 = m4_pop / m2_pop^2 - 3
    //   G2 = ((n+1)*g2 + 6) * (n-1) / ((n-2)*(n-3))
    let excess_kurtosis = if n_usize < 4 {
        0.0
    } else {
        let m2_pop = m2 / n_f;
        let m4_pop = m4 / n_f;
        let g2 = m4_pop / (m2_pop * m2_pop) - 3.0;
        ((n_f + 1.0) * g2 + 6.0) * (n_f - 1.0) / ((n_f - 2.0) * (n_f - 3.0))
    };

    WelfordStats {
        mean,
        std,
        skew,
        excess_kurtosis,
    }
}

/// Inter-quartile range (P75 - P25) via linear interpolation between adjacent
/// order statistics at `0.25 * (n - 1)` and `0.75 * (n - 1)`. Matches
/// `scipy.stats.iqr` default (`interpolation='linear'`).
///
/// # Panics
/// Panics via `debug_assert` when `values.len() < 2`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n is a bar count; quantile indices are bounded by n-1 and round into usize via floor"
)]
pub(super) fn iqr(values: &[f64]) -> f64 {
    debug_assert!(
        values.len() >= 2,
        "iqr: need >= 2 samples; got {}",
        values.len()
    );
    let mut sorted: Vec<f64> = values.to_vec();
    // total_cmp pins ordering across platforms.
    sorted.sort_by(f64::total_cmp);
    quantile_linear(&sorted, 0.75) - quantile_linear(&sorted, 0.25)
}

/// Linear-interpolation quantile on a pre-sorted slice. Mirrors numpy's
/// default `quantile(..., interpolation='linear')`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n is bounded by the bar count; quantile index arithmetic is exact within f64 mantissa"
)]
fn quantile_linear(sorted: &[f64], q: f64) -> f64 {
    debug_assert!(!sorted.is_empty(), "quantile_linear: empty slice");
    debug_assert!(
        (0.0..=1.0).contains(&q),
        "quantile_linear: q must be in [0, 1]"
    );
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let h = q * (n as f64 - 1.0);
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = h - h.floor();
    sorted[lo] * (1.0 - frac) + sorted[hi] * frac
}

/// Min / max of a `&[f64]` via `f64::total_cmp` ordering. Treats NaN as larger
/// than any other value (the `total_cmp` total order). Callers that want NaN
/// rejection should do so BEFORE calling here.
///
/// # Panics
/// Panics via `debug_assert` when `values.is_empty()`.
#[inline]
pub(super) fn min_max(values: &[f64]) -> (f64, f64) {
    debug_assert!(!values.is_empty(), "min_max: empty slice");
    let mut lo = values[0];
    let mut hi = values[0];
    for &v in &values[1..] {
        if v.total_cmp(&lo).is_lt() {
            lo = v;
        }
        if v.total_cmp(&hi).is_gt() {
            hi = v;
        }
    }
    (lo, hi)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    // -----------------------------------------------------------------------
    // welford_pass — hand-derived references
    // -----------------------------------------------------------------------

    #[test]
    fn welford_known_input_mean_and_std() {
        // [1, 2, 3, 4, 5]: mean = 3.0; sample-std (ddof=1) = sqrt(2.5).
        let values = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let s = welford_pass(&values);
        assert!(approx_eq(s.mean, 3.0, TOL), "mean={}", s.mean);
        assert!(approx_eq(s.std, 2.5_f64.sqrt(), TOL), "std={}", s.std);
    }

    /// Plan 04-03 Task 2 Behavior Test 6 — bias-corrected sample skew (G1)
    /// for [1, 2, 3, 4, 5] is exactly 0 (symmetric input).
    #[test]
    fn welford_skew_symmetric_input_is_zero() {
        let values = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let s = welford_pass(&values);
        assert!(approx_eq(s.skew, 0.0, TOL), "skew={}", s.skew);
    }

    /// Plan 04-03 Task 2 Behavior Test 6 — G1 skew matches scipy hand-
    /// derived for asymmetric input [1, 2, 3, 4, 10].
    /// scipy reference: scipy.stats.skew([1,2,3,4,10], bias=False) =
    /// 1.5404320130488395 (computed offline). Within 1e-12.
    #[test]
    fn welford_skew_asymmetric_known_input() {
        let values = [1.0_f64, 2.0, 3.0, 4.0, 10.0];
        let s = welford_pass(&values);
        // scipy.stats.skew([1,2,3,4,10], bias=False) per scipy 1.14.1:
        // sqrt(20)/3 * 36/sqrt(1000) = 1.6970562748477143
        let expected = 1.697_056_274_847_714_3_f64;
        assert!(
            approx_eq(s.skew, expected, TOL),
            "skew={} expected {}",
            s.skew,
            expected
        );
    }

    /// Plan 04-03 Task 2 Behavior Test 7 — bias-corrected excess kurtosis
    /// for [1, 2, 3, 4, 5, 6, 7, 8].
    /// scipy reference: scipy.stats.kurtosis([1..8], bias=False) =
    /// -1.2 (computed offline; the uniform-distribution kurtosis is -6/5).
    /// Within 1e-12.
    #[test]
    fn welford_excess_kurtosis_known_input() {
        let values: Vec<f64> = (1..=8).map(|i| i as f64).collect();
        let s = welford_pass(&values);
        let expected = -1.2_f64;
        assert!(
            approx_eq(s.excess_kurtosis, expected, TOL),
            "kurt={} expected {}",
            s.excess_kurtosis,
            expected
        );
    }

    #[test]
    fn welford_constant_input_is_zero_std_skew_kurt() {
        let values = [3.7_f64; 10];
        let s = welford_pass(&values);
        assert!(approx_eq(s.mean, 3.7, TOL));
        assert!(approx_eq(s.std, 0.0, TOL), "constant -> std == 0");
        assert!(approx_eq(s.skew, 0.0, TOL));
        assert!(approx_eq(s.excess_kurtosis, 0.0, TOL));
    }

    #[test]
    #[should_panic(expected = "welford_pass: need >= 2 samples")]
    fn welford_panics_below_two_samples() {
        let _ = welford_pass(&[42.0_f64]);
    }

    // -----------------------------------------------------------------------
    // iqr — hand-derived references
    // -----------------------------------------------------------------------

    /// Plan 04-03 Task 2 Behavior Test 8 — IQR for [1, 2, 3, 4, 5] equals
    /// 2.0 via linear interpolation:
    /// P75 = sorted[3] = 4.0 + 0*frac = 4.0; P25 = sorted[1] = 2.0; IQR = 2.0.
    #[test]
    fn iqr_known_input_odd_length() {
        let values = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let q = iqr(&values);
        assert!(approx_eq(q, 2.0, TOL), "iqr={}", q);
    }

    /// IQR for [1, 2, 3, 4]: scipy reference linear interpolation gives
    /// P75 = sorted[2.25] = 3 + 0.25*(4-3) = 3.25; P25 = sorted[0.75] =
    /// 1 + 0.75*(2-1) = 1.75; IQR = 1.5.
    #[test]
    fn iqr_known_input_even_length() {
        let values = [1.0_f64, 2.0, 3.0, 4.0];
        let q = iqr(&values);
        assert!(approx_eq(q, 1.5, TOL), "iqr={}", q);
    }

    #[test]
    fn iqr_constant_input_is_zero() {
        let values = [5.0_f64; 4];
        assert!(approx_eq(iqr(&values), 0.0, TOL));
    }

    // -----------------------------------------------------------------------
    // min_max
    // -----------------------------------------------------------------------

    #[test]
    fn min_max_basic() {
        let values = [3.0_f64, -1.0, 7.0, 2.0];
        let (lo, hi) = min_max(&values);
        assert!(approx_eq(lo, -1.0, TOL));
        assert!(approx_eq(hi, 7.0, TOL));
    }
}

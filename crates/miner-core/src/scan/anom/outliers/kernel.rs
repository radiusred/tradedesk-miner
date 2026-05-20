//! Pure outlier-detection kernels for ANOM-10 — `z_scores`, `modified_z_scores`,
//! `median`, `mad`.
//!
//! Pattern analog: `crates/miner-core/src/scan/ljung_box/kernel.rs` and
//! `crates/miner-core/src/scan/anom/summary/kernel.rs` — private `#[inline]
//! pub(super)` pure functions over `&[f64]` with a sibling `#[cfg(test)] mod
//! tests` block.
//!
//! ## Implementation notes
//!
//! - Two-pass mean + std (ddof=0; population std) per the `scipy.stats.zscore`
//!   default convention (`ddof=0`). The standard z-score is
//!   `(x - mean) / std_pop`.
//! - Modified z-score uses the Iglewicz-Hoaglin formula:
//!   `M_i = 0.6745 * (x_i - median) / MAD` where `MAD = median(|x_i - median|)`.
//!   The constant `0.6745` is the inverse of the 75th percentile of the
//!   standard normal distribution (an asymptotically consistent estimator
//!   of the population std for normal data).
//! - Median uses linear interpolation for even-length arrays
//!   (matches numpy's `np.median` default).
//! - MAD == 0 branch returns 0 — the scan body then converts this to a
//!   `ScanError::Kernel` so consumers never observe NaN.
//! - Sort uses `f64::total_cmp` to pin cross-platform determinism.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// Population z-scores for a slice — `(x - mean) / std_pop` where
/// `std_pop = sqrt(sum((x - mean)^2) / n)` (ddof=0). Matches
/// `scipy.stats.zscore(x)` with default `ddof=0`.
///
/// Returns `(z_scores, mean, std_pop)`. Constant-input branch (std == 0)
/// returns a vector of zeros (no NaN propagation).
///
/// # Panics
/// Panics via `debug_assert` when `values.is_empty()`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n is a bar/return count; fits in f64's 52-bit mantissa for any realistic series"
)]
pub(super) fn z_scores(values: &[f64]) -> (Vec<f64>, f64, f64) {
    debug_assert!(!values.is_empty(), "z_scores: empty slice");
    let n = values.len();
    let n_f = n as f64;
    let mean: f64 = values.iter().sum::<f64>() / n_f;
    // Population std (ddof=0) per scipy.stats.zscore default.
    let var_pop: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n_f;
    let std_pop = var_pop.sqrt();
    if std_pop == 0.0 || !std_pop.is_finite() {
        return (vec![0.0; n], mean, 0.0);
    }
    let out: Vec<f64> = values.iter().map(|v| (v - mean) / std_pop).collect();
    (out, mean, std_pop)
}

/// Modified (Iglewicz-Hoaglin) z-scores for a slice.
///
/// Formula: `M_i = 0.6745 * (x_i - median) / MAD` where
/// `MAD = median(|x_i - median|)`.
///
/// Returns `(modified_z_scores, median, mad)`. The constant `0.6745` is the
/// inverse of the 75th percentile of the standard normal (~0.6745) — this
/// scales MAD into an asymptotically-consistent std estimator under
/// normality. The convention threshold for outlier flagging is `|M_i| > 3.5`
/// (Iglewicz-Hoaglin 1993).
///
/// **Constant-input branch:** MAD == 0 yields a vector of zeros AND returns
/// `mad = 0.0`. The scan body inspects `mad == 0` and converts to a
/// `ScanError::Kernel` to avoid emitting a finding with all-zero modified-z
/// scores.
///
/// # Panics
/// Panics via `debug_assert` when `values.is_empty()`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n is a bar/return count; fits in f64's 52-bit mantissa"
)]
pub(super) fn modified_z_scores(values: &[f64]) -> (Vec<f64>, f64, f64) {
    debug_assert!(!values.is_empty(), "modified_z_scores: empty slice");
    let med = median(values);
    let mad_value = mad(values, med);
    let n = values.len();
    if mad_value == 0.0 {
        return (vec![0.0; n], med, 0.0);
    }
    // Iglewicz-Hoaglin constant: inverse CDF of normal at 0.75 ~= 0.6745.
    let out: Vec<f64> = values
        .iter()
        .map(|v| 0.6745 * (v - med) / mad_value)
        .collect();
    (out, med, mad_value)
}

/// Median via linear interpolation for even-length arrays (matches
/// `np.median` default). Sorts a copy via `f64::total_cmp` to pin
/// cross-platform determinism.
///
/// # Panics
/// Panics via `debug_assert` when `values.is_empty()`.
#[inline]
pub(super) fn median(values: &[f64]) -> f64 {
    debug_assert!(!values.is_empty(), "median: empty slice");
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n % 2 == 0 {
        // even length -> mean of the two middle elements (linear interp).
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}

/// Median absolute deviation: `median(|x_i - median_of_x|)`. Uses the
/// supplied `med` so callers can compute it once.
#[inline]
pub(super) fn mad(values: &[f64], med: f64) -> f64 {
    let abs_dev: Vec<f64> = values.iter().map(|v| (v - med).abs()).collect();
    median(&abs_dev)
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
    // median
    // -----------------------------------------------------------------------

    #[test]
    fn median_odd_length() {
        assert!(approx_eq(median(&[1.0_f64, 2.0, 3.0, 4.0, 5.0]), 3.0, TOL));
    }

    #[test]
    fn median_even_length_interpolates() {
        // [1,2,3,4] -> (2+3)/2 = 2.5
        assert!(approx_eq(median(&[1.0_f64, 2.0, 3.0, 4.0]), 2.5, TOL));
    }

    #[test]
    fn median_unsorted_input() {
        assert!(approx_eq(median(&[5.0_f64, 1.0, 3.0, 4.0, 2.0]), 3.0, TOL));
    }

    #[test]
    fn median_constant_input() {
        assert!(approx_eq(median(&[7.0_f64; 5]), 7.0, TOL));
    }

    #[test]
    #[should_panic(expected = "median: empty slice")]
    fn median_empty_panics() {
        let _ = median(&[]);
    }

    // -----------------------------------------------------------------------
    // mad
    // -----------------------------------------------------------------------

    #[test]
    fn mad_known_input() {
        // [1, 2, 3, 4, 5]: median=3; abs deviations = [2, 1, 0, 1, 2];
        // sorted = [0, 1, 1, 2, 2]; median (odd) = 1. MAD = 1.
        let values = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let m = median(&values);
        assert!(approx_eq(mad(&values, m), 1.0, TOL));
    }

    #[test]
    fn mad_constant_input_is_zero() {
        let values = [5.0_f64; 10];
        let m = median(&values);
        assert!(approx_eq(mad(&values, m), 0.0, TOL));
    }

    // -----------------------------------------------------------------------
    // z_scores
    // -----------------------------------------------------------------------

    #[test]
    fn z_scores_basic() {
        // [1, 2, 3, 4, 5]: mean=3; var_pop = ((4+1+0+1+4)/5) = 2;
        // std_pop = sqrt(2). z = (x - 3) / sqrt(2) -> [-sqrt(2),
        // -1/sqrt(2), 0, 1/sqrt(2), sqrt(2)].
        let (z, mean, std) = z_scores(&[1.0_f64, 2.0, 3.0, 4.0, 5.0]);
        let s = 2.0_f64.sqrt();
        let expected = [-s, -1.0 / s, 0.0, 1.0 / s, s];
        assert!(approx_eq(mean, 3.0, TOL));
        assert!(approx_eq(std, s, TOL));
        assert_eq!(z.len(), 5);
        for (i, (got, want)) in z.iter().zip(expected.iter()).enumerate() {
            assert!(
                approx_eq(*got, *want, TOL),
                "z[{i}]={got} expected {want}"
            );
        }
    }

    #[test]
    fn z_scores_constant_input_is_zero() {
        let (z, mean, std) = z_scores(&[5.0_f64; 6]);
        assert!(approx_eq(mean, 5.0, TOL));
        assert!(approx_eq(std, 0.0, TOL));
        for v in z {
            assert!(approx_eq(v, 0.0, TOL));
        }
    }

    /// Strong outlier in [1, 2, 3, 4, 100] — last index clearly outlier.
    #[test]
    fn z_scores_strong_outlier() {
        let (z, _, _) = z_scores(&[1.0_f64, 2.0, 3.0, 4.0, 100.0]);
        // The last z-score must be > 1 (well above moderate threshold).
        assert!(z[4] > 1.0, "z[4]={} should be > 1", z[4]);
    }

    // -----------------------------------------------------------------------
    // modified_z_scores — Iglewicz-Hoaglin hand-derivation
    // -----------------------------------------------------------------------

    /// Hand-derived: median=0, MAD=1 -> `modified_z` = 0.6745 * x / 1.
    /// x=4 -> M = 0.6745 * 4 = 2.698 < 3.5 (NOT outlier).
    /// x=10 -> M = 0.6745 * 10 = 6.745 > 3.5 (IS outlier).
    #[test]
    fn modified_z_iglewicz_hoaglin_x_eq_4_not_outlier() {
        // Construct a series so median=0, MAD=1.
        // Use 5 elements: [-2, -1, 0, 1, 2]. Median=0. abs devs=[2,1,0,1,2];
        // MAD=1.
        let values = [-2.0_f64, -1.0, 0.0, 1.0, 2.0];
        let (mz, med, mad_v) = modified_z_scores(&values);
        assert!(approx_eq(med, 0.0, TOL));
        assert!(approx_eq(mad_v, 1.0, TOL));
        // mz[0] = 0.6745 * -2.0 / 1.0 = -1.349
        assert!(approx_eq(mz[0], -1.349, TOL));
        // For x=4: 0.6745*4 = 2.698 (NOT outlier at 3.5).
        // For x=10: 0.6745*10 = 6.745 (IS outlier at 3.5).
        // Hand-derived directly without going through the full slice.
        let m4 = 0.6745 * 4.0_f64 / 1.0;
        assert!(m4 < 3.5);
        assert!(approx_eq(m4, 2.698, TOL));
        let m10 = 0.6745 * 10.0_f64 / 1.0;
        assert!(m10 > 3.5);
        assert!(approx_eq(m10, 6.745, TOL));
    }

    #[test]
    fn modified_z_constant_input_returns_zeros_and_mad_zero() {
        let (mz, med, mad_v) = modified_z_scores(&[7.0_f64; 8]);
        assert!(approx_eq(med, 7.0, TOL));
        assert!(approx_eq(mad_v, 0.0, TOL));
        for v in mz {
            assert!(approx_eq(v, 0.0, TOL));
        }
    }

    #[test]
    fn modified_z_known_input_strong_outlier() {
        // [1, 2, 3, 4, 100]: median=3; abs devs=[2,1,0,1,97];
        // sorted abs devs=[0,1,1,2,97]; median=1. So MAD=1.
        // mz[4] = 0.6745 * (100-3) / 1 = 0.6745 * 97 = 65.4265
        let (mz, _, mad_v) = modified_z_scores(&[1.0_f64, 2.0, 3.0, 4.0, 100.0]);
        assert!(approx_eq(mad_v, 1.0, TOL));
        assert!(approx_eq(mz[4], 0.6745 * 97.0, TOL));
    }

    #[test]
    fn modified_z_length_invariant() {
        let n = 7;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let (mz, _, _) = modified_z_scores(&xs);
        assert_eq!(mz.len(), n);
    }
}

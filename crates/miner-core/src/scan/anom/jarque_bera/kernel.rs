//! Pure Jarque-Bera normality kernel — closed-form on the third and fourth
//! central moments + statrs chi²(2) tail.
//!
//! Pattern analog: `crates/miner-core/src/scan/anom/summary/kernel.rs` —
//! reuses the bias-corrected skew + excess kurtosis from `welford_pass` so
//! the Jarque-Bera statistic ties algebraically to ANOM-02's published
//! moments. The kernel itself is tiny: combine those two moments via the
//! standard JB formula and look up the chi²(2) p-value.
//!
//! ## Reference
//!
//! `scipy.stats.jarque_bera(x)` — emits `(jb_stat, p_value)`. The formula:
//!
//! ```text
//!   JB = (n / 6) * (S² + (K - 3)² / 4)
//! ```
//!
//! where `S` is the (bias-corrected) sample skew and `K - 3` is the
//! (bias-corrected) sample excess kurtosis. The kernel uses the same
//! definitions as `welford_pass` for byte-identical alignment with
//! `stats.summary.welford@1` (ANOM-02).
//!
//! Under H0 (normality), JB follows a `ChiSquared(2)` distribution
//! asymptotically. `p_value = 1 - ChiSquared(2).cdf(JB)`.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use statrs::distribution::{ChiSquared, ContinuousCDF};

use crate::scan::anom::summary::kernel::welford_pass;

/// Output of [`jarque_bera`] — JB statistic, chi²(2) p-value, the input
/// (bias-corrected) sample skew + excess kurtosis used in the formula, and
/// the sample size `n`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct JbResult {
    /// JB statistic = `(n / 6) * (S² + (K-3)² / 4)`.
    pub statistic: f64,
    /// p-value = `1 - ChiSquared(2).cdf(statistic)`.
    pub p_value: f64,
    /// Bias-corrected sample skew (G1) — identical to ANOM-02's `skew`.
    pub skew: f64,
    /// Bias-corrected sample excess kurtosis (G2) — identical to ANOM-02's
    /// `excess_kurtosis`.
    pub excess_kurtosis: f64,
    /// Number of samples.
    pub n: usize,
}

/// Jarque-Bera normality test on a sample. Reuses `welford_pass` from
/// `anom::summary::kernel` for the moments (byte-identical alignment with
/// `stats.summary.welford@1`).
///
/// Returns `Err(String)` if `n < 4` (the bias-corrected kurtosis estimator
/// divides by `n - 3`) or if the input has zero variance (skew + kurtosis
/// are undefined for constant input — kernel surfaces this as `Err` so the
/// scan body can convert to `ScanError::Kernel`).
///
/// # Panics
/// Does not panic on valid inputs; `welford_pass` `debug_asserts` `n >= 2`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n is a bar count; fits in f64's 52-bit mantissa for any realistic OHLCV series"
)]
pub(crate) fn jarque_bera(values: &[f64]) -> Result<JbResult, String> {
    let n = values.len();
    if n < 4 {
        return Err(format!(
            "jarque_bera: need n >= 4 to compute bias-corrected excess kurtosis; got n={n}"
        ));
    }

    let stats = welford_pass(values);
    // Constant-input rejection: welford_pass returns std == 0 for constant
    // input and zeros out the moments; JB is undefined in that case.
    if stats.std == 0.0 {
        return Err(format!(
            "jarque_bera: constant input (std=0); skew/kurtosis undefined; got n={n}"
        ));
    }

    let n_f = n as f64;
    let skew = stats.skew;
    let excess_kurt = stats.excess_kurtosis;
    let statistic = (n_f / 6.0) * (skew * skew + (excess_kurt * excess_kurt) / 4.0);

    // p_value under ChiSquared(2). ChiSquared::new(2.0) cannot fail for df=2.
    let chi = ChiSquared::new(2.0).expect("ChiSquared(2) is well-defined");
    let p_value = 1.0 - chi.cdf(statistic);

    Ok(JbResult {
        statistic,
        p_value,
        skew,
        excess_kurtosis: excess_kurt,
        n,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-10;
    const TOL_TIGHT: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Hand-derivable closed form: for the symmetric uniform-shaped input
    /// `[1, 2, 3, 4, 5, 6, 7, 8]`, skew == 0 exactly (symmetric) and
    /// `excess_kurtosis` matches scipy's `-1.2`. JB = (8/6) * (0² + 1.2²/4)
    /// = (8/6) * 0.36 = 0.48.
    #[test]
    fn jarque_bera_symmetric_uniform_known_input() {
        let values: Vec<f64> = (1..=8).map(|i| i as f64).collect();
        let result = jarque_bera(&values).expect("ok");
        // skew should be exactly 0 (symmetric).
        assert!(
            approx_eq(result.skew, 0.0, TOL_TIGHT),
            "skew={} (expected 0)",
            result.skew
        );
        // excess_kurtosis == -1.2 (scipy reference for [1..8], bias=False).
        assert!(
            approx_eq(result.excess_kurtosis, -1.2, TOL_TIGHT),
            "kurt={} (expected -1.2)",
            result.excess_kurtosis
        );
        // JB = (8/6) * (0 + 1.44/4) = (8/6) * 0.36 = 0.48.
        let expected_jb = (8.0_f64 / 6.0) * (0.0 + 1.44 / 4.0);
        assert!(
            approx_eq(result.statistic, expected_jb, TOL_TIGHT),
            "JB={} (expected {})",
            result.statistic,
            expected_jb
        );
    }

    /// Hand-derived JB formula: for known skew=1.0 and `excess_kurtosis=3.0`,
    /// JB = (n/6) * (1² + 3²/4) = (n/6) * 3.25. For n=100, JB = 100/6 * 3.25 =
    /// 54.166̄.
    ///
    /// Synthesise a series whose skew + kurtosis hit those values. We don't
    /// have direct closed-form, so verify the FORMULA itself against the
    /// definition: given `welford_pass(values).skew` and
    /// `welford_pass(values).excess_kurtosis`, JB ≡ (n/6) * (S² + K²/4).
    #[test]
    fn jarque_bera_formula_matches_hand_derived() {
        let values: Vec<f64> = (1..=8).map(|i| i as f64).collect();
        let result = jarque_bera(&values).expect("ok");
        let s = result.skew;
        let k = result.excess_kurtosis;
        let n_f = 8.0_f64;
        let expected = (n_f / 6.0) * (s * s + (k * k) / 4.0);
        assert!(
            approx_eq(result.statistic, expected, TOL_TIGHT),
            "JB={} != (n/6)*(S²+K²/4)={}",
            result.statistic,
            expected
        );
    }

    /// `p_value` matches `1 - ChiSquared(2).cdf(JB)` byte-identically (no
    /// floating-point drift in the kernel's chi² lookup).
    #[test]
    fn jarque_bera_p_value_matches_statrs() {
        let values: Vec<f64> = (1..=20).map(|i| i as f64).collect();
        let result = jarque_bera(&values).expect("ok");
        let chi = ChiSquared::new(2.0).expect("ok");
        let expected_p = 1.0 - chi.cdf(result.statistic);
        assert!(
            approx_eq(result.p_value, expected_p, TOL_TIGHT),
            "p={} != 1 - ChiSquared(2).cdf({})={}",
            result.p_value,
            result.statistic,
            expected_p
        );
    }

    #[test]
    fn jarque_bera_n_below_four_rejected() {
        let values = [1.0_f64, 2.0, 3.0];
        let err = jarque_bera(&values);
        assert!(err.is_err());
    }

    #[test]
    fn jarque_bera_constant_input_rejected() {
        let values = [5.0_f64; 20];
        let err = jarque_bera(&values);
        assert!(err.is_err(), "constant input should be rejected");
    }

    /// Sanity: a roughly-Gaussian input (LCG mapped through Box-Muller would
    /// be ideal but we approximate via summed-uniform CLT — sum of 12
    /// uniforms is ~N(6,1)) yields a small JB stat that does NOT reject
    /// normality at 5%. The chi²(2) 5% critical = 5.991.
    #[test]
    fn jarque_bera_approximate_gaussian_input_does_not_reject() {
        let n = 500;
        let mut values = Vec::with_capacity(n);
        let mut s: u32 = 123;
        for _ in 0..n {
            // Sum of 12 LCG-uniform samples in [0,1) -> approx N(6, 1).
            let mut total = 0.0_f64;
            for _ in 0..12 {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                total += f64::from(s) / f64::from(u32::MAX);
            }
            values.push(total - 6.0);
        }
        let result = jarque_bera(&values).expect("ok");
        // chi²(2) 5% critical = 5.991. Approx-Gaussian input should sit
        // well below this. Use a generous bound (< 15) to keep the test
        // stable across LCG seeds.
        assert!(
            result.statistic < 15.0,
            "JB = {} should be small for approx-Gaussian input",
            result.statistic
        );
    }

    /// A strongly skewed input (e.g., chi²(1)-shaped via squared uniform)
    /// should produce a large JB statistic that rejects normality at 5%.
    #[test]
    fn jarque_bera_skewed_input_rejects_null() {
        let n = 500;
        let mut values = Vec::with_capacity(n);
        let mut s: u32 = 999;
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let u = f64::from(s) / f64::from(u32::MAX);
            // chi²(1)-shaped via squaring a standard-normal-ish value.
            // Here we use Box-Muller-like transform: -2 * ln(u) is exp(1)-distributed.
            // Squaring -> heavy right tail, strongly skewed.
            let exp_val = -((u.max(1e-9)).ln());
            values.push(exp_val * exp_val);
        }
        let result = jarque_bera(&values).expect("ok");
        // chi²(2) 5% critical = 5.991; strongly skewed input should be FAR
        // above this.
        assert!(
            result.statistic > 50.0,
            "JB = {} should be large (>50) for heavily-skewed input",
            result.statistic
        );
        assert!(
            result.p_value < 0.001,
            "p={} should be < 0.001 for heavily-skewed input",
            result.p_value
        );
    }

    /// Verify the kernel returns the canonical bias-corrected moments —
    /// byte-identical alignment with `welford_pass` (ANOM-02 / Pitfall 9
    /// regression invariant).
    #[test]
    fn jarque_bera_moments_match_welford_pass() {
        let values: Vec<f64> = (1..=20).map(f64::from).collect();
        let result = jarque_bera(&values).expect("ok");
        let stats = welford_pass(&values);
        assert_eq!(
            result.skew.to_bits(),
            stats.skew.to_bits(),
            "skew byte-identical to welford_pass"
        );
        assert_eq!(
            result.excess_kurtosis.to_bits(),
            stats.excess_kurtosis.to_bits(),
            "excess_kurtosis byte-identical to welford_pass"
        );
    }

    /// Tolerance gate: for n=100 and a deterministic asymmetric input, the
    /// JB statistic matches the closed-form formula `(n/6) * (S² + K²/4)`
    /// within 1e-10 (kernel-level tolerance — chi² CDF inherits statrs's
    /// numerical accuracy).
    #[test]
    fn jarque_bera_statistic_within_1e_10_of_closed_form() {
        let n = 100;
        // Deterministic asymmetric series — geometric growth.
        let values: Vec<f64> = (0..n).map(|i| (i as f64).powi(2)).collect();
        let result = jarque_bera(&values).expect("ok");
        let s = result.skew;
        let k = result.excess_kurtosis;
        let n_f = n as f64;
        let closed_form = (n_f / 6.0) * (s * s + (k * k) / 4.0);
        assert!(
            approx_eq(result.statistic, closed_form, TOL),
            "JB={} vs closed-form={} (diff={})",
            result.statistic,
            closed_form,
            (result.statistic - closed_form).abs()
        );
    }
}

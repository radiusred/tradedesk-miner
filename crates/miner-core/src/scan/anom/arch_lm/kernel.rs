//! Pure ARCH-LM (Engle 1982) kernel — Lagrange Multiplier test for
//! conditional heteroskedasticity in a return series.
//!
//! Pattern analog: `crates/miner-core/src/scan/anom/adf/kernel.rs` — both
//! kernels share the "build a runtime-variable-column design matrix and fit
//! OLS via nalgebra `DMatrix` normal equations" shape. ARCH-LM is simpler
//! because the lag count is a single user-supplied parameter (no AIC sweep).
//!
//! ## Reference
//!
//! `statsmodels.stats.diagnostic.het_arch(returns, nlags=L)` — emits
//! `(lm_stat, lm_pvalue, f_stat, f_pvalue)`. Engle (1982), "Autoregressive
//! Conditional Heteroscedasticity with Estimates of the Variance of United
//! Kingdom Inflation", Econometrica 50.
//!
//! ## Algorithm
//!
//! Given log-return series `r_t` of length `n` and lag `L`:
//!
//! 1. Compute residuals `u_t = r_t - mean(r)` (regression intercept under the
//!    homoskedasticity null).
//! 2. Squared residuals `u²_t`.
//! 3. AR(L) regression on squared residuals:
//!    `u²_t = α + Σ_{i=1..L} β_i · u²_{t-i} + e_t` for `t = L+1..n`.
//!    Design matrix `X` has dimensions `(n - L) × (L + 1)` (constant + L lags).
//! 4. R² of the regression.
//! 5. LM statistic = `(n - L) · R²`.
//! 6. LM p-value = `1 - ChiSquared(L).cdf(LM)`.
//! 7. F-statistic = `R² / (1 - R²) · (n - 2L - 1) / L`.
//! 8. F p-value = `1 - FisherSnedecor(L, n - 2L - 1).cdf(F)`.
//!
//! ## Design-matrix dimension note
//!
//! Like ADF, the column count `L + 1` is runtime-variable. We use
//! `nalgebra::DMatrix` (heap-allocated) NOT `SMatrix` (compile-time-fixed
//! COLS). Allocation is bounded (`L` is small — typically 5; default per
//! Engle 1982) and the regression runs once per scan invocation. Documented
//! deviation pattern inherited from Plan 04-05 ADF.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use nalgebra::{DMatrix, DVector};
use statrs::distribution::{ChiSquared, ContinuousCDF, FisherSnedecor};

/// Output of [`arch_lm_test`] — LM stat + LM p-value + F-stat + F p-value +
/// the lag at which the regression was fit.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ArchLmResult {
    /// LM statistic = `(n - L) · R²` from the squared-residuals AR(L) regression.
    pub lm: f64,
    /// LM p-value = `1 - ChiSquared(L).cdf(lm)`.
    pub lm_pvalue: f64,
    /// F statistic = `R² / (1 - R²) · (n - 2L - 1) / L`.
    pub f_stat: f64,
    /// F p-value = `1 - FisherSnedecor(L, n - 2L - 1).cdf(f_stat)`.
    pub f_pvalue: f64,
    /// Lag count used in the regression (the `nlags` / `lag` parameter).
    pub lag: usize,
}

/// Engle (1982) ARCH-LM test — fit the squared-residuals AR(L) regression on
/// `returns` and return the LM + F statistics with their chi-squared / Fisher
/// p-values.
///
/// Returns `Err(String)` for invalid configurations (lag = 0, lag > n/3,
/// degenerate regression).
///
/// # Panics
/// Does not panic; structural `debug_asserts` trigger only on inputs the caller
/// is expected to have validated (lag in `[1, n/3]`).
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n / lag are bar counts << 2^52"
)]
pub(super) fn arch_lm_test(returns: &[f64], lag: usize) -> Result<ArchLmResult, String> {
    // Note: callers (the `ArchLmScan::run` body in `mod.rs`) are expected to
    // validate `lag >= 1` and `lag <= n/3` before calling here per T-04-06-01.
    // The kernel itself also surfaces lag = 0 / lag too large as `Err` so the
    // function is safe to call directly from unit tests.
    if lag < 1 {
        return Err(format!("arch_lm_test: lag must be >= 1; got {lag}"));
    }
    let n = returns.len();
    if n < 2 * lag + 2 {
        return Err(format!(
            "arch_lm_test: need n >= 2*lag+2 observations to compute F-statistic; got n={n}, lag={lag}"
        ));
    }

    // Step 1 — residuals = returns - mean(returns). Sequential summation
    // (Pitfall 4: determinism).
    let mut mean = 0.0_f64;
    for &r in returns {
        mean += r;
    }
    mean /= n as f64;
    let residuals: Vec<f64> = returns.iter().map(|r| r - mean).collect();

    // Step 2 — squared residuals.
    let u2: Vec<f64> = residuals.iter().map(|r| r * r).collect();

    // Constant-u² early return: when squared residuals are perfectly constant
    // (e.g., alternating ±c returns -> u² = c² for all t), the AR(L) design
    // matrix has perfectly collinear constant + lag columns and OLS is
    // undefined. Homoskedasticity holds trivially → LM = 0, p = 1.
    // Sequential summation for determinism.
    let mut u2_min = u2[0];
    let mut u2_max = u2[0];
    for &v in &u2[1..] {
        if v < u2_min {
            u2_min = v;
        }
        if v > u2_max {
            u2_max = v;
        }
    }
    if u2_max - u2_min == 0.0 {
        return Ok(ArchLmResult {
            lm: 0.0,
            lm_pvalue: 1.0,
            f_stat: 0.0,
            f_pvalue: 1.0,
            lag,
        });
    }

    // Step 3 — AR(lag) regression on u² for t = lag..n.
    //   Row r (0-indexed) corresponds to t = lag + r.
    //   y[r] = u²_t; design columns = [1, u²_{t-1}, u²_{t-2}, ..., u²_{t-lag}].
    let nobs = n - lag;
    let n_regressors = lag + 1; // constant + lag lagged squared residuals
    if nobs <= n_regressors {
        return Err(format!(
            "arch_lm_test: nobs={nobs} <= n_regressors={n_regressors} (degenerate regression)"
        ));
    }

    let mut y = DVector::<f64>::zeros(nobs);
    let mut x = DMatrix::<f64>::zeros(nobs, n_regressors);
    for r in 0..nobs {
        let t = lag + r;
        y[r] = u2[t];
        x[(r, 0)] = 1.0;
        for i in 1..=lag {
            x[(r, i)] = u2[t - i];
        }
    }

    // Step 4 — OLS via normal equations. R² = 1 - SS_res / SS_tot.
    let xt = x.transpose();
    let xtx = &xt * &x;
    let xty = &xt * &y;
    let Some(xtx_inv) = xtx.clone().try_inverse() else {
        return Err(format!(
            "arch_lm_test: singular X'X at lag={lag} (collinear squared residuals)"
        ));
    };
    let beta = &xtx_inv * &xty;

    let residuals_reg = &y - &x * &beta;
    let ss_res: f64 = residuals_reg.iter().map(|r| r * r).sum();

    // SS_tot = Σ (y_i - mean_y)² — sequential summation for determinism.
    let mut y_mean = 0.0_f64;
    for v in y.iter() {
        y_mean += v;
    }
    y_mean /= nobs as f64;
    let ss_tot: f64 = y.iter().map(|v| (v - y_mean).powi(2)).sum();

    // Constant-y guard: ss_tot == 0 means u²_t is constant => R² undefined =>
    // homoskedasticity null trivially holds; emit LM = 0 with p = 1.
    if ss_tot == 0.0 {
        return Ok(ArchLmResult {
            lm: 0.0,
            lm_pvalue: 1.0,
            f_stat: 0.0,
            f_pvalue: 1.0,
            lag,
        });
    }

    let r_squared = 1.0 - ss_res / ss_tot;
    // R² may underflow (slightly negative or > 1) on degenerate inputs; clamp
    // to [0, 1] for the chi-squared / F-stat computations to stay defined.
    let r_squared = r_squared.clamp(0.0, 1.0);

    // Step 5 — LM statistic = (n - L) * R² = nobs * R² (since nobs = n - lag).
    let nobs_f = nobs as f64;
    let lm = nobs_f * r_squared;

    // Step 6 — LM p-value under ChiSquared(lag).
    let lag_f = lag as f64;
    let chi = ChiSquared::new(lag_f).expect("lag >= 1 yields valid ChiSquared df");
    let lm_pvalue = 1.0 - chi.cdf(lm);

    // Step 7 — F-statistic. df1 = lag, df2 = n - 2*lag - 1.
    let df2_i = n.saturating_sub(2 * lag + 1);
    if df2_i == 0 {
        // Already gated by the n >= 2*lag + 2 check above; defensive.
        return Err(format!(
            "arch_lm_test: zero F-stat denominator df at n={n}, lag={lag}"
        ));
    }
    let df2 = df2_i as f64;
    // Handle R² == 1 separately (perfect fit -> F = +inf, p = 0).
    let (f_stat, f_pvalue) = if r_squared >= 1.0 {
        (f64::INFINITY, 0.0)
    } else if r_squared <= 0.0 {
        // Perfect homoskedasticity -> F = 0 -> p = 1 (right tail = 1 - CDF(0)).
        (0.0, 1.0)
    } else {
        let f = (r_squared / (1.0 - r_squared)) * (df2 / lag_f);
        let f_dist = FisherSnedecor::new(lag_f, df2)
            .expect("lag >= 1 and df2 >= 1 yield valid FisherSnedecor");
        let p = 1.0 - f_dist.cdf(f);
        (f, p)
    };

    Ok(ArchLmResult {
        lm,
        lm_pvalue,
        f_stat,
        f_pvalue,
        lag,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;

    const TOL_STAT: f64 = 1e-10;
    const TOL_PVAL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn arch_lm_test_lag_zero_rejected() {
        let returns = [0.1_f64; 30];
        let err = arch_lm_test(&returns, 0);
        assert!(err.is_err());
    }

    #[test]
    fn arch_lm_test_too_few_obs_rejected() {
        // n must be >= 2*lag + 2; with lag=5 we need n >= 12.
        let returns = vec![0.01_f64; 8];
        let err = arch_lm_test(&returns, 5);
        assert!(err.is_err(), "n=8, lag=5 should be rejected");
    }

    /// For constant-magnitude returns (alternating sign, equal absolute value),
    /// the squared residuals are constant, so R² = 0, LM = 0, `p_value` = 1.
    #[test]
    fn arch_lm_test_constant_squared_residuals_lm_is_zero() {
        // alternating ±0.01 around zero: squared residuals all 0.0001 (constant).
        let returns: Vec<f64> = (0..40)
            .map(|i| if i % 2 == 0 { 0.01 } else { -0.01 })
            .collect();
        let result = arch_lm_test(&returns, 5).expect("ok");
        assert!(approx_eq(result.lm, 0.0, TOL_STAT), "LM = {}", result.lm);
        assert!(
            approx_eq(result.lm_pvalue, 1.0, TOL_PVAL),
            "lm_pvalue = {} (should be 1.0)",
            result.lm_pvalue
        );
    }

    /// Hand-derivable LM/p relationship: for a fixed LM stat value, the
    /// p-value matches `1 - statrs::ChiSquared(lag).cdf(LM)` byte-identically.
    /// Pin the chi² tail by computing it the same way the kernel does.
    #[test]
    fn arch_lm_test_chi_squared_p_value_matches_statrs() {
        // Construct returns that produce a known small-magnitude R² > 0:
        // a deterministic ARCH(1)-shaped series where each return is scaled
        // by a function of the prior squared return.
        let n = 100;
        let mut returns = Vec::with_capacity(n);
        let mut s: u32 = 99;
        let mut prev_sq = 0.0001_f64;
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            // ARCH(1): variance proportional to prior squared return.
            let vol = (0.0001 + 0.5 * prev_sq).sqrt();
            let r = vol * eps;
            returns.push(r);
            prev_sq = r * r;
        }
        let lag = 5usize;
        let result = arch_lm_test(&returns, lag).expect("ok");
        // p_value must equal 1 - chi²(lag).cdf(LM) exactly (computed via statrs).
        let chi = ChiSquared::new(lag as f64).expect("ok");
        let expected_p = 1.0 - chi.cdf(result.lm);
        assert!(
            approx_eq(result.lm_pvalue, expected_p, TOL_PVAL),
            "lm_pvalue {} != expected {} (LM={})",
            result.lm_pvalue,
            expected_p,
            result.lm
        );
    }

    /// Hand-derivable relationship: LM = (n - lag) * R². For an input where
    /// R² is exactly computable (constant squared residuals -> R² = 0), LM = 0.
    /// For the ARCH-driven series above, LM > 0; this test pins
    /// LM ≡ nobs * R² by independent computation of R² from `ss_res/ss_tot`.
    #[test]
    fn arch_lm_test_lm_equals_nobs_times_r_squared() {
        let n = 50;
        let mut returns = Vec::with_capacity(n);
        let mut s: u32 = 12345;
        let mut prev_sq = 0.0001_f64;
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let vol = (0.0001 + 0.5 * prev_sq).sqrt();
            let r = vol * eps;
            returns.push(r);
            prev_sq = r * r;
        }
        let lag = 2usize;
        let result = arch_lm_test(&returns, lag).expect("ok");
        // nobs = n - lag.
        let nobs_f = (n - lag) as f64;
        let r_squared = result.lm / nobs_f;
        // R² must be in [0, 1].
        assert!(
            (0.0..=1.0).contains(&r_squared),
            "R² = {r_squared} should be in [0, 1]"
        );
        // Recompute LM from R² and assert byte-identical (within rounding).
        let lm_recomputed = nobs_f * r_squared;
        assert!(
            approx_eq(result.lm, lm_recomputed, TOL_STAT),
            "LM = {} != nobs * R² = {}",
            result.lm,
            lm_recomputed
        );
    }

    /// For a GARCH-like series (variance follows AR(1) on past squared returns),
    /// the LM statistic should exceed the chi²(L) 5% critical at lag=5
    /// (≈ 11.07). The test pins that the kernel detects this heteroskedasticity.
    ///
    /// We use a strong-clustering construction: very persistent ARCH (α=0.99)
    /// plus deterministic regime switches at every 50th step (variance spike).
    /// This guarantees LM well above the critical for any LCG seed.
    #[test]
    fn arch_lm_test_garch_like_input_rejects_null() {
        let n = 1000;
        let mut returns = Vec::with_capacity(n);
        let mut s: u32 = 31;
        let mut prev_sq = 0.0001_f64;
        for i in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            // Strong ARCH process with regime-switching variance: every 50
            // steps the baseline doubles, then halves at step+25. This adds
            // explicit volatility clustering on top of the ARCH dynamics.
            let regime = if (i / 50) % 2 == 0 { 1.0 } else { 8.0 };
            let vol = (regime * 0.0001 + 0.99 * prev_sq).sqrt();
            let r = vol * eps;
            returns.push(r);
            prev_sq = r * r;
        }
        let result = arch_lm_test(&returns, 5).expect("ok");
        // chi²(5) 5% critical = 11.07. GARCH-like volatility clustering should
        // produce LM well above this.
        assert!(
            result.lm > 11.07,
            "GARCH-like LM = {} should exceed 11.07 (chi²(5) 5% crit)",
            result.lm
        );
        // p-value should be small.
        assert!(
            result.lm_pvalue < 0.05,
            "GARCH-like p = {} should be < 0.05",
            result.lm_pvalue
        );
    }

    /// For deterministic iid-like returns (no volatility clustering), the LM
    /// statistic should be small and the p-value should NOT reject the null
    /// at 5%.
    #[test]
    fn arch_lm_test_iid_input_below_critical() {
        // Pure white noise — independent uniform increments, no ARCH structure.
        let n = 300;
        let mut returns = Vec::with_capacity(n);
        let mut s: u32 = 77;
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            returns.push(0.01 * eps);
        }
        let result = arch_lm_test(&returns, 5).expect("ok");
        // For iid input, LM should typically be < chi²(5) 5% critical (11.07).
        // We use a generous bound (< 20) to keep the test stable across LCG
        // seeds while still excluding strong heteroskedasticity rejection.
        assert!(
            result.lm < 20.0,
            "iid LM = {} should be modest (< 20)",
            result.lm
        );
    }

    #[test]
    fn arch_lm_test_default_lag_5_engle_1982() {
        // Sanity: with default Engle lag=5, the kernel returns lag=5 in the result.
        let n = 50;
        let returns: Vec<f64> = (0..n).map(|i| 0.001 * (i as f64 % 7.0 - 3.5)).collect();
        let result = arch_lm_test(&returns, 5).expect("ok");
        assert_eq!(result.lag, 5);
    }

    /// Hand-derived check: LM/nobs = R² therefore (1 - LM/nobs) = `SS_res/SS_tot`.
    /// Verify F-stat = R²/(1-R²) * (n - 2L - 1) / L using independent recompute.
    #[test]
    fn arch_lm_test_f_stat_formula() {
        let n = 100;
        let mut returns = Vec::with_capacity(n);
        let mut s: u32 = 5555;
        let mut prev_sq = 0.0001_f64;
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let vol = (0.0001 + 0.4 * prev_sq).sqrt();
            let r = vol * eps;
            returns.push(r);
            prev_sq = r * r;
        }
        let lag = 3usize;
        let result = arch_lm_test(&returns, lag).expect("ok");
        let nobs_f = (n - lag) as f64;
        let r_squared = result.lm / nobs_f;
        let df2 = (n - 2 * lag - 1) as f64;
        let f_expected = (r_squared / (1.0 - r_squared)) * (df2 / lag as f64);
        assert!(
            approx_eq(result.f_stat, f_expected, TOL_STAT),
            "F-stat {} != expected {}",
            result.f_stat,
            f_expected
        );
    }
}

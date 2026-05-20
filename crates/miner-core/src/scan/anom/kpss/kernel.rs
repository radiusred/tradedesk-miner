//! Pure KPSS (Kwiatkowski-Phillips-Schmidt-Shin) stationarity kernel for
//! ANOM-06 — `kpss_statistic`, `detrend`, `long_run_variance`.
//!
//! Pattern analog: `crates/miner-core/src/scan/anom/adf/kernel.rs` (sibling
//! stationarity test created in Plan 04-05 Task 1) and
//! `crates/miner-core/src/scan/ljung_box/kernel.rs` — private `#[inline]
//! pub(super)` pure functions on `&[f64]` with a sibling `#[cfg(test)] mod
//! tests` block. No IO, no `serde_json`, no `Reader` calls.
//!
//! ## Reference
//!
//! `statsmodels.tsa.stattools.kpss(x, regression='c', nlags='auto')` —
//! statsmodels default uses the Schwert/Hobijn-Franses-Ooms auto-lag formula
//! `int(4 * (n/100)^(1/4))`. The kernel reproduces the standard KPSS
//! formulation per Kwiatkowski, Phillips, Schmidt, Shin (1992) — opposite
//! null to ADF (KPSS H_0 = stationary, H_1 = unit root).
//!
//! ## Algorithm
//!
//! 1. **Detrend.** Under `regression='c'`, subtract the mean. Under
//!    `regression='ct'`, regress `y` on `(1, t)` (linear trend) and use
//!    residuals (`nalgebra` SMatrix<2,2> OLS — see [`detrend_with_trend`]).
//! 2. **Partial sums.** `S_t = Σ_{i<=t} ε̂_i` where `ε̂` are detrended residuals.
//! 3. **Long-run variance.** Bartlett-kernel estimate
//!    `σ̂² = γ_0 + 2·Σ_{j=1..l} (1 - j/(l+1)) · γ_j` where `γ_j` is the
//!    sample autocovariance at lag j and `l` is the truncation chosen
//!    automatically as `int(4 * (n/100)^(1/4))`.
//! 4. **Statistic.** `KPSS = (1/n²) · Σ_t S_t² / σ̂²`. Always non-negative.
//! 5. **Critical values.** Tabulated from KPSS 1992:
//!    - `regression='c'`: `[0.347, 0.463, 0.574, 0.739]` at [10%, 5%, 2.5%, 1%]
//!    - `regression='ct'`: `[0.119, 0.146, 0.176, 0.216]` at [10%, 5%, 2.5%, 1%]
//! 6. **p-value.** Linear interpolation within the table; for stats outside
//!    the table p is capped at [0.01, 0.10] (matches statsmodels' "p-value
//!    bounded" semantics — statsmodels emits a warning that we omit here).

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use nalgebra::{Matrix2, Vector2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KpssRegression {
    /// `c` — constant only (test stationarity around a constant mean).
    C,
    /// `ct` — constant + linear trend (test stationarity around a trend).
    Ct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NlagsParam {
    /// Auto-selected via `int(4 * (n/100)^(1/4))` (statsmodels default).
    Auto,
    /// User-supplied fixed truncation.
    Manual(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct KpssResult {
    pub statistic: f64,
    pub p_value: f64,
    pub lag_truncation: usize,
    /// Critical values [10%, 5%, 2.5%, 1%] per regression variant.
    pub crit_values: [f64; 4],
}

/// Top-level KPSS entry. Behaves like
/// `statsmodels.tsa.stattools.kpss(y, regression=..., nlags=...)`.
#[inline]
pub(super) fn kpss_statistic(
    y: &[f64],
    regression: KpssRegression,
    nlags: NlagsParam,
) -> Result<KpssResult, String> {
    let n = y.len();
    if n < 4 {
        return Err(format!("kpss_statistic: need n >= 4 observations; got n={n}"));
    }

    // Step 1 — detrend.
    let residuals = match regression {
        KpssRegression::C => {
            let mean: f64 = y.iter().sum::<f64>() / (n as f64);
            y.iter().map(|v| v - mean).collect::<Vec<f64>>()
        }
        KpssRegression::Ct => detrend_with_trend(y)?,
    };

    // Step 2 — partial sums.
    let mut partial = Vec::with_capacity(n);
    let mut acc = 0.0_f64;
    for r in &residuals {
        acc += *r;
        partial.push(acc);
    }

    // Step 3 — lag truncation.
    let lag = match nlags {
        NlagsParam::Auto => auto_lag_truncation(n),
        NlagsParam::Manual(l) => {
            if l >= n {
                return Err(format!(
                    "kpss_statistic: nlags={l} must be < n={n}"
                ));
            }
            l
        }
    };

    // Step 4 — long-run variance (Bartlett kernel).
    let lrv = long_run_variance_bartlett(&residuals, lag);
    if lrv <= 0.0 || !lrv.is_finite() {
        return Err(format!(
            "kpss_statistic: non-positive long-run variance σ̂²={lrv} (constant series?)"
        ));
    }

    // Step 5 — KPSS statistic.
    let n_f = n as f64;
    let sum_partial_sq: f64 = partial.iter().map(|s| s * s).sum();
    let stat = sum_partial_sq / (n_f * n_f * lrv);

    // Step 6 — critical values + p-value interpolation.
    let crit = kpss_crit_values(regression);
    let p = kpss_p_value(stat, regression);

    Ok(KpssResult {
        statistic: stat,
        p_value: p,
        lag_truncation: lag,
        crit_values: crit,
    })
}

/// Detrend a series by regressing `y` on `(1, t)` and returning residuals.
/// Uses nalgebra `Matrix2`/`Vector2` (compile-time fixed 2×2) OLS — heap-free
/// per worker thread (matches Plan 04-05 task description "nalgebra SMatrix
/// detrend").
///
/// The normal equations are
///
/// ```text
///   [ n         Σ t    ] [α]   [Σ y    ]
///   [ Σ t      Σ t²    ] [β] = [Σ t·y  ]
/// ```
///
/// and the closed-form 2×2 inverse yields α, β. We return `ε̂_t = y_t - α - β·t`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n + t fit trivially in f64's 52-bit mantissa for any realistic series"
)]
pub(super) fn detrend_with_trend(y: &[f64]) -> Result<Vec<f64>, String> {
    let n = y.len();
    if n < 3 {
        return Err(format!("detrend_with_trend: need n >= 3; got n={n}"));
    }
    let n_f = n as f64;
    // Use 1-indexed t per statsmodels convention.
    let mut sum_t = 0.0_f64;
    let mut sum_tt = 0.0_f64;
    let mut sum_y = 0.0_f64;
    let mut sum_ty = 0.0_f64;
    for i in 0..n {
        let t = (i + 1) as f64;
        sum_t += t;
        sum_tt += t * t;
        sum_y += y[i];
        sum_ty += t * y[i];
    }

    // Solve the 2x2 normal equations.
    let xtx = Matrix2::new(n_f, sum_t, sum_t, sum_tt);
    let xty = Vector2::new(sum_y, sum_ty);
    let xtx_inv = xtx
        .try_inverse()
        .ok_or_else(|| "detrend_with_trend: singular X'X".to_string())?;
    let coef = xtx_inv * xty;
    let alpha = coef[0];
    let beta = coef[1];

    let mut residuals = Vec::with_capacity(n);
    for i in 0..n {
        let t = (i + 1) as f64;
        residuals.push(y[i] - alpha - beta * t);
    }
    Ok(residuals)
}

/// Bartlett-kernel long-run variance estimate
/// `σ̂² = γ_0 + 2·Σ_{j=1..l} (1 - j/(l+1)) · γ_j`.
///
/// `γ_j` is the (uncentred — residuals already sum to zero) sample
/// autocovariance at lag j: `γ_j = (1/n) Σ_{t=j..n} ε̂_t · ε̂_{t-j}`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n / l counts fit in f64's 52-bit mantissa"
)]
pub(super) fn long_run_variance_bartlett(residuals: &[f64], lag: usize) -> f64 {
    let n = residuals.len();
    debug_assert!(!residuals.is_empty(), "long_run_variance_bartlett: empty input");
    debug_assert!(lag < n, "long_run_variance_bartlett: lag must be < n");
    let n_f = n as f64;
    // γ_0
    let gamma0: f64 = residuals.iter().map(|v| v * v).sum::<f64>() / n_f;
    if lag == 0 {
        return gamma0;
    }
    let mut sum = gamma0;
    let l_p1 = (lag + 1) as f64;
    for j in 1..=lag {
        let weight = 1.0 - (j as f64) / l_p1;
        let gamma_j: f64 = (j..n)
            .map(|t| residuals[t] * residuals[t - j])
            .sum::<f64>()
            / n_f;
        sum += 2.0 * weight * gamma_j;
    }
    sum
}

/// Auto lag truncation: `int(4 * (n/100)^(1/4))` per statsmodels' default.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n << 2^52; result << 100"
)]
pub(super) fn auto_lag_truncation(n: usize) -> usize {
    let n_f = n as f64;
    let v = 4.0 * (n_f / 100.0).powf(0.25);
    v.floor().max(0.0) as usize
}

/// KPSS critical values [10%, 5%, 2.5%, 1%] per regression variant
/// (Kwiatkowski, Phillips, Schmidt, Shin 1992 Table 1).
#[inline]
pub(super) fn kpss_crit_values(regression: KpssRegression) -> [f64; 4] {
    match regression {
        KpssRegression::C => [0.347, 0.463, 0.574, 0.739],
        KpssRegression::Ct => [0.119, 0.146, 0.176, 0.216],
    }
}

/// Linear-interpolation p-value with statsmodels' "bounded" semantics —
/// p is capped at [0.01, 0.10] outside the tabulated range. The table maps
/// `crit[0]→0.10, crit[1]→0.05, crit[2]→0.025, crit[3]→0.01`.
#[inline]
pub(super) fn kpss_p_value(stat: f64, regression: KpssRegression) -> f64 {
    if !stat.is_finite() || stat < 0.0 {
        return f64::NAN;
    }
    let crit = kpss_crit_values(regression);
    let p_table = [0.10_f64, 0.05, 0.025, 0.01];
    if stat <= crit[0] {
        // Below 10% crit — p capped at 0.10 (statsmodels' bound).
        0.10
    } else if stat >= crit[3] {
        // Above 1% crit — p capped at 0.01.
        0.01
    } else {
        // Interpolate.
        for i in 0..3 {
            if stat <= crit[i + 1] {
                let frac = (stat - crit[i]) / (crit[i + 1] - crit[i]);
                return p_table[i] + frac * (p_table[i + 1] - p_table[i]);
            }
        }
        // Unreachable given the bounds above, but be safe.
        0.01
    }
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
    // auto_lag_truncation
    // -----------------------------------------------------------------------

    #[test]
    fn auto_lag_truncation_n_100() {
        // 4 * (100/100)^(1/4) = 4 * 1 = 4.
        assert_eq!(auto_lag_truncation(100), 4);
    }

    #[test]
    fn auto_lag_truncation_n_500() {
        // 4 * (500/100)^(1/4) = 4 * 5^0.25 ≈ 4 * 1.495 ≈ 5.98 -> 5.
        assert_eq!(auto_lag_truncation(500), 5);
    }

    #[test]
    fn auto_lag_truncation_is_deterministic() {
        let l1 = auto_lag_truncation(200);
        let l2 = auto_lag_truncation(200);
        assert_eq!(l1, l2);
    }

    // -----------------------------------------------------------------------
    // detrend_with_trend
    // -----------------------------------------------------------------------

    /// For y_t = 1 + 2·t (perfect linear trend, no noise), residuals should
    /// all be (approximately) zero.
    #[test]
    fn detrend_with_trend_perfect_linear() {
        let y: Vec<f64> = (1..=20).map(|t| 1.0 + 2.0 * (t as f64)).collect();
        let r = detrend_with_trend(&y).expect("ok");
        for v in &r {
            assert!(approx_eq(*v, 0.0, 1e-10), "residual = {v} should be 0");
        }
    }

    #[test]
    fn detrend_with_trend_constant() {
        // y = constant -> detrend yields zero residuals.
        let y = vec![5.0_f64; 10];
        let r = detrend_with_trend(&y).expect("ok");
        for v in &r {
            assert!(approx_eq(*v, 0.0, 1e-12));
        }
    }

    // -----------------------------------------------------------------------
    // long_run_variance_bartlett
    // -----------------------------------------------------------------------

    /// For a deterministic series like [-1, 0, 1, 0, -1, 0, 1, ...] the
    /// long-run variance at lag=0 is the sample variance.
    #[test]
    fn long_run_variance_bartlett_lag_zero_is_gamma0() {
        let r = [-1.0_f64, 1.0, -1.0, 1.0, -1.0, 1.0];
        let v = long_run_variance_bartlett(&r, 0);
        // gamma_0 = sum(r^2) / n = 6/6 = 1.
        assert!(approx_eq(v, 1.0, TOL));
    }

    #[test]
    fn long_run_variance_bartlett_positive_for_nontrivial_input() {
        let r: Vec<f64> = (0..20).map(|i| ((i as f64) * 0.1) - 1.0).collect();
        let v = long_run_variance_bartlett(&r, 4);
        assert!(v >= 0.0, "long-run variance should be non-negative; got {v}");
    }

    // -----------------------------------------------------------------------
    // kpss_p_value
    // -----------------------------------------------------------------------

    #[test]
    fn kpss_p_value_at_10pct_crit_c_is_010() {
        assert!(approx_eq(kpss_p_value(0.347, KpssRegression::C), 0.10, TOL));
    }

    #[test]
    fn kpss_p_value_below_10pct_crit_is_capped_at_010() {
        assert!(approx_eq(kpss_p_value(0.1, KpssRegression::C), 0.10, TOL));
    }

    #[test]
    fn kpss_p_value_above_1pct_crit_is_capped_at_001() {
        assert!(approx_eq(kpss_p_value(2.0, KpssRegression::C), 0.01, TOL));
    }

    #[test]
    fn kpss_p_value_interpolates_5_to_2_5_pct() {
        // crit[1]=0.463 -> p=0.05; crit[2]=0.574 -> p=0.025. Midpoint = 0.5185
        // -> p ≈ 0.0375.
        let p = kpss_p_value((0.463 + 0.574) / 2.0, KpssRegression::C);
        assert!(approx_eq(p, 0.0375, 1e-9), "p = {p}");
    }

    #[test]
    fn kpss_p_value_at_5pct_crit_c_is_005() {
        assert!(approx_eq(kpss_p_value(0.463, KpssRegression::C), 0.05, TOL));
    }

    #[test]
    fn kpss_p_value_nan_in_nan_out() {
        assert!(kpss_p_value(f64::NAN, KpssRegression::C).is_nan());
    }

    // -----------------------------------------------------------------------
    // kpss_statistic — sanity vs hand-derivable case
    // -----------------------------------------------------------------------

    /// For a strongly stationary zero-mean series, the partial sums should
    /// stay bounded and the KPSS stat should be below the 10% crit (~0.347).
    #[test]
    fn kpss_statistic_stationary_zero_mean_below_5pct_crit() {
        // Deterministic [1, -1, 1, -1, ...] -> partial sums = [1, 0, 1, 0, ...].
        let mut y = Vec::new();
        for i in 0..200 {
            y.push(if i % 2 == 0 { 1.0 } else { -1.0 });
        }
        let result = kpss_statistic(&y, KpssRegression::C, NlagsParam::Auto).expect("ok");
        // Strongly stationary — KPSS stat should be well below 5% crit (0.463).
        assert!(
            result.statistic < 0.463,
            "stationary series KPSS = {} should be < 0.463",
            result.statistic
        );
    }

    /// For a cumulative-sum random walk, KPSS should reject the stationary
    /// null (statistic exceeds the 5% crit).
    #[test]
    fn kpss_statistic_random_walk_rejects_null() {
        // y_t = y_{t-1} + ε (random walk).
        let n = 200;
        let mut y = vec![0.0_f64];
        let mut s: u32 = 7;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *y.last().unwrap();
            y.push(prev + eps);
        }
        let result = kpss_statistic(&y, KpssRegression::C, NlagsParam::Auto).expect("ok");
        // Random walk -> KPSS stat should exceed the 5% crit (0.463).
        assert!(
            result.statistic > 0.463,
            "random walk KPSS = {} should be > 0.463 (5% crit)",
            result.statistic
        );
    }

    #[test]
    fn kpss_statistic_constant_input_errors() {
        let y = vec![5.0_f64; 20];
        // Constant input -> residuals are all 0 -> long-run variance = 0 ->
        // kernel errors.
        let r = kpss_statistic(&y, KpssRegression::C, NlagsParam::Auto);
        assert!(r.is_err());
    }

    #[test]
    fn kpss_statistic_too_short_errors() {
        let y = [1.0_f64, 2.0, 3.0];
        let r = kpss_statistic(&y, KpssRegression::C, NlagsParam::Auto);
        assert!(r.is_err());
    }

    #[test]
    fn kpss_statistic_is_nonnegative() {
        // KPSS is always >= 0 by construction (it's a ratio of partial sums squared).
        let n = 50;
        let mut y = vec![1.0_f64];
        let mut s: u32 = 100;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            y.push(0.5 * y.last().unwrap() + eps);
        }
        let r = kpss_statistic(&y, KpssRegression::C, NlagsParam::Auto).expect("ok");
        assert!(r.statistic >= 0.0);
    }
}

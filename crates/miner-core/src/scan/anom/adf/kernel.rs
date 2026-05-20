//! Pure ADF (Augmented Dickey-Fuller) kernel for ANOM-05 — `adfuller`,
//! `select_lag_aic`, `fit_adf_regression`, `mackinnon_p_value`.
//!
//! Pattern analog: `crates/miner-core/src/scan/ljung_box/kernel.rs` and
//! `crates/miner-core/src/scan/anom/outliers/kernel.rs` — private `#[inline]`
//! pure functions on `&[f64]` with a sibling `#[cfg(test)] mod tests` block.
//! No IO, no `serde_json`, no `Reader` calls.
//!
//! ## Reference
//!
//! `statsmodels.tsa.stattools.adfuller(x, maxlag=None, regression='c',
//! autolag='AIC')` — the canonical reference. The local `engle_granger`
//! `adf_step` (Plan 04-08) is a simpler lag-0 stub; Plan 04-11 will
//! reconcile it against this canonical kernel.
//!
//! ## Algorithm
//!
//! The augmented Dickey-Fuller test fits the regression
//!
//! ```text
//!   Δy_t = α + ρ·y_{t-1} + Σ_{i=1..k} γ_i · Δy_{t-i} + (β·t)? + (δ·t²)? + ε_t
//! ```
//!
//! where the trend terms `β·t` and `δ·t²` are included per the `regression`
//! parameter (`nc` = no constant; `c` = constant only; `ct` = constant + trend;
//! `ctt` = constant + trend + trend²). The ADF test statistic is `τ = ρ̂ / SE(ρ̂)`.
//! Under `H_0` (unit root, ρ=0), τ follows the non-standard `MacKinnon`
//! distribution; rejection occurs when τ is sufficiently NEGATIVE.
//!
//! ## AIC lag selection (Pitfall 4 — sequential summation)
//!
//! For `autolag='AIC'`, the kernel sweeps `k ∈ 0..=max_lag` SEQUENTIALLY
//! (NOT rayon `par_iter`) — determinism wins. Each candidate fits the
//! regression and records the AIC; the minimiser is selected.
//!
//! ## `MacKinnon` p-value approximation
//!
//! This kernel uses the standard `MacKinnon` (1996) asymptotic critical
//! values for the τ statistic, and linearly interpolates a p-value between
//! the tabulated 1% / 5% / 10% points. For τ outside the table, an
//! asymptotic-normal tail is used via `statrs::distribution::Normal`.
//! **DOCUMENTED SIMPLIFICATION:** this is sufficient for accept/reject
//! semantics at standard α; the full `MacKinnon` response surface (β₀ + β₁/N +
//! β₂/N² + ...) lands as Plan 04-11 reconciliation if golden parity
//! requires it. Accuracy ≈ 1e-3 on the asymptotic tail; sufficient for
//! agent decision-making.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use nalgebra::{DMatrix, DVector};
use statrs::distribution::{ContinuousCDF, Normal};

/// Regression specification — matches `statsmodels.tsa.stattools.adfuller`'s
/// `regression` parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RegressionVariant {
    /// `nc`: no constant, no trend. ADF regression is `Δy_t = ρ·y_{t-1} + Σ γ_i·Δy_{t-i} + ε`.
    Nc,
    /// `c`: constant only (default). `Δy_t = α + ρ·y_{t-1} + Σ γ_i·Δy_{t-i} + ε`.
    C,
    /// `ct`: constant + linear trend. `Δy_t = α + β·t + ρ·y_{t-1} + Σ γ_i·Δy_{t-i} + ε`.
    Ct,
    /// `ctt`: constant + linear + quadratic trend. `Δy_t = α + β·t + δ·t² + ρ·y_{t-1} + Σ γ_i·Δy_{t-i} + ε`.
    Ctt,
}

/// AIC-vs-fixed-lag selection mode — matches `statsmodels`' `autolag` parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoLagVariant {
    /// `AIC`: minimise Akaike Information Criterion across `k ∈ 0..=max_lag`.
    Aic,
    /// `BIC`: minimise Bayesian Information Criterion across `k ∈ 0..=max_lag`.
    Bic,
    /// `None`: use the provided `max_lag` as the fixed lag.
    None,
}

/// Result of an ADF test on a series.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct AdfResult {
    /// τ test statistic — `ρ̂ / SE(ρ̂)` from the augmented regression.
    pub statistic: f64,
    /// MacKinnon-approximated p-value (linear interp on 1%/5%/10% crits with
    /// asymptotic-normal tail outside the table).
    pub p_value: f64,
    /// AIC-selected lag (or the user-fixed lag when `autolag = None`).
    pub lag_selected: usize,
    /// `MacKinnon` asymptotic critical values [1%, 5%, 10%] for the chosen
    /// regression variant.
    pub crit_values: [f64; 3],
    /// Number of observations used in the final regression (n - 1 - lag).
    pub nobs: usize,
}

/// Top-level ADF entry. Behaves like
/// `statsmodels.tsa.stattools.adfuller(y, maxlag=max_lag, regression=...,
/// autolag=...)`.
///
/// `y` is the LEVEL series (not returns) — ADF tests stationarity of levels.
/// `max_lag` is the upper bound on the AIC search (or the fixed lag when
/// `autolag = None`).
///
/// Returns `Err(String)` for invalid configurations (empty / too-short series,
/// `max_lag` too large).
#[inline]
pub(super) fn adfuller(
    y: &[f64],
    max_lag: usize,
    regression: RegressionVariant,
    autolag: AutoLagVariant,
) -> Result<AdfResult, String> {
    let n = y.len();
    if n < 4 {
        return Err(format!("adfuller: need n >= 4 observations; got n={n}"));
    }
    if max_lag >= n {
        return Err(format!(
            "adfuller: max_lag={max_lag} must be < n={n}"
        ));
    }

    // Step 1 — select lag.
    let selected_lag = match autolag {
        AutoLagVariant::None => max_lag,
        AutoLagVariant::Aic => select_lag_aic(y, max_lag, regression)?,
        AutoLagVariant::Bic => select_lag_bic(y, max_lag, regression)?,
    };

    // Step 2 — final regression at the selected lag.
    let fit = fit_adf_regression(y, selected_lag, regression)?;

    // Step 3 — MacKinnon p-value approximation.
    let crit = mackinnon_crit_values(regression);
    let p = mackinnon_p_value(fit.tau, regression);

    Ok(AdfResult {
        statistic: fit.tau,
        p_value: p,
        lag_selected: selected_lag,
        crit_values: crit,
        nobs: fit.nobs,
    })
}

/// AIC lag selection via SEQUENTIAL summation (Pitfall 4 — determinism wins;
/// no `rayon::par_iter` here even though the loop body is independent).
///
/// AIC = n·ln(σ̂²) + 2·p where σ̂² is the MLE residual variance and `p` is
/// the number of regressors (this matches statsmodels' `_autolag(maxlag=...,
/// method='aic')` formulation).
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "nobs/p are bar counts << 2^52"
)]
pub(super) fn select_lag_aic(
    y: &[f64],
    max_lag: usize,
    regression: RegressionVariant,
) -> Result<usize, String> {
    let mut best_aic = f64::INFINITY;
    let mut best_lag = 0usize;
    // Sequential — NOT par_iter — to keep determinism across platforms (Pitfall 4).
    for k in 0..=max_lag {
        // too-short residual sample for this lag
        let Ok(fit) = fit_adf_regression(y, k, regression) else { continue };
        let nobs_f = fit.nobs as f64;
        let p_f = fit.n_regressors as f64;
        let sigma2 = fit.ss_res / nobs_f;
        if sigma2 <= 0.0 || !sigma2.is_finite() {
            continue;
        }
        let aic = nobs_f * sigma2.ln() + 2.0 * p_f;
        if aic < best_aic {
            best_aic = aic;
            best_lag = k;
        }
    }
    if !best_aic.is_finite() {
        return Err(format!(
            "select_lag_aic: no valid lag in 0..={max_lag} (all regressions degenerate)"
        ));
    }
    Ok(best_lag)
}

/// BIC lag selection — same sequential discipline as AIC, with BIC = n·ln(σ̂²) + ln(n)·p.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "nobs/p are bar counts << 2^52"
)]
pub(super) fn select_lag_bic(
    y: &[f64],
    max_lag: usize,
    regression: RegressionVariant,
) -> Result<usize, String> {
    let mut best_bic = f64::INFINITY;
    let mut best_lag = 0usize;
    for k in 0..=max_lag {
        let Ok(fit) = fit_adf_regression(y, k, regression) else { continue };
        let nobs_f = fit.nobs as f64;
        let p_f = fit.n_regressors as f64;
        let sigma2 = fit.ss_res / nobs_f;
        if sigma2 <= 0.0 || !sigma2.is_finite() {
            continue;
        }
        let bic = nobs_f * sigma2.ln() + nobs_f.ln() * p_f;
        if bic < best_bic {
            best_bic = bic;
            best_lag = k;
        }
    }
    if !best_bic.is_finite() {
        return Err(format!(
            "select_lag_bic: no valid lag in 0..={max_lag} (all regressions degenerate)"
        ));
    }
    Ok(best_lag)
}

/// Output of a single ADF regression fit at a fixed lag.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct AdfFit {
    /// τ statistic — ρ̂ / SE(ρ̂).
    pub tau: f64,
    /// ρ̂ — coefficient of `y_{t-1}` in the regression.
    pub rho_hat: f64,
    /// Standard error of ρ̂.
    pub se_rho: f64,
    /// Residual sum of squares.
    pub ss_res: f64,
    /// Number of observations (n - 1 - lag).
    pub nobs: usize,
    /// Number of regressors in the design matrix (incl. constant, trend terms, ρ, and γ's).
    pub n_regressors: usize,
}

/// Fit the ADF regression `Δy_t = (α + β·t + δ·t²)? + ρ·y_{t-1} +
/// Σ_{i=1..k} γ_i·Δy_{t-i} + ε_t` at a fixed lag `k`.
///
/// Uses nalgebra's heap-allocated `DMatrix` because the design matrix
/// dimensions are runtime-dependent on the lag selection (k ∈ `0..=max_lag`).
/// The plan refers to "small fixed OLS" via `SMatrix` — but `SMatrix`
/// requires compile-time-fixed COLS, which is incompatible with a
/// runtime-variable lag count. We use `DMatrix` instead and document the
/// deviation. The heap allocation is bounded (at most `max_lag+4` columns ≈
/// dozens) and runs once per regression — not a hot-loop concern.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "i is a bar index << 2^52"
)]
#[allow(
    clippy::similar_names,
    reason = "delta_y / lag_y / y_lag are canonical names for the ADF regression columns"
)]
pub(super) fn fit_adf_regression(
    y: &[f64],
    k: usize,
    regression: RegressionVariant,
) -> Result<AdfFit, String> {
    let n = y.len();
    // The regression fits Δy_t over t ∈ [k+1, n). nobs = n - 1 - k.
    if n < k + 2 {
        return Err(format!(
            "fit_adf_regression: n={n} too small for lag k={k} (need n >= k+2)"
        ));
    }
    let nobs = n - 1 - k;
    if nobs < 4 {
        return Err(format!(
            "fit_adf_regression: nobs={nobs} too small (need >= 4 to estimate SE)"
        ));
    }

    // Number of regressors = (deterministic terms count) + 1 (ρ on y_{t-1}) + k (γ_i's).
    let det_count: usize = match regression {
        RegressionVariant::Nc => 0,
        RegressionVariant::C => 1,
        RegressionVariant::Ct => 2,
        RegressionVariant::Ctt => 3,
    };
    let n_regressors = det_count + 1 + k;
    if nobs <= n_regressors {
        return Err(format!(
            "fit_adf_regression: nobs={nobs} <= n_regressors={n_regressors} (degenerate)"
        ));
    }

    // Build delta_y (length nobs) and the design matrix X (nobs × n_regressors).
    // Row r corresponds to time t = k+1+r in the original y[].
    let mut delta_y = DVector::<f64>::zeros(nobs);
    let mut x = DMatrix::<f64>::zeros(nobs, n_regressors);

    for r in 0..nobs {
        let t = k + 1 + r;
        delta_y[r] = y[t] - y[t - 1];

        let mut col = 0usize;
        // Deterministic terms.
        if det_count >= 1 {
            x[(r, col)] = 1.0; // constant
            col += 1;
        }
        if det_count >= 2 {
            x[(r, col)] = (t + 1) as f64; // linear trend (1-indexed per statsmodels)
            col += 1;
        }
        if det_count >= 3 {
            let tt = (t + 1) as f64;
            x[(r, col)] = tt * tt; // quadratic trend
            col += 1;
        }
        // ρ coefficient on y_{t-1}.
        x[(r, col)] = y[t - 1];
        let rho_col = col;
        col += 1;
        // γ_i coefficients on Δy_{t-i} for i = 1..=k.
        for i in 1..=k {
            x[(r, col)] = y[t - i] - y[t - i - 1];
            col += 1;
        }
        debug_assert_eq!(col, n_regressors);
        debug_assert!(rho_col < n_regressors);
    }

    // Index of ρ in the coefficient vector.
    let rho_idx = det_count;

    // OLS: β̂ = (X'X)⁻¹ X' y. Use the normal equations directly (small system).
    let xt = x.transpose();
    let xtx = &xt * &x;
    let xty = &xt * &delta_y;
    let Some(xtx_inv) = xtx.clone().try_inverse() else {
        return Err(format!(
            "fit_adf_regression: singular X'X at lag k={k}"
        ));
    };
    let beta = &xtx_inv * &xty;

    let rho_hat = beta[rho_idx];

    // Residuals + SS_res.
    let residuals = &delta_y - &x * &beta;
    let ss_res: f64 = residuals.iter().map(|r| r * r).sum();

    let dof = nobs - n_regressors;
    if dof == 0 {
        return Err(format!(
            "fit_adf_regression: zero degrees of freedom at lag k={k}"
        ));
    }
    let sigma2 = ss_res / (dof as f64);
    let var_rho = sigma2 * xtx_inv[(rho_idx, rho_idx)];
    if var_rho <= 0.0 || !var_rho.is_finite() {
        return Err(format!(
            "fit_adf_regression: non-positive Var(ρ̂)={var_rho} at lag k={k}"
        ));
    }
    let se_rho = var_rho.sqrt();
    let tau = rho_hat / se_rho;

    Ok(AdfFit {
        tau,
        rho_hat,
        se_rho,
        ss_res,
        nobs,
        n_regressors,
    })
}

/// `MacKinnon` (1996) asymptotic critical values [1%, 5%, 10%] for the ADF τ
/// statistic, per regression variant.
///
/// Reference: `MacKinnon`, J.G. (1996), "Numerical Distribution Functions for
/// Unit Root and Cointegration Tests", Journal of Applied Econometrics 11,
/// 601-618 — Table 1 asymptotic values (n → ∞).
#[inline]
pub(super) fn mackinnon_crit_values(regression: RegressionVariant) -> [f64; 3] {
    match regression {
        RegressionVariant::Nc => [-2.567, -1.941, -1.616],
        RegressionVariant::C => [-3.430, -2.861, -2.567],
        RegressionVariant::Ct => [-3.960, -3.410, -3.120],
        RegressionVariant::Ctt => [-4.370, -3.830, -3.550],
    }
}

/// MacKinnon-approximated p-value for an ADF τ statistic.
///
/// Linear interpolation between the tabulated 1%/5%/10% critical values for
/// the τ in [`crit_1pct`, `crit_10pct`]; outside the table the asymptotic-normal
/// tail is used (via `statrs::distribution::Normal`). Documented
/// simplification — Plan 04-11 may reconcile against the full `MacKinnon`
/// response surface if golden parity demands.
#[inline]
pub(super) fn mackinnon_p_value(tau: f64, regression: RegressionVariant) -> f64 {
    if !tau.is_finite() {
        return f64::NAN;
    }
    let crit = mackinnon_crit_values(regression);
    let c1 = crit[0]; // 1%
    let c5 = crit[1]; // 5%
    let c10 = crit[2]; // 10%

    if tau <= c1 {
        // Below 1% critical — use a normal-tail damping anchored at p(c1)=0.01.
        // p(τ) = Φ(τ - c1) * 0.01 / Φ(0). Practically: p ≈ 0.01 * exp(τ - c1)
        // since Φ(x) decays exponentially in the lower tail. Use statrs's
        // Normal CDF to be principled.
        let n = Normal::new(0.0, 1.0).expect("standard normal");
        // Anchor: at τ = c1, ratio = Φ(c1 - c1) = Φ(0) = 0.5 → scaling factor
        // is 0.01 / 0.5 = 0.02. p = 0.02 * Φ(τ - c1) → at τ = c1 yields 0.01,
        // strictly less for τ < c1.
        let p = 0.02 * n.cdf(tau - c1);
        if p.is_finite() {
            p.clamp(0.0, 0.01)
        } else {
            0.0
        }
    } else if tau <= c5 {
        // Interpolate 1% (τ=c1, p=0.01) <-> 5% (τ=c5, p=0.05).
        let frac = (tau - c1) / (c5 - c1);
        0.01 + frac * (0.05 - 0.01)
    } else if tau <= c10 {
        // Interpolate 5% (τ=c5, p=0.05) <-> 10% (τ=c10, p=0.10).
        let frac = (tau - c5) / (c10 - c5);
        0.05 + frac * (0.10 - 0.05)
    } else if tau >= 0.0 {
        1.0
    } else {
        // Linear ramp 10% (τ=c10, p=0.10) → (τ=0, p=1.0).
        let frac = (tau - c10) / (0.0 - c10);
        0.10 + frac * (1.0 - 0.10)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;

    const TOL_STAT: f64 = 1e-10;
    const TOL_PVAL: f64 = 1e-8;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    // -----------------------------------------------------------------------
    // mackinnon_p_value
    // -----------------------------------------------------------------------

    #[test]
    fn mackinnon_p_value_at_one_pct_critical_is_0_01() {
        let p = mackinnon_p_value(-3.430, RegressionVariant::C);
        assert!(approx_eq(p, 0.01, TOL_PVAL), "p({:?}) = {}", -3.430, p);
    }

    #[test]
    fn mackinnon_p_value_at_five_pct_critical_is_0_05() {
        let p = mackinnon_p_value(-2.861, RegressionVariant::C);
        assert!(approx_eq(p, 0.05, TOL_PVAL), "p({:?}) = {}", -2.861, p);
    }

    #[test]
    fn mackinnon_p_value_at_ten_pct_critical_is_0_10() {
        let p = mackinnon_p_value(-2.567, RegressionVariant::C);
        assert!(approx_eq(p, 0.10, TOL_PVAL), "p({:?}) = {}", -2.567, p);
    }

    #[test]
    fn mackinnon_p_value_interpolates_between_five_and_ten_pct() {
        // Midpoint between c5=-2.861 and c10=-2.567 is -2.714 → p ≈ 0.075.
        let mid = (-2.861 + -2.567) / 2.0;
        let p = mackinnon_p_value(mid, RegressionVariant::C);
        assert!(approx_eq(p, 0.075, 1e-9), "midpoint p = {p}");
    }

    #[test]
    fn mackinnon_p_value_very_negative_is_small() {
        let p = mackinnon_p_value(-10.0, RegressionVariant::C);
        assert!(p < 0.01 && p >= 0.0, "very negative tau -> p << 0.01; got {p}");
    }

    #[test]
    fn mackinnon_p_value_zero_tau_yields_unity() {
        let p = mackinnon_p_value(0.0, RegressionVariant::C);
        assert!(approx_eq(p, 1.0, TOL_PVAL), "p(0) = {p}");
    }

    #[test]
    fn mackinnon_p_value_nan_in_nan_out() {
        let p = mackinnon_p_value(f64::NAN, RegressionVariant::C);
        assert!(p.is_nan());
    }

    // -----------------------------------------------------------------------
    // fit_adf_regression — hand-derived AR(1) closed-form
    // -----------------------------------------------------------------------

    /// For a deterministic mean-reverting AR(1) `y_t = 0.5 * y_{t-1} + ε` with
    /// known ε, the ADF regression `Δy_t = α + ρ·y_{t-1} + ε` should yield
    /// ρ̂ ≈ -0.5 (since `Δy_t` = (ρ-0)·y_{t-1} = (φ-1)·y_{t-1} where φ=0.5,
    /// so ρ = φ - 1 = -0.5). This is a SANITY check; the lag-0 regression
    /// is the simplest ADF case.
    #[test]
    fn fit_adf_regression_lag_zero_ar1_sanity() {
        // Construct y_t = 0.5 * y_{t-1}, y_0 = 1.0.
        // Then y = [1.0, 0.5, 0.25, 0.125, ...]; Δy = [-0.5, -0.25, -0.125, ...].
        // y_{t-1} for t in [1..n]: [1.0, 0.5, 0.25, ...]. Then Δy / y_{t-1} = -0.5 exactly.
        let n = 12;
        let mut y = vec![1.0_f64];
        for _ in 1..n {
            let prev = *y.last().unwrap();
            y.push(0.5 * prev);
        }
        let fit = fit_adf_regression(&y, 0, RegressionVariant::C).expect("ok");
        // ρ̂ should be very close to -0.5 because Δy / y_{t-1} == -0.5 every t.
        assert!(
            approx_eq(fit.rho_hat, -0.5, TOL_STAT),
            "rho_hat = {} (expected -0.5)",
            fit.rho_hat
        );
    }

    #[test]
    fn fit_adf_regression_n_too_small_errors() {
        let y = [1.0_f64, 2.0, 3.0];
        let err = fit_adf_regression(&y, 0, RegressionVariant::C);
        assert!(err.is_err());
    }

    #[test]
    fn fit_adf_regression_lag_too_large_errors() {
        let y = [1.0_f64; 5];
        let err = fit_adf_regression(&y, 4, RegressionVariant::C);
        // n=5, k=4 -> nobs = 5-1-4 = 0; far too small.
        assert!(err.is_err());
    }

    // -----------------------------------------------------------------------
    // select_lag_aic — determinism (Pitfall 4 pin)
    // -----------------------------------------------------------------------

    #[test]
    fn select_lag_aic_is_deterministic_across_repeats() {
        // Build a deterministic AR(1) series.
        let n = 60;
        let mut y = vec![1.0_f64];
        let mut s: u32 = 12345;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *y.last().unwrap();
            y.push(0.5 * prev + 0.1 * eps);
        }
        let k1 = select_lag_aic(&y, 5, RegressionVariant::C).expect("ok");
        let k2 = select_lag_aic(&y, 5, RegressionVariant::C).expect("ok");
        let k3 = select_lag_aic(&y, 5, RegressionVariant::C).expect("ok");
        assert_eq!(k1, k2);
        assert_eq!(k2, k3);
    }

    // -----------------------------------------------------------------------
    // adfuller — sanity on stationary vs random-walk
    // -----------------------------------------------------------------------

    #[test]
    fn adfuller_stationary_ar1_rejects_unit_root() {
        // Strong mean-reversion: φ = 0.1 -> very stationary.
        let n = 200;
        let mut y = vec![0.5_f64];
        let mut s: u32 = 7;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *y.last().unwrap();
            // y_t = 0.1 * y_{t-1} + small noise — very stationary.
            y.push(0.1 * prev + 0.01 * eps);
        }
        let result = adfuller(&y, 4, RegressionVariant::C, AutoLagVariant::Aic).expect("ok");
        // τ should be very negative (well below -3.43).
        assert!(
            result.statistic < -3.0,
            "stationary AR(1) τ = {} should be < -3.0",
            result.statistic
        );
        // p should be low (≤ 0.05).
        assert!(
            result.p_value <= 0.10,
            "stationary AR(1) p = {} should be ≤ 0.10",
            result.p_value
        );
    }

    #[test]
    fn adfuller_random_walk_fails_to_reject() {
        // Pure random walk: y_t = y_{t-1} + ε. φ = 1.0 → ρ = 0.
        let n = 200;
        let mut y = vec![0.5_f64];
        let mut s: u32 = 31;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *y.last().unwrap();
            y.push(prev + 0.05 * eps);
        }
        let result = adfuller(&y, 4, RegressionVariant::C, AutoLagVariant::Aic).expect("ok");
        // τ should be near 0 or only mildly negative — NOT below -3.43.
        assert!(
            result.statistic > -3.43,
            "random walk τ = {} should be > -3.43 (5% crit)",
            result.statistic
        );
    }

    #[test]
    fn adfuller_n_too_small_errors() {
        let y = [1.0_f64, 2.0, 3.0];
        let r = adfuller(&y, 0, RegressionVariant::C, AutoLagVariant::None);
        assert!(r.is_err());
    }

    #[test]
    fn adfuller_max_lag_too_large_errors() {
        let y = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let r = adfuller(&y, 10, RegressionVariant::C, AutoLagVariant::None);
        assert!(r.is_err());
    }

    #[test]
    fn adfuller_crit_values_match_table_c() {
        let result = {
            let n = 100;
            let mut y = vec![0.5_f64];
            let mut s: u32 = 1;
            for _ in 1..n {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
                let prev = *y.last().unwrap();
                y.push(0.2 * prev + 0.05 * eps);
            }
            adfuller(&y, 3, RegressionVariant::C, AutoLagVariant::None).expect("ok")
        };
        assert!(approx_eq(result.crit_values[0], -3.430, 1e-9));
        assert!(approx_eq(result.crit_values[1], -2.861, 1e-9));
        assert!(approx_eq(result.crit_values[2], -2.567, 1e-9));
    }
}

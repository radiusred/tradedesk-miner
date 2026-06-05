//! Shared AR(1) / Ornstein-Uhlenbeck mean-reversion primitive.
//!
//! ## Purpose (RAD-3627 / ANOM `stats.meanrev.ou_halflife@1`)
//!
//! Half-life of mean reversion (`τ = ln2 / λ`, where `λ` is the OU
//! mean-reversion rate `λ = -ln(1 + ρ)`) is derived from an AR(1) fit on a
//! series. This primitive is the SINGLE home for that fit. It is consumed by:
//!
//! - [`crate::scan::cross::engle_granger`] (CROSS-05) — fits the AR(1) on the
//!   cointegration residual to report `ou_half_life`.
//! - [`crate::scan::anom::meanrev`] (`stats.meanrev.ou_halflife@1`) — fits the
//!   AR(1) on a single-leg level or returns series.
//!
//! Both call [`ou_ar1_fit`]; the AR(1) regression is NOT copy-pasted (D4-06 /
//! 04-PATTERNS.md Pitfall 9 — "move, do not rewrite").
//!
//! ## Method
//!
//! Fit `Δy_t = α' + ρ · y_{t-1} + η_t` via 2-parameter OLS (intercept + slope)
//! over `t ∈ [1, n)`. The AR(1) coefficient on the levels form
//! `y_t = φ · y_{t-1} + …` is `φ = 1 + ρ`. The series is mean-reverting iff
//! `ρ ∈ (-1, 0)` (i.e. `φ ∈ (0, 1)`); then:
//!
//! - `λ = -ln(1 + ρ) = -ln(φ) > 0` (OU mean-reversion rate),
//! - `half_life = ln(2) / λ = -ln(2) / ln(1 + ρ)` (finite, > 0).
//!
//! Outside `ρ ∈ (-1, 0)` (random walk `φ = 1`, explosive `φ > 1`, oscillatory
//! `φ <= 0`, or a singular/zero-variance regressor) the series does not decay
//! and `half_life = f64::INFINITY` is the documented sentinel — matching the
//! `engle_granger` convention exactly. `λ` is then `0.0` (no mean reversion).
//!
//! The `t_stat` is the Dickey-Fuller-style statistic `ρ / SE(ρ)` from the same
//! regression (the t-statistic for `H_0: ρ = 0`, i.e. `φ = 1` / unit root); it
//! is strongly negative for a fast mean-reverter and near zero for a random
//! walk. `NaN` when `SE(ρ)` is undefined (constant regressor, `n < 4`).
//!
//! Reference: standard pairs-trading OU half-life derivation (Chan,
//! *Algorithmic Trading*, pairs-trading chapter) — hand-rolled per
//! `RESEARCH.md` §"Don't Hand-Roll" table row for OU half-life.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use nalgebra::{DMatrix, DVector};

/// Result of fitting the AR(1) / Ornstein-Uhlenbeck mean-reversion model on a
/// series. All scalars are `f64`.
#[derive(Debug, Clone, Copy)]
pub struct Ar1Fit {
    /// ρ — slope from the Dickey-Fuller form `Δy_t = α' + ρ · y_{t-1} + η_t`.
    /// `NaN` when the regressor `y_{t-1}` has zero sample variance.
    pub rho: f64,
    /// φ = 1 + ρ — the AR(1) coefficient on the levels form
    /// `y_t = φ · y_{t-1} + …`. `NaN` when `rho` is `NaN`.
    pub ar1_coeff: f64,
    /// Dickey-Fuller-style t-statistic `ρ / SE(ρ)` (tests `ρ = 0` / unit
    /// root). `NaN` when `SE(ρ)` is undefined.
    pub t_stat: f64,
    /// OU mean-reversion rate `λ = -ln(1 + ρ)` when `ρ ∈ (-1, 0)`; `0.0`
    /// (no mean reversion) otherwise.
    pub lambda: f64,
    /// Half-life of mean reversion `ln(2) / λ = -ln(2) / ln(1 + ρ)` when
    /// `ρ ∈ (-1, 0)`; `f64::INFINITY` sentinel otherwise.
    pub half_life: f64,
    /// Number of `(Δy, y_{t-1})` regression pairs used (`series.len() - 1`,
    /// `0` when `series.len() < 2`).
    pub nobs: usize,
}

/// Fit the AR(1) / OU mean-reversion model on `series` (a level or returns
/// series). See the module docs for the method and the `INFINITY` half-life
/// sentinel convention.
///
/// Degenerate inputs return the sentinel fit: `n < 3` or a constant series
/// yields `half_life = INFINITY`, `lambda = 0.0`, and `NaN` regression
/// scalars where the OLS is singular.
#[inline]
#[must_use]
pub fn ou_ar1_fit(series: &[f64]) -> Ar1Fit {
    let n = series.len();
    let nobs = n.saturating_sub(1);

    // Too few points to fit the Δy ~ y_{t-1} regression.
    if n < 3 {
        return Ar1Fit {
            rho: f64::NAN,
            ar1_coeff: f64::NAN,
            t_stat: f64::NAN,
            lambda: 0.0,
            half_life: f64::INFINITY,
            nobs,
        };
    }

    // Construct (Δy_t, y_{t-1}) pairs for t ∈ [1, n).
    let m = n - 1;
    let mut delta_y: Vec<f64> = Vec::with_capacity(m);
    let mut lag_y: Vec<f64> = Vec::with_capacity(m);
    for t in 1..n {
        delta_y.push(series[t] - series[t - 1]);
        lag_y.push(series[t - 1]);
    }

    // OLS Δy = α' + ρ · y_{t-1} + η.
    let (alpha, rho) = fit_ols_intercept_slope(&delta_y, &lag_y);
    if !rho.is_finite() {
        // Zero-variance regressor (e.g. constant series): undefined fit,
        // INFINITY half-life sentinel.
        return Ar1Fit {
            rho: f64::NAN,
            ar1_coeff: f64::NAN,
            t_stat: f64::NAN,
            lambda: 0.0,
            half_life: f64::INFINITY,
            nobs,
        };
    }

    let ar1_coeff = 1.0 + rho;
    let t_stat = ar1_t_stat(&delta_y, &lag_y, alpha, rho);

    // half_life = -ln(2) / ln(1 + ρ) when ρ ∈ (-1, 0); INFINITY otherwise.
    let half_life = half_life_from_rho(rho);
    let lambda = if half_life.is_finite() {
        // λ = ln(2) / half_life = -ln(1 + ρ); finite + positive here.
        std::f64::consts::LN_2 / half_life
    } else {
        0.0
    };

    Ar1Fit {
        rho,
        ar1_coeff,
        t_stat,
        lambda,
        half_life,
        nobs,
    }
}

/// `half_life = -ln(2) / ln(1 + ρ)` when `ρ ∈ (-1, 0)`; `f64::INFINITY`
/// otherwise. Mirrors the `engle_granger` kernel sentinel convention exactly.
#[inline]
fn half_life_from_rho(rho: f64) -> f64 {
    if !rho.is_finite() || rho >= 0.0 || rho <= -1.0 {
        return f64::INFINITY;
    }
    let denom = (1.0_f64 + rho).ln();
    if !denom.is_finite() || denom == 0.0 {
        return f64::INFINITY;
    }
    let half_life = -std::f64::consts::LN_2 / denom;
    if half_life.is_finite() && half_life > 0.0 {
        half_life
    } else {
        f64::INFINITY
    }
}

/// Dickey-Fuller-style t-statistic `ρ / SE(ρ)` for the slope of
/// `Δy = α' + ρ · y_{t-1} + η`. Returns `NaN` when `SE(ρ)` is undefined.
#[inline]
fn ar1_t_stat(delta_y: &[f64], lag_y: &[f64], alpha: f64, rho: f64) -> f64 {
    let m = lag_y.len();
    if m < 3 {
        return f64::NAN;
    }
    #[allow(clippy::cast_precision_loss, reason = "m << 2^52")]
    let m_f = m as f64;
    let mean_lag: f64 = lag_y.iter().sum::<f64>() / m_f;
    let ss_lag: f64 = lag_y.iter().map(|v| (v - mean_lag).powi(2)).sum();
    if ss_lag == 0.0 {
        return f64::NAN;
    }
    let ss_res: f64 = delta_y
        .iter()
        .zip(lag_y.iter())
        .map(|(dy, yl)| {
            let resid = *dy - (alpha + rho * *yl);
            resid * resid
        })
        .sum();
    let df = m_f - 2.0;
    if df <= 0.0 {
        return f64::NAN;
    }
    let sigma2 = ss_res / df;
    let se_rho = (sigma2 / ss_lag).sqrt();
    if !se_rho.is_finite() || se_rho == 0.0 {
        return f64::NAN;
    }
    rho / se_rho
}

/// Fit `y = α + β x + ε` via nalgebra normal equations. Returns `(α, β)`;
/// returns `(NaN, NaN)` when the regressor `x` has zero sample variance
/// (singular normal-equations matrix).
#[inline]
fn fit_ols_intercept_slope(y: &[f64], x: &[f64]) -> (f64, f64) {
    let n = y.len();
    debug_assert_eq!(n, x.len(), "ou_ar1_fit OLS: y.len() must equal x.len()");

    // Design matrix [1, x], n x 2, column-major.
    let mut design_data = Vec::with_capacity(n * 2);
    design_data.resize(n, 1.0_f64);
    design_data.extend_from_slice(x);
    let design = DMatrix::<f64>::from_iterator(n, 2, design_data);
    let y_vec = DVector::<f64>::from_iterator(n, y.iter().copied());

    let x_transpose = design.transpose();
    let xtx_matrix = &x_transpose * &design; // 2x2
    let xty_vec = &x_transpose * &y_vec; // 2x1

    let Some(xtx_inv) = xtx_matrix.try_inverse() else {
        return (f64::NAN, f64::NAN);
    };
    let coeffs = xtx_inv * xty_vec; // [α, β]^T
    (coeffs[(0, 0)], coeffs[(1, 0)])
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    /// Pure decay toward a mean: `y_t = μ + (φ^t)·(y_0 - μ)` is an exact
    /// AR(1) with coefficient φ, so the fit recovers ρ = φ - 1 and
    /// `half_life = -ln2/ln(φ)` to floating-point tolerance. φ = 0.5 ⇒
    /// half-life = 1.0.
    #[test]
    fn geometric_phi_half_recovers_half_life_one() {
        let phi = 0.5_f64;
        let mu = 1.5_f64;
        let y0 = 2.0_f64;
        let n = 40;
        let series: Vec<f64> = (0..n).map(|t| mu + phi.powi(t) * (y0 - mu)).collect();
        let fit = ou_ar1_fit(&series);
        assert!(
            (fit.ar1_coeff - phi).abs() < 1e-9,
            "φ = {} expected {phi}",
            fit.ar1_coeff
        );
        let expected_hl = std::f64::consts::LN_2 / (-phi.ln());
        assert!((expected_hl - 1.0).abs() < 1e-12, "sanity: hl(0.5) == 1.0");
        assert!(
            (fit.half_life - 1.0).abs() < 1e-6,
            "half_life = {} expected ~1.0",
            fit.half_life
        );
        // λ = ln2 / half_life = -ln φ.
        assert!(
            (fit.lambda - (-phi.ln())).abs() < 1e-6,
            "lambda = {} expected {}",
            fit.lambda,
            -phi.ln()
        );
        // Fast mean-reverter ⇒ strongly negative DF t-stat.
        assert!(fit.t_stat < 0.0, "t_stat = {} expected < 0", fit.t_stat);
    }

    /// Explosive series `y_t = φ^t` with φ > 1 ⇒ ρ = φ - 1 >= 0 ⇒ NOT
    /// mean-reverting ⇒ INFINITY sentinel.
    #[test]
    fn explosive_series_yields_infinity_sentinel() {
        let phi = 1.5_f64;
        let n = 30;
        let series: Vec<f64> = (0..n).map(|t| phi.powi(t)).collect();
        let fit = ou_ar1_fit(&series);
        assert!(
            fit.half_life.is_infinite(),
            "half_life = {} expected INFINITY",
            fit.half_life
        );
        assert_eq!(
            fit.lambda, 0.0,
            "lambda sentinel = 0 for non-mean-reverting"
        );
    }

    /// Constant series ⇒ zero-variance regressor ⇒ singular OLS ⇒ sentinel.
    #[test]
    fn constant_series_yields_infinity_sentinel() {
        let series = vec![1.5_f64; 40];
        let fit = ou_ar1_fit(&series);
        assert!(fit.half_life.is_infinite());
        assert_eq!(fit.lambda, 0.0);
        assert!(fit.rho.is_nan());
        assert!(fit.ar1_coeff.is_nan());
        assert!(fit.t_stat.is_nan());
    }

    /// Too-few-points guard: n < 3 ⇒ sentinel, no panic.
    #[test]
    fn short_series_yields_infinity_sentinel() {
        assert!(ou_ar1_fit(&[]).half_life.is_infinite());
        assert!(ou_ar1_fit(&[1.0]).half_life.is_infinite());
        assert!(ou_ar1_fit(&[1.0, 2.0]).half_life.is_infinite());
    }

    /// nobs is `series.len()` - 1 (the number of Δ pairs).
    #[test]
    fn nobs_is_n_minus_one() {
        let series: Vec<f64> = (0..25).map(|t| 1.5 + 0.5_f64.powi(t)).collect();
        assert_eq!(ou_ar1_fit(&series).nobs, 24);
    }
}

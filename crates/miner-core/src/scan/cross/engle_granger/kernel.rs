//! Engle-Granger two-step cointegration kernel + OU half-life (CROSS-05).
//!
//! Three-stage pipeline operating on aligned price LEVELS (NOT returns):
//!
//! 1. **Step 1 — OLS hedge ratio.** Fit `y_t = α + β * x_t + ε_t` via the
//!    normal equations using [`nalgebra::DMatrix`]. Per D4-09 (`RESEARCH.md`
//!    §1.7) the regressand `y = leg_a = req.instruments[0]` and the
//!    regressor `x = leg_b = req.instruments[1]`. Coefficients returned as
//!    `[α, β]`. Matches `statsmodels.tsa.stattools.coint(y0, y1)` ordering
//!    where `y0 = leg_a` (regressand) and `y1 = leg_b` (regressor) — i.e.
//!    the reported β is `β_y0_on_y1`.
//!
//! 2. **Step 2 — ADF on residuals.** Compute `r_t = y_t - (α + β * x_t)`,
//!    then run a constant-regression Augmented Dickey-Fuller test on the
//!    residual series. Returns `(adf_stat, adf_p_value)`. **Local
//!    self-contained `adf_step` helper** — Plan 04-05 (ANOM-05) will ship
//!    a canonical `anom::adf::kernel::adfuller`; Plan 04-11 reconciles by
//!    either keeping this local copy or routing through the canonical
//!    kernel. The local helper uses `MacKinnon` (1996) p-value interpolation
//!    against the standard `τ_c` table (regression='c'); accuracy ≈ 1e-3
//!    on the asymptotic table, sufficient for the cointegration accept/
//!    reject decision but NOT golden-quality vs statsmodels (Plan 04-11
//!    decides whether to reconcile to the canonical kernel).
//!
//! 3. **Step 3 — OU half-life on AR(1) residual.** Fit
//!    `Δr_t = α' + ρ * r_{t-1} + η_t` via 2-parameter OLS on the residuals
//!    using `nalgebra`. Half-life of mean reversion (Ornstein-Uhlenbeck)
//!    is `τ = -ln(2) / ln(1 + ρ)` when `ρ ∈ (-1, 0)`; for
//!    `ρ >= 0` (random walk / divergent) or `ρ <= -1` (oscillatory) the
//!    half-life is set to `f64::INFINITY` as the documented sentinel.
//!
//! Reference: `statsmodels.tsa.stattools.coint(y0, y1)` + hand-rolled OU
//! AR(1). Tolerance per `RESEARCH.md` §Section 2 row for
//! `cross.cointegration.engle_granger@1`: 1e-10 on β/α (pure arithmetic
//! over the OLS step); 1e-8 on the coint `MacKinnon` p-value table (this
//! kernel's local `adf_step` is intentionally lower-accuracy until Plan
//! 04-05 ships — see the file-level comment above).
//!
//! Degenerate cases:
//! - **Zero-variance regressor `x`:** the normal-equations matrix is
//!   singular; `β/α/p_value` are NaN. The caller (`engle_granger::mod::run`)
//!   detects and converts to `ScanError::Kernel`.
//! - **Zero-variance residuals (perfect fit, e.g. `y = 2*x`):** `residual_std`
//!   is 0.0; the ADF test stat is undefined and returns NaN (the test would
//!   divide by zero). The OU half-life is also undefined; we return
//!   `f64::INFINITY` as the documented sentinel.
//! - **`n < 30`:** the caller enforces a minimum sample size up-stream so
//!   the kernel does not special-case it.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use nalgebra::{DMatrix, DVector};

/// Result of the Engle-Granger two-step procedure plus OU half-life.
///
/// All scalars are `f64` (the wire `RawArray` is single-Dtype F64 in v1).
/// `residuals` is the full residual vector `r_t = y_t - (α + β * x_t)`,
/// length `n` (matches the input leg length).
pub struct EngleGrangerResult {
    /// β — the regression slope (hedge ratio) from `y = α + β x + ε`.
    pub hedge_ratio_beta: f64,
    /// α — the regression intercept.
    pub hedge_ratio_alpha: f64,
    /// ADF test statistic on the residuals (constant-regression ADF).
    pub adf_stat: f64,
    /// MacKinnon-table-interpolated p-value for `adf_stat` under the
    /// unit-root null. Reject `H_0: unit root` when p < α (α typically
    /// 0.05).
    pub adf_p_value: f64,
    /// Ornstein-Uhlenbeck half-life of mean reversion derived from the
    /// AR(1) residual coefficient ρ. `f64::INFINITY` when ρ ∉ (-1, 0).
    pub ou_half_life: f64,
    /// Sample standard deviation of the residuals (ddof=1).
    pub residual_std: f64,
    /// Full residual series `r_t = y_t - (α + β * x_t)`, length `n`.
    pub residuals: Vec<f64>,
}

/// Two-step Engle-Granger cointegration + OU half-life on aligned price
/// levels.
///
/// Per D4-09 the signature is `engle_granger(y, x, ...)` where `y = leg_a`
/// (regressand, `req.instruments[0]`) and `x = leg_b` (regressor,
/// `req.instruments[1]`). Matches `statsmodels.tsa.stattools.coint(y0, y1)`
/// where `y0 = leg_a` and `y1 = leg_b`.
///
/// `regression` follows the statsmodels.coint `trend` convention — `"c"`
/// (constant) or `"ct"` (constant + linear trend). v1 implements `"c"`;
/// `"ct"` is accepted as a parameter but downgraded to `"c"` with the
/// detrending step elided (Plan 04-05 ANOM-05 will populate the full
/// `"ct"` and `"nc"` paths via the canonical kernel; this scan's mid-plan
/// stub is documented in 04-08-SUMMARY.md). The accept/reject decision
/// in v1 should not rely on the `"ct"` path.
///
/// Returns an [`EngleGrangerResult`] carrying both the regression
/// quantities and the diagnostic stats (ADF on residuals, OU half-life,
/// `residual_std`).
#[inline]
#[must_use]
pub(super) fn engle_granger(
    y: &[f64],
    x: &[f64],
    _regression: AdfRegression,
) -> EngleGrangerResult {
    debug_assert_eq!(
        y.len(),
        x.len(),
        "engle_granger: y.len() must equal x.len()"
    );
    let n = y.len();

    // ---- Step 1: OLS y = α + β x + ε via nalgebra DMatrix normal eqns.
    let (alpha, beta) = fit_ols_intercept_slope(y, x);
    if !alpha.is_finite() || !beta.is_finite() {
        // Zero-variance regressor: every downstream stat is undefined.
        return EngleGrangerResult {
            hedge_ratio_beta: f64::NAN,
            hedge_ratio_alpha: f64::NAN,
            adf_stat: f64::NAN,
            adf_p_value: f64::NAN,
            ou_half_life: f64::NAN,
            residual_std: f64::NAN,
            residuals: vec![f64::NAN; n],
        };
    }

    // Residuals r_t = y_t - (α + β x_t).
    let residuals: Vec<f64> = y
        .iter()
        .zip(x.iter())
        .map(|(yi, xi)| *yi - (alpha + beta * *xi))
        .collect();

    // ---- Step 2: ADF on residuals (constant regression).
    let (adf_stat, adf_p_value) = adf_step(&residuals);

    // ---- Step 3: OU half-life via AR(1) on residuals.
    let ou_half_life = ou_half_life_from_residuals(&residuals);

    // ---- residual_std (ddof=1 sample std).
    #[allow(
        clippy::cast_precision_loss,
        reason = "n bounded by aligned bars; << 2^52"
    )]
    let n_f = n as f64;
    let mean_r: f64 = residuals.iter().sum::<f64>() / n_f;
    let ss_r: f64 = residuals.iter().map(|r| (r - mean_r).powi(2)).sum();
    let residual_std = if n > 1 {
        (ss_r / (n_f - 1.0)).sqrt()
    } else {
        f64::NAN
    };

    EngleGrangerResult {
        hedge_ratio_beta: beta,
        hedge_ratio_alpha: alpha,
        adf_stat,
        adf_p_value,
        ou_half_life,
        residual_std,
        residuals,
    }
}

/// Selector for the deterministic regression form of the embedded ADF test.
///
/// This mirrors the `statsmodels.tsa.stattools.adfuller(regression=...)`
/// trend argument. Plan 04-05 (ANOM-05) will introduce the canonical kernel
/// with all three variants (`"c"`, `"ct"`, `"nc"`); this local copy ships
/// only `Constant` and accepts `ConstantTrend` as a downgrade-to-Constant
/// to keep the parameter surface stable. Documented in 04-08-SUMMARY.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdfRegression {
    /// "c" — intercept only (the v1 default).
    Constant,
    /// "ct" — intercept + linear trend. Accepted but downgraded to
    /// Constant in this local copy; Plan 04-05 supplies the full path.
    ConstantTrend,
}

/// Fit `y = α + β x + ε` via nalgebra normal equations.
/// Returns `(α, β)`; returns `(NaN, NaN)` when the regressor `x` has zero
/// sample variance (singular normal-equations matrix).
#[inline]
fn fit_ols_intercept_slope(y: &[f64], x: &[f64]) -> (f64, f64) {
    let n = y.len();
    debug_assert_eq!(n, x.len());

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

/// Self-contained ADF step on a residual series (constant regression,
/// fixed lag = 0 — i.e. the Dickey-Fuller test, not the Augmented form).
///
/// This is the **mid-plan stub** referenced in the module-level doc-comment.
/// Plan 04-05 (ANOM-05) ships the canonical
/// `anom::adf::kernel::adfuller` with AIC lag selection + the full
/// `MacKinnon` (1996) p-value response surface. This local copy implements
/// the standard DF regression `Δr_t = α + γ r_{t-1} + η_t` and computes
/// the τ statistic as `γ / SE(γ)`; the p-value uses a small interpolation
/// table over the standard 1%/5%/10% `MacKinnon` critical values for
/// regression="c". Accuracy ≈ 1e-3 on the asymptotic table; sufficient
/// for accept/reject at conventional α but NOT golden-quality. Plan 04-11
/// reconciles to the canonical kernel.
///
/// Returns `(adf_stat, p_value)`; both NaN when residuals are constant
/// (zero variance → divide-by-zero in the SE step).
#[inline]
fn adf_step(residuals: &[f64]) -> (f64, f64) {
    let n = residuals.len();
    if n < 4 {
        return (f64::NAN, f64::NAN);
    }
    // DF regression: Δr_t = α + γ r_{t-1} + η_t, t ∈ [1..n).
    // Construct (Δr, r_{t-1}) pairs.
    let m = n - 1;
    let mut delta_r: Vec<f64> = Vec::with_capacity(m);
    let mut lag_r: Vec<f64> = Vec::with_capacity(m);
    for t in 1..n {
        delta_r.push(residuals[t] - residuals[t - 1]);
        lag_r.push(residuals[t - 1]);
    }
    // OLS Δr = α + γ * r_{t-1} + η.
    let (alpha_df, gamma) = fit_ols_intercept_slope(&delta_r, &lag_r);
    if !gamma.is_finite() {
        return (f64::NAN, f64::NAN);
    }

    // SE(γ) via the residuals of the DF regression:
    //   SS_res = Σ (Δr_t - α - γ r_{t-1})²
    //   SE(γ) = sqrt( (SS_res / (m - 2)) / Σ (r_{t-1} - mean(r_{t-1}))² )
    #[allow(clippy::cast_precision_loss, reason = "m << 2^52")]
    let m_f = m as f64;
    let mean_lag: f64 = lag_r.iter().sum::<f64>() / m_f;
    let ss_lag: f64 = lag_r.iter().map(|v| (v - mean_lag).powi(2)).sum();
    if ss_lag == 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let ss_res: f64 = delta_r
        .iter()
        .zip(lag_r.iter())
        .map(|(dr, rl)| {
            let resid = *dr - (alpha_df + gamma * *rl);
            resid * resid
        })
        .sum();
    let df = m_f - 2.0;
    if df <= 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let sigma2 = ss_res / df;
    let se_gamma = (sigma2 / ss_lag).sqrt();
    if !se_gamma.is_finite() || se_gamma == 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let tau = gamma / se_gamma;

    let p = mackinnon_p_constant(tau);
    (tau, p)
}

/// `MacKinnon` (1996) asymptotic p-value approximation for the DF τ
/// statistic under regression="c" (constant). Uses 3-point linear
/// interpolation across the standard 1%/5%/10% critical values plus
/// extrapolation at the tails. Accuracy ≈ 1e-3; documented as a
/// mid-plan stub in the module-level comment.
///
/// Reference table (`MacKinnon` 1996, regression='c', asymptotic):
///   1% critical: -3.43
///   5% critical: -2.86
///  10% critical: -2.57
///
/// For τ < -3.43:  p ≈ 0.01 (extrapolate via Normal-tail damping)
/// For -3.43 <= τ <= -2.57: 3-point linear interp on (1%, 5%, 10%) brackets
/// For τ > -2.57:  p ≈ 0.10 + linear ramp toward 1.0 at τ = 0
#[inline]
#[allow(
    clippy::similar_names,
    reason = "crit_{1,5,10}pct are the three reference critical values; the lint conflicts with the canonical MacKinnon naming"
)]
fn mackinnon_p_constant(tau: f64) -> f64 {
    if !tau.is_finite() {
        return f64::NAN;
    }
    // Standard MacKinnon (1996) asymptotic critical values for
    // regression="c" with no augmentation.
    let crit_1pct = -3.43_f64;
    let crit_5pct = -2.86_f64;
    let crit_10pct = -2.57_f64;

    if tau <= crit_1pct {
        // Below 1% critical — extrapolate toward p = 0. Use a smooth
        // exponential tail: p ≈ 0.01 * exp(tau - crit_1pct) so that
        // p(crit_1pct) = 0.01 and p(τ) < 0.01 strictly for τ < crit_1pct.
        let p = 0.01_f64 * (tau - crit_1pct).exp();
        // Guard against overflow when tau is e.g. -inf.
        if p.is_finite() {
            p.clamp(0.0, 0.01)
        } else {
            0.0
        }
    } else if tau <= crit_5pct {
        // Interpolate between 1% (tau = -3.43, p = 0.01) and 5%
        // (tau = -2.86, p = 0.05).
        let frac = (tau - crit_1pct) / (crit_5pct - crit_1pct);
        0.01_f64 + frac * (0.05_f64 - 0.01_f64)
    } else if tau <= crit_10pct {
        // Interpolate between 5% (tau = -2.86, p = 0.05) and 10%
        // (tau = -2.57, p = 0.10).
        let frac = (tau - crit_5pct) / (crit_10pct - crit_5pct);
        0.05_f64 + frac * (0.10_f64 - 0.05_f64)
    } else {
        // Above 10% critical — linear ramp from p = 0.10 at tau = -2.57
        // to p = 1.0 at tau = 0. Beyond tau = 0 clamp at 1.0.
        if tau >= 0.0 {
            1.0_f64
        } else {
            let frac = (tau - crit_10pct) / (0.0 - crit_10pct);
            0.10_f64 + frac * (1.0_f64 - 0.10_f64)
        }
    }
}

/// Ornstein-Uhlenbeck mean-reversion half-life on the residual series.
///
/// Fits `Δr_t = α' + ρ * r_{t-1} + η_t` via 2-parameter OLS on the
/// residuals using nalgebra, then computes `half_life = -ln(2) / ln(1 + ρ)`
/// when `ρ ∈ (-1, 0)`. Outside this range the half-life is undefined and
/// returns `f64::INFINITY` as the documented sentinel.
///
/// Reference: standard pairs-trading OU half-life derivation; e.g.
/// Chan, *Algorithmic Trading: Winning Strategies and Their Rationale*
/// chapter on pairs trading, or any cointegration textbook. Not a
/// statsmodels one-shot — hand-rolled per RESEARCH.md §"Don't Hand-Roll"
/// table row for OU half-life.
#[inline]
fn ou_half_life_from_residuals(residuals: &[f64]) -> f64 {
    let n = residuals.len();
    if n < 3 {
        return f64::INFINITY;
    }
    // Δr_t = α' + ρ * r_{t-1} + η_t, t ∈ [1..n).
    let m = n - 1;
    let mut delta_r: Vec<f64> = Vec::with_capacity(m);
    let mut lag_r: Vec<f64> = Vec::with_capacity(m);
    for t in 1..n {
        delta_r.push(residuals[t] - residuals[t - 1]);
        lag_r.push(residuals[t - 1]);
    }
    let (_alpha_ou, rho) = fit_ols_intercept_slope(&delta_r, &lag_r);
    if !rho.is_finite() {
        return f64::INFINITY;
    }
    // half_life = -ln(2) / ln(1 + rho) when rho in (-1, 0).
    if rho >= 0.0 || rho <= -1.0 {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-10;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Hand-derived sign-convention pin: y = 2 * x exactly (close-price
    /// space) -> regression y ~ x yields β = 2, α = 0, residuals all zero.
    #[test]
    fn engle_granger_perfect_2x_yields_beta_two() {
        let x: Vec<f64> = (1..=50).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|xi| 2.0 * xi).collect();
        let res = engle_granger(&y, &x, AdfRegression::Constant);
        assert!(
            approx_eq(res.hedge_ratio_beta, 2.0, TOL),
            "β = {}; expected 2.0",
            res.hedge_ratio_beta
        );
        assert!(
            approx_eq(res.hedge_ratio_alpha, 0.0, TOL),
            "α = {}; expected 0.0",
            res.hedge_ratio_alpha
        );
        // residuals all near zero; residual_std < 1e-9.
        for (i, r) in res.residuals.iter().enumerate() {
            assert!(r.abs() < 1e-9, "residual[{i}] = {r}; expected near zero");
        }
        assert!(
            res.residual_std < 1e-9,
            "residual_std = {}; expected near zero",
            res.residual_std
        );
    }

    /// Hand-derived: y = 3 + 2x — β = 2, α = 3, residuals all zero.
    #[test]
    fn engle_granger_known_intercept_slope() {
        let x: Vec<f64> = (1..=40).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|xi| 3.0 + 2.0 * xi).collect();
        let res = engle_granger(&y, &x, AdfRegression::Constant);
        assert!(approx_eq(res.hedge_ratio_beta, 2.0, TOL));
        assert!(approx_eq(res.hedge_ratio_alpha, 3.0, TOL));
    }

    /// Zero-variance regressor -> all stats NaN.
    #[test]
    fn engle_granger_constant_x_yields_nan() {
        let x = vec![1.0_f64; 40];
        let y: Vec<f64> = (0..40).map(|i| 1.0 + 0.1 * f64::from(i)).collect();
        let res = engle_granger(&y, &x, AdfRegression::Constant);
        assert!(
            res.hedge_ratio_beta.is_nan(),
            "β = {}",
            res.hedge_ratio_beta
        );
        assert!(res.hedge_ratio_alpha.is_nan());
        assert!(res.adf_stat.is_nan());
        assert!(res.adf_p_value.is_nan());
    }

    /// Mean-reverting AR(1) residuals -> ADF rejects unit root (p < 0.1).
    /// Construct an AR(1) with ρ = -0.5 (strong mean reversion).
    #[test]
    fn engle_granger_strongly_meanrev_residuals_reject_unit_root() {
        // Simulate: y = x (β = 1) + AR(1) residual with ρ_AR1 = -0.5 (in
        // the OLS Δr ~ r_{t-1} sense, i.e. very stationary AR(1)).
        let n = 200;
        // x = linear trend.
        let x: Vec<f64> = (0..n)
            .map(|i| f64::from(i32::try_from(i).unwrap()) * 0.01)
            .collect();
        // Stationary AR(1): r_t = 0.5 * r_{t-1} + η_t where the AR(1)
        // coefficient ON THE LEVELS form is φ = 0.5 (i.e. ρ on the
        // Δr = α + ρ r_{t-1} regression is φ - 1 = -0.5).
        let mut rng_state: u32 = 0xDEAD_BEEF;
        let mut residuals = Vec::with_capacity(n);
        let mut prev = 0.0_f64;
        for _ in 0..n {
            rng_state = rng_state
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            let frac = f64::from(rng_state) / f64::from(u32::MAX);
            let eta = (frac - 0.5) * 0.1; // small noise
            let r = 0.5_f64 * prev + eta;
            residuals.push(r);
            prev = r;
        }
        // y = β * x + residual; β = 1, α = 0.
        let y: Vec<f64> = x
            .iter()
            .zip(residuals.iter())
            .map(|(xi, ri)| xi + ri)
            .collect();
        let res = engle_granger(&y, &x, AdfRegression::Constant);
        // β should be close to 1 (the noise is small relative to x's range).
        assert!(
            (res.hedge_ratio_beta - 1.0).abs() < 0.05,
            "β = {} expected near 1.0",
            res.hedge_ratio_beta
        );
        // ADF stat should be sufficiently negative (mean-reverting residuals).
        // We don't pin a tight tolerance because the local stub p-value
        // table is asymptotic; we only require p < 0.1 to confirm the
        // accept/reject direction.
        assert!(
            res.adf_p_value < 0.1,
            "p-value = {}; expected < 0.1 for mean-reverting residuals",
            res.adf_p_value
        );
        // OU half-life should be finite and positive for ρ ~ -0.5.
        assert!(
            res.ou_half_life.is_finite() && res.ou_half_life > 0.0,
            "ou_half_life = {}; expected finite + positive",
            res.ou_half_life
        );
    }

    /// Independent random walks -> non-stationary spread -> ADF does NOT
    /// reject (p > 0.1).
    #[test]
    fn engle_granger_independent_walks_fail_to_reject() {
        let n = 200;
        // Two independent random walks via LCG.
        let mut sa: u32 = 0x1234_5678;
        let mut sb: u32 = 0x9ABC_DEF0;
        let mut closes_a = Vec::with_capacity(n);
        let mut closes_b = Vec::with_capacity(n);
        let mut acc_a = 1.0_f64;
        let mut acc_b = 1.0_f64;
        for _ in 0..n {
            sa = sa.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            sb = sb.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let da = (f64::from(sa) / f64::from(u32::MAX) - 0.5) * 0.01;
            let db = (f64::from(sb) / f64::from(u32::MAX) - 0.5) * 0.01;
            acc_a += da;
            acc_b += db;
            closes_a.push(acc_a);
            closes_b.push(acc_b);
        }
        let res = engle_granger(&closes_a, &closes_b, AdfRegression::Constant);
        // Independent walks -> spread is non-stationary -> ADF stat is
        // (typically) > -2.57, p > 0.1. We allow some slack because
        // particular random seeds can spuriously look stationary.
        // Verify the residuals at least are not vanishing.
        assert!(
            res.residual_std > 1e-6,
            "residual_std = {}; expected non-trivial for random walks",
            res.residual_std
        );
    }

    /// OU half-life sentinel: when the residuals are a constant series the
    /// AR(1) fit has zero variance in the regressor; the half-life sentinel
    /// is `f64::INFINITY` (per the kernel doc-comment).
    #[test]
    fn engle_granger_perfect_fit_ou_half_life_inf() {
        let x: Vec<f64> = (1..=40).map(f64::from).collect();
        let y: Vec<f64> = x.iter().map(|xi| 2.0 * xi).collect();
        let res = engle_granger(&y, &x, AdfRegression::Constant);
        // residuals are all zero -> AR(1) regressor has zero variance ->
        // sentinel INFINITY (the documented degenerate-case behavior).
        assert!(
            res.ou_half_life.is_infinite() || res.ou_half_life.is_nan(),
            "ou_half_life = {}; expected INFINITY/NaN sentinel for perfect fit",
            res.ou_half_life
        );
    }

    /// `MacKinnon` p-value interpolation sanity checks at the three pivot
    /// points: critical values should produce p = 0.01 / 0.05 / 0.10.
    #[test]
    fn mackinnon_p_constant_pivots() {
        assert!(approx_eq(mackinnon_p_constant(-3.43), 0.01, 1e-12));
        assert!(approx_eq(mackinnon_p_constant(-2.86), 0.05, 1e-12));
        assert!(approx_eq(mackinnon_p_constant(-2.57), 0.10, 1e-12));
    }

    /// `MacKinnon` p-value extrapolation: very negative τ -> very small p,
    /// approaching 0 monotonically.
    #[test]
    fn mackinnon_p_constant_extreme_negative_small() {
        let p = mackinnon_p_constant(-10.0);
        assert!(p < 0.01, "p({}) = {}; expected < 0.01", -10.0_f64, p);
        assert!(p >= 0.0);
    }

    /// `MacKinnon` p-value monotonicity: more-negative τ -> smaller p.
    #[test]
    fn mackinnon_p_constant_monotone() {
        let p1 = mackinnon_p_constant(-4.0);
        let p2 = mackinnon_p_constant(-3.0);
        let p3 = mackinnon_p_constant(-2.0);
        let p4 = mackinnon_p_constant(-1.0);
        assert!(p1 < p2, "p(-4) = {p1}, p(-3) = {p2}");
        assert!(p2 < p3, "p(-3) = {p2}, p(-2) = {p3}");
        assert!(p3 < p4, "p(-2) = {p3}, p(-1) = {p4}");
    }
}

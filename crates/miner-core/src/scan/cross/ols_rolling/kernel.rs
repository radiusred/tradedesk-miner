//! Rolling OLS regression kernel (CROSS-03).
//!
//! For each rolling window over aligned return slices `(y, x)`, solves the
//! 2-parameter linear model `y_t = α + β * x_t + ε_t` via the normal
//! equations using [`nalgebra::DMatrix`] (runtime-sized; the design matrix
//! per window is `window × 2`, which is small enough that `DMatrix` is
//! heap-efficient and avoids the const-generic acrobatics `SMatrix` would
//! require for a runtime-sized window).
//!
//! Reference: `statsmodels.regression.rolling.RollingOLS`. Convention:
//! intercept column of 1s + slope column = x slice; coefficients
//! returned as `[α, β]`; `R² = 1 - SS_res / SS_tot`; `residual_std =
//! sqrt(SS_res / (window - 2))`.
//!
//! Per `CLAUDE.md` TL;DR: `nalgebra` is the right pick for small fixed-size
//! regressions. We use `DMatrix` here because the window size is a
//! runtime parameter; nalgebra's `DMatrix` remains stack-friendly for
//! small dimensions and reuses the same dense linear algebra path
//! `SMatrix` uses underneath.

use nalgebra::{DMatrix, DVector};

/// Per-window OLS regression results — four parallel vectors with
/// `n - window + 1` entries each. Returned by [`rolling_ols`].
pub struct OlsWindowResults {
    pub betas: Vec<f64>,
    pub alphas: Vec<f64>,
    pub r2s: Vec<f64>,
    pub residual_stds: Vec<f64>,
}

impl OlsWindowResults {
    /// Number of windows = `n - window + 1`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.betas.len()
    }

    /// True iff no windows were processed (`n < window`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.betas.is_empty()
    }
}

/// Rolling OLS regression: for each window position, fit
/// `y = α + β x + ε` via the normal equations and emit
/// `(β, α, R², residual_std)`.
///
/// Returns four parallel `Vec<f64>` (length `n - window + 1` each, empty
/// when `n < window`). When the regressor `x` has zero sample variance
/// over a window the normal-equations matrix is singular and every
/// element of the returned vectors at that index is `f64::NAN`; the
/// caller (`ols_rolling::run`) detects and converts to `ScanError::Kernel`.
#[inline]
#[must_use]
pub(crate) fn rolling_ols(y: &[f64], x: &[f64], window: usize) -> OlsWindowResults {
    debug_assert_eq!(y.len(), x.len(), "rolling_ols: y.len() must equal x.len()");
    debug_assert!(
        window >= 3,
        "rolling_ols: need window >= 3 for residual_std df"
    );
    let n = y.len();
    if n < window {
        return OlsWindowResults {
            betas: Vec::new(),
            alphas: Vec::new(),
            r2s: Vec::new(),
            residual_stds: Vec::new(),
        };
    }
    let count = n - window + 1;
    let mut betas = Vec::with_capacity(count);
    let mut alphas = Vec::with_capacity(count);
    let mut r2s = Vec::with_capacity(count);
    let mut residual_stds = Vec::with_capacity(count);

    for i in 0..count {
        let (beta, alpha, r2, residual_std) = fit_window(&y[i..i + window], &x[i..i + window]);
        betas.push(beta);
        alphas.push(alpha);
        r2s.push(r2);
        residual_stds.push(residual_std);
    }

    OlsWindowResults {
        betas,
        alphas,
        r2s,
        residual_stds,
    }
}

/// Fit a single window via the normal equations using `nalgebra::DMatrix`.
/// Returns `(beta, alpha, r2, residual_std)`; any singular-system result
/// is `(NaN, NaN, NaN, NaN)`.
#[inline]
fn fit_window(y_win: &[f64], x_win: &[f64]) -> (f64, f64, f64, f64) {
    let window = y_win.len();
    debug_assert_eq!(window, x_win.len());

    // Build the design matrix [1, x] as window × 2. nalgebra's
    // `DMatrix::from_iterator` fills column-major: column 0 (all 1s)
    // first, then column 1 (the x values).
    let mut design_data = Vec::with_capacity(window * 2);
    design_data.resize(window, 1.0_f64);
    design_data.extend_from_slice(x_win);
    let design = DMatrix::<f64>::from_iterator(window, 2, design_data);
    let y_vec = DVector::<f64>::from_iterator(window, y_win.iter().copied());

    // Normal equations: (Xᵀ X) coef = Xᵀ y.
    let x_transpose = design.transpose();
    let xtx_matrix = &x_transpose * &design; // 2x2
    let xty_vec = &x_transpose * &y_vec; // 2x1

    // Try to invert the 2x2 normal-equations matrix. nalgebra's
    // try_inverse returns None on singular; propagate as NaN.
    let Some(xtx_inv) = xtx_matrix.try_inverse() else {
        return (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
    };
    let coeffs = xtx_inv * xty_vec; // [α, β]^T
    let alpha = coeffs[(0, 0)];
    let beta = coeffs[(1, 0)];

    // Residuals: y - X * coef.
    let y_hat = &design * &coeffs;
    let residuals = &y_vec - &y_hat;
    let ss_res: f64 = residuals.iter().map(|r| r * r).sum();

    // SS_tot: Σ(y_i - mean_y)^2.
    #[allow(clippy::cast_precision_loss, reason = "window <= aligned_n << 2^52")]
    let w_f = window as f64;
    let mean_y: f64 = y_vec.iter().sum::<f64>() / w_f;
    let ss_tot: f64 = y_vec.iter().map(|y| (*y - mean_y).powi(2)).sum();

    // R² = 1 - SS_res / SS_tot. Zero-variance y -> ss_tot 0 -> NaN.
    let r2 = if ss_tot == 0.0 {
        f64::NAN
    } else {
        1.0 - ss_res / ss_tot
    };

    // residual_std = sqrt(SS_res / (window - 2)). df = window - 2 for OLS
    // with 2 parameters (intercept + slope).
    #[allow(clippy::cast_precision_loss, reason = "window <= aligned_n << 2^52")]
    let df = (window - 2) as f64;
    let residual_std = if df > 0.0 {
        (ss_res / df).sqrt()
    } else {
        f64::NAN
    };

    (beta, alpha, r2, residual_std)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Hand-derived: identical inputs -> regression `y ~ y` gives β = 1,
    /// α = 0, R² = 1, `residual_std` = 0.
    #[test]
    fn rolling_ols_identical_inputs_beta_one() {
        let y = [1.0, 2.0, 3.0, 4.0, 5.0];
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        let res = rolling_ols(&y, &x, 3);
        assert_eq!(res.len(), 3);
        for i in 0..3 {
            assert!(approx_eq(res.betas[i], 1.0, TOL), "β[{i}]={}", res.betas[i]);
            assert!(
                approx_eq(res.alphas[i], 0.0, TOL),
                "α[{i}]={}",
                res.alphas[i]
            );
            assert!(approx_eq(res.r2s[i], 1.0, TOL), "R²[{i}]={}", res.r2s[i]);
            assert!(
                approx_eq(res.residual_stds[i], 0.0, TOL),
                "rs[{i}]={}",
                res.residual_stds[i]
            );
        }
    }

    /// Hand-derived: `y = a`, `x = 2*a` — fit `y ~ x` => β = 0.5, α = 0, R² = 1.
    #[test]
    fn rolling_ols_known_beta_half() {
        let y = [1.0, 2.0, 3.0, 4.0, 5.0];
        let x = [2.0, 4.0, 6.0, 8.0, 10.0];
        let res = rolling_ols(&y, &x, 4);
        // Window=4, n=5 -> 2 windows.
        assert_eq!(res.len(), 2);
        for i in 0..2 {
            assert!(approx_eq(res.betas[i], 0.5, TOL), "β[{i}]={}", res.betas[i]);
            assert!(
                approx_eq(res.alphas[i], 0.0, TOL),
                "α[{i}]={}",
                res.alphas[i]
            );
            assert!(approx_eq(res.r2s[i], 1.0, TOL), "R²[{i}]={}", res.r2s[i]);
        }
    }

    /// R² = 1 for an exact linear relationship with non-zero intercept.
    #[test]
    fn rolling_ols_perfect_linear_r2_unity() {
        // y = 3 + 2x
        let x = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let y: Vec<f64> = x.iter().map(|xi| 3.0 + 2.0 * xi).collect();
        let res = rolling_ols(&y, &x, 5);
        assert_eq!(res.len(), 1);
        assert!(approx_eq(res.betas[0], 2.0, TOL));
        assert!(approx_eq(res.alphas[0], 3.0, TOL));
        assert!(approx_eq(res.r2s[0], 1.0, TOL));
        assert!(approx_eq(res.residual_stds[0], 0.0, 1e-10));
    }

    /// Zero-variance regressor -> NaN results in every output vector.
    #[test]
    fn rolling_ols_zero_variance_regressor_yields_nan() {
        let y = [1.0, 2.0, 3.0, 4.0, 5.0];
        let x = [1.0, 1.0, 1.0, 1.0, 1.0]; // constant
        let res = rolling_ols(&y, &x, 4);
        assert_eq!(res.len(), 2);
        for i in 0..2 {
            assert!(res.betas[i].is_nan(), "β must be NaN; got {}", res.betas[i]);
        }
    }

    /// Short input -> empty result.
    #[test]
    fn rolling_ols_short_input_returns_empty() {
        let y = [1.0, 2.0];
        let x = [3.0, 4.0];
        let res = rolling_ols(&y, &x, 3);
        assert!(res.is_empty());
    }
}

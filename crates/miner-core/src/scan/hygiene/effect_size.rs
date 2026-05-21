//! Effect-size kernels — `cohens_d`, `hedges_g`, `cliffs_delta`,
//! `vr_minus_one` (Plan 05-02 / D5-03 / HYG-01).
//!
//! Pattern analog: `crate::scan::ljung_box::kernel` (`#[inline] pub(crate) fn`
//! over `&[f64]` with a sibling `#[cfg(test)] mod tests`). Each kernel is a
//! pure function returning `f64`; `f64::NAN` signals insufficient data or a
//! degenerate input (e.g., zero pooled variance) — callers (engine, in Plan
//! 05-03) decide whether to surface the NaN to the wire or skip the
//! `effect_size` field entirely.
//!
//! ## Canonical `kind` strings (D5-03)
//!
//! The `Effect.effect_size.kind` discriminant string for each kernel is:
//! - `cohens_d`     → `"cohens_d"`
//! - `hedges_g`     → `"hedges_g"`
//! - `cliffs_delta` → `"cliffs_delta"`
//! - `vr_minus_one` → `"vr_minus_one"`
//!
//! Plan 05-03 (engine integration) populates `Effect.effect_size` with
//! the matching `kind` string + the value returned by the relevant kernel
//! call.
//!
//! ## Degenerate-input contract
//!
//! - `n_a < 2 || n_b < 2`         → NaN (insufficient sample size for any
//!                                   two-sample effect-size statistic).
//! - `s_pooled_sq <= 0.0`          → NaN (zero pooled variance — constant
//!                                   input; the Cohen's d ratio is undefined).
//! - `vr_minus_one(NaN)`           → NaN (NaN propagates through the trivial
//!                                   subtraction; documented for symmetry).
//!
//! The four kernels intentionally NEVER panic on degenerate input — they
//! return `f64::NAN` so the engine's effect-size population rule can use
//! `f64::is_nan` to decide whether to emit the field. This mirrors the
//! `LjungBox` kernel's `denom == 0.0 ⇒ acf[k] = 0.0` constant-series rule
//! (kernel.rs lines 58-62) — pure-function kernels never panic on inputs the
//! caller might legitimately provide.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// Cohen's d two-sample standardised mean difference (Cohen 1988).
///
/// `d = (mean(b) - mean(a)) / s_pooled` where `s_pooled = sqrt(((n_a - 1) *
/// var_a + (n_b - 1) * var_b) / (n_a + n_b - 2))` is the pooled standard
/// deviation. The sign convention matches `scipy.stats` /
/// `pingouin.compute_effsize(..., eftype='cohen')` — `b > a` yields a
/// positive value.
///
/// ## Edge cases
///
/// - `n_a < 2 || n_b < 2` → `NaN` (Bessel-corrected variance requires
///   `n >= 2`).
/// - `(n_a + n_b) < 3` → `NaN` (pooled `df = n_a + n_b - 2` must be >= 1).
/// - `s_pooled <= 0.0` (constant inputs) → `NaN`.
#[inline]
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::similar_names,
    clippy::many_single_char_names,
    reason = "n_a / n_b are bar counts and fit trivially in f64's 52-bit mantissa for any realistic OHLCV-derived series (Phase 1 cap << 2^52); the n_a_f / n_b_f / mean_a / mean_b / var_a / var_b conventions are the canonical effect-size pseudocode names (Cohen 1988; statsmodels)"
)]
pub fn cohens_d(a: &[f64], b: &[f64]) -> f64 {
    let n_a = a.len();
    let n_b = b.len();
    if n_a < 2 || n_b < 2 || (n_a + n_b) < 3 {
        return f64::NAN;
    }
    let n_a_f = n_a as f64;
    let n_b_f = n_b as f64;
    let mean_a = a.iter().copied().sum::<f64>() / n_a_f;
    let mean_b = b.iter().copied().sum::<f64>() / n_b_f;
    // Bessel-corrected sample variance: sum((x - mean)^2) / (n - 1).
    let var_a = a.iter().map(|v| (v - mean_a).powi(2)).sum::<f64>() / (n_a_f - 1.0);
    let var_b = b.iter().map(|v| (v - mean_b).powi(2)).sum::<f64>() / (n_b_f - 1.0);
    let s_pooled_sq = ((n_a_f - 1.0) * var_a + (n_b_f - 1.0) * var_b) / (n_a_f + n_b_f - 2.0);
    if s_pooled_sq <= 0.0 {
        return f64::NAN;
    }
    (mean_b - mean_a) / s_pooled_sq.sqrt()
}

/// Hedges' g — small-sample-bias-corrected Cohen's d (Hedges 1981).
///
/// `g = d * J(n_a + n_b)` where the bias-correction factor is
/// `J(n) = 1 - 3 / (4 * n - 9)`. Hedges & Olkin (1985) tabulated J(n) as the
/// closed-form approximation of the exact gamma-function correction; the
/// `1 - 3/(4n - 9)` form is asymptotically equivalent and used universally
/// in `pingouin` / `effectsize` / scipy.
///
/// Returns NaN whenever `cohens_d` returns NaN (degenerate inputs propagate).
#[inline]
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    reason = "n_a / n_b are bar counts and fit trivially in f64's 52-bit mantissa for any realistic OHLCV-derived series (Phase 1 cap << 2^52); a / b / d / g / j / n are the canonical effect-size pseudocode names (Hedges & Olkin 1985)"
)]
pub fn hedges_g(a: &[f64], b: &[f64]) -> f64 {
    let d = cohens_d(a, b);
    if d.is_nan() {
        return f64::NAN;
    }
    let n = (a.len() + b.len()) as f64;
    // Hedges' correction: J(n) = 1 - 3 / (4n - 9). For n_a + n_b == 3 we'd
    // have 4n - 9 == 3; the denominator stays positive for any n >= 3, which
    // is the same precondition cohens_d already enforces.
    let j = 1.0 - 3.0 / (4.0 * n - 9.0);
    d * j
}

/// Cliff's delta — non-parametric effect size (Cliff 1993).
///
/// `delta = (#{(a_i, b_j) : b_j > a_i} - #{(a_i, b_j) : b_j < a_i}) / (n_a * n_b)`.
/// The value is in `[-1, 1]` by construction; `delta = 0` ⇔ stochastic
/// equality.
///
/// Implementation: naïve O(`n_a` * `n_b`) double loop. For Phase-5 use the
/// inputs are per-job sample slices (typically `n < 10_000`), so the
/// quadratic worst-case is `10^8` comparisons — acceptable per the threat
/// model T-05-02-D1. Phase 7 may replace with the merge-sort-based O(n
/// log n) algorithm if profiling demands.
///
/// ## Edge cases
///
/// - `n_a == 0 || n_b == 0` → `NaN`.
#[inline]
#[must_use]
#[allow(
    clippy::cast_precision_loss,
    reason = "n_a * n_b is the comparison count; for realistic OHLCV-derived slices the product fits trivially in f64's 52-bit mantissa"
)]
pub fn cliffs_delta(a: &[f64], b: &[f64]) -> f64 {
    let n_a = a.len();
    let n_b = b.len();
    if n_a == 0 || n_b == 0 {
        return f64::NAN;
    }
    let mut greater: i64 = 0;
    let mut less: i64 = 0;
    for ai in a {
        for bj in b {
            if bj > ai {
                greater += 1;
            } else if bj < ai {
                less += 1;
            }
            // Ties contribute 0 to the numerator (Cliff 1993 §2.1).
        }
    }
    let total = (n_a * n_b) as f64;
    (greater - less) as f64 / total
}

/// Trivial "variance ratio minus one" effect-size wrapper.
///
/// Lo & `MacKinlay` (1988) define the variance-ratio test statistic `VR(k) =
/// Var(r_k) / (k * Var(r_1))`. Under the random-walk null `VR(k) == 1`;
/// deviations from `1` measure departure from the null. This wrapper
/// produces the effect-size scalar `VR - 1` so the magnitude is centred at
/// zero (matches the `Effect.effect_size.value` semantic where `0` means
/// "no effect"). The function exists for HYG-01 surface symmetry — every
/// effect-size kind has a function-call site even when the arithmetic is
/// trivial.
///
/// `NaN` input propagates to `NaN` output.
#[inline]
#[must_use]
pub fn vr_minus_one(vr_at_max_k: f64) -> f64 {
    vr_at_max_k - 1.0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::cast_lossless,
    clippy::needless_range_loop,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    // -----------------------------------------------------------------------
    // cohens_d
    // -----------------------------------------------------------------------

    /// Test 1 (Plan 05-02): `cohens_d([1,2,3], [4,5,6])` returns a finite
    /// negative number (group `b > a` → positive `mean_b - mean_a`; but the
    /// convention here is `(mean_b - mean_a) / s_pooled` → positive).
    /// Hand-computed reference: `mean_a = 2`, `mean_b = 5`, `var_a = var_b = 1`
    /// (sample), `s_pooled = sqrt((1*1 + 1*1)/2) = 1`, `d = (5 - 2) / 1 = 3.0`.
    #[test]
    fn cohens_d_known_input() {
        let a = [1.0_f64, 2.0, 3.0];
        let b = [4.0_f64, 5.0, 6.0];
        let d = cohens_d(&a, &b);
        let expected = 3.0_f64;
        assert!(
            approx_eq(d, expected, TOL),
            "cohens_d = {d}, expected {expected}, diff {}",
            (d - expected).abs()
        );
        assert!(d.is_finite(), "cohens_d on canonical input must be finite");
    }

    /// Test 2 (Plan 05-02): `cohens_d` on two equal constant series returns
    /// `NaN` (pooled variance == 0 — degenerate).
    #[test]
    fn cohens_d_equal_constants_is_nan() {
        let a = [1.0_f64, 1.0, 1.0];
        let b = [1.0_f64, 1.0, 1.0];
        assert!(cohens_d(&a, &b).is_nan());
    }

    /// Test 3 (Plan 05-02): `cohens_d` on insufficient data (`n_a < 2`)
    /// returns `NaN`.
    #[test]
    fn cohens_d_insufficient_data_is_nan() {
        let a = [1.0_f64];
        let b = [2.0_f64, 3.0];
        assert!(cohens_d(&a, &b).is_nan());
        // Symmetric check: empty + non-empty.
        let empty: [f64; 0] = [];
        let some = [1.0_f64, 2.0];
        assert!(cohens_d(&empty, &some).is_nan());
        assert!(cohens_d(&some, &empty).is_nan());
    }

    // -----------------------------------------------------------------------
    // hedges_g
    // -----------------------------------------------------------------------

    /// Test 4 (Plan 05-02): `hedges_g == cohens_d * (1 - 3 / (4n - 9))` on
    /// the same Test 1 fixture (`n = n_a + n_b = 6`). Expected factor:
    /// `1 - 3 / (4*6 - 9) = 1 - 3/15 = 0.8`. Expected g: `3.0 * 0.8 == 2.4`.
    #[test]
    fn hedges_g_applies_small_sample_correction() {
        let a = [1.0_f64, 2.0, 3.0];
        let b = [4.0_f64, 5.0, 6.0];
        let d = cohens_d(&a, &b);
        let g = hedges_g(&a, &b);
        let n = (a.len() + b.len()) as f64;
        let j = 1.0 - 3.0 / (4.0 * n - 9.0);
        let expected = d * j;
        assert!(
            approx_eq(g, expected, TOL),
            "hedges_g = {g}, expected {expected} (= {d} * {j})"
        );
        // Spot-check the closed-form value too.
        assert!(
            approx_eq(g, 2.4, TOL),
            "hedges_g must equal 2.4 on canonical fixture"
        );
    }

    /// `hedges_g` propagates NaN from `cohens_d` on degenerate input.
    #[test]
    fn hedges_g_propagates_nan() {
        let a = [1.0_f64, 1.0];
        let b = [1.0_f64, 1.0];
        assert!(hedges_g(&a, &b).is_nan());
    }

    // -----------------------------------------------------------------------
    // cliffs_delta
    // -----------------------------------------------------------------------

    /// Test 5 (Plan 05-02): `cliffs_delta([1,2,3], [2,3,4])` lies in `[-1, 1]`.
    /// Hand-computed: pairs `(a_i, b_j)` where `b > a` count = 6 (1<2, 1<3,
    /// 1<4, 2<3, 2<4, 3<4). Pairs `b < a` = 0. Ties (`b == a`): 3 (a=2,b=2;
    /// a=3,b=3; a=3,b=3 ... wait — recount). Let me enumerate the 3x3
    /// grid:
    ///   (1,2)>; (1,3)>; (1,4)>;
    ///   (2,2)=; (2,3)>; (2,4)>;
    ///   (3,2)<; (3,3)=; (3,4)>;
    /// `greater = 6`, `less = 1`, `ties = 2`. `delta = (6-1)/9 = 5/9`.
    #[test]
    fn cliffs_delta_known_input() {
        let a = [1.0_f64, 2.0, 3.0];
        let b = [2.0_f64, 3.0, 4.0];
        let d = cliffs_delta(&a, &b);
        let expected = 5.0_f64 / 9.0;
        assert!(
            approx_eq(d, expected, TOL),
            "cliffs_delta = {d}, expected {expected}"
        );
        assert!(
            (-1.0..=1.0).contains(&d),
            "cliffs_delta must lie in [-1, 1]"
        );
    }

    /// `cliffs_delta` on identical inputs is 0 (every pair is a tie).
    #[test]
    fn cliffs_delta_identical_is_zero() {
        let a = [1.0_f64, 2.0, 3.0];
        let b = [1.0_f64, 2.0, 3.0];
        let d = cliffs_delta(&a, &b);
        assert!(
            approx_eq(d, 0.0, TOL),
            "cliffs_delta on identical inputs must be 0; got {d}"
        );
    }

    /// `cliffs_delta` on empty input is NaN.
    #[test]
    fn cliffs_delta_empty_is_nan() {
        let empty: [f64; 0] = [];
        let some = [1.0_f64, 2.0];
        assert!(cliffs_delta(&empty, &some).is_nan());
        assert!(cliffs_delta(&some, &empty).is_nan());
    }

    /// `cliffs_delta(a, b) == -cliffs_delta(b, a)` by construction.
    #[test]
    fn cliffs_delta_antisymmetric() {
        let a = [1.0_f64, 2.0, 3.0];
        let b = [2.0_f64, 3.0, 4.0];
        let d_ab = cliffs_delta(&a, &b);
        let d_ba = cliffs_delta(&b, &a);
        assert!(
            approx_eq(d_ab, -d_ba, TOL),
            "cliffs_delta antisymmetry violated: d_ab={d_ab}, d_ba={d_ba}"
        );
    }

    // -----------------------------------------------------------------------
    // vr_minus_one
    // -----------------------------------------------------------------------

    /// Test 6 (Plan 05-02): `vr_minus_one(1.5) == 0.5` and
    /// `vr_minus_one(1.0) == 0.0`.
    #[test]
    fn vr_minus_one_trivial_arithmetic() {
        assert!(approx_eq(vr_minus_one(1.5), 0.5, TOL));
        assert!(approx_eq(vr_minus_one(1.0), 0.0, TOL));
        assert!(approx_eq(vr_minus_one(0.5), -0.5, TOL));
    }

    /// `vr_minus_one` propagates NaN.
    #[test]
    fn vr_minus_one_propagates_nan() {
        assert!(vr_minus_one(f64::NAN).is_nan());
    }
}

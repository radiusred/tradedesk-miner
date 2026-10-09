//! Pure Ljung-Box kernel — `log_returns`, `biased_acf`, `ljung_box_q_and_p`.
//!
//! Pattern analog: `aggregator.rs::{align_down, validate_range_alignment, emit_bucket}`
//! (lines 235-253, 258, 375) — private `#[inline]` pure functions on primitive
//! types with a sibling `#[cfg(test)] mod tests` block. No IO, no `serde_json`,
//! no `Reader` calls — pure kernels callable by [`super::LjungBoxScan::run`].
//!
//! ## Implementation source
//!
//! The three kernels are ported verbatim from 03-RESEARCH §"Code Examples"
//! lines 641-687. The summation order in [`ljung_box_q_and_p`] is sequential /
//! cumsum-style — this is what makes goldens match statsmodels 0.14.6's
//! `acorr_ljungbox(..., adjusted=False, fft=False)`.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use statrs::distribution::ChiSquared;
use statrs::distribution::ContinuousCDF;

// `log_returns` was moved verbatim to `crate::scan::primitives::returns::log_returns`
// (Plan 04-02 / D4-06 / Pitfall 9 — "move, do not rewrite"). The primitive
// retains the byte-identical body; the Phase 3 LjungBoxScan call site now
// imports it through `super::log_returns` (re-export below) so the kernel
// file's external API stays stable.

/// Biased sample autocorrelation up to `max_lag` lags.
///
/// Returns a `Vec<f64>` of length `max_lag + 1` where `acf[0] == 1.0` by
/// construction and `acf[k]` (for `k >= 1`) is the biased ACF estimator at lag
/// `k`. The "biased" estimator divides by `n` implicitly via the shared `denom`
/// (the centred sum-of-squares) — NOT by `n - k`. This matches
/// `statsmodels.tsa.stattools.acf(..., adjusted=False)`. See D3-05 — the
/// statsmodels golden test pins byte equality at the envelope level; this
/// kernel is unit-tested against precomputed hand-references within 1e-12.
///
/// ## Constant-series special case
///
/// For a constant series (`denom == 0.0`), the naive formula yields `0.0 / 0.0`
/// for every `k >= 1`. This kernel returns `0.0` at every `k >= 1` instead of
/// `NaN` — matching `statsmodels`' practical behaviour on constant input and
/// keeping the downstream Q-statistic finite. `acf[0]` stays `1.0` by
/// construction (the lag-0 autocorrelation of any series is `1` modulo the
/// degenerate-variance edge).
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n is the returns sample size — bar counts fit trivially in f64's 52-bit mantissa for any realistic OHLCV series (Phase 1 cap << 2^52)"
)]
pub(crate) fn biased_acf(x: &[f64], max_lag: usize) -> Vec<f64> {
    let n = x.len();
    let n_f = n as f64;
    let mean = x.iter().copied().sum::<f64>() / n_f;
    let cent: Vec<f64> = x.iter().map(|v| v - mean).collect();
    let denom: f64 = cent.iter().map(|v| v * v).sum();

    let mut out = Vec::with_capacity(max_lag + 1);
    out.push(1.0);
    for k in 1..=max_lag {
        if denom == 0.0 {
            out.push(0.0);
            continue;
        }
        let num: f64 = (0..n.saturating_sub(k))
            .map(|i| cent[i] * cent[i + k])
            .sum();
        out.push(num / denom);
    }
    out
}

/// Ljung-Box Q-statistic + chi-squared p-values for lags `1..=max_lag`.
///
/// `returns_n` is the sample size of the returns series (length of the input
/// to [`biased_acf`]). `acf` is the biased-ACF output (length `max_lag + 1`;
/// `acf[0]` is unused).
///
/// Returns a pair `(q_stats, p_values)` each of length `max_lag` (one element
/// per lag in `1..=max_lag`). `q_stats[k-1]` is the cumulative Q-statistic at
/// lag `k`; `p_values[k-1]` is the chi-squared(k) tail probability
/// (`1 - chi2.cdf(q[k-1], df=k)`).
///
/// The summation order is sequential and cumsum-style (RESEARCH line 687) —
/// this is what makes goldens match statsmodels' `np.cumsum`.
///
/// # Panics
/// Panics via `debug_assert` when `max_lag < 1` or when `acf.len() <= max_lag`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "k is bounded by max_lag (typical max 10..50 per D3-03); returns_n is the bar count which fits trivially in f64's 52-bit mantissa for realistic series"
)]
#[allow(
    clippy::similar_names,
    reason = "`acf` (autocorrelation function) and `acc` (cumulative accumulator) are short, well-known statistics-domain names; renaming either would be noise"
)]
pub(crate) fn ljung_box_q_and_p(
    returns_n: usize,
    acf: &[f64],
    max_lag: usize,
) -> (Vec<f64>, Vec<f64>) {
    debug_assert!(max_lag >= 1, "ljung_box_q_and_p: max_lag must be >= 1");
    debug_assert!(
        acf.len() > max_lag,
        "ljung_box_q_and_p: acf.len() must be > max_lag"
    );

    let n = returns_n as f64;
    let mut q = Vec::with_capacity(max_lag);
    let mut p = Vec::with_capacity(max_lag);
    let mut acc = 0.0_f64;
    // Sequential cumsum over `k in 1..=max_lag` — the summation order pins
    // byte-equality with statsmodels' `np.cumsum` (RESEARCH line 687). Direct
    // indexed iteration is the natural shape; the `needless_range_loop` lint
    // is suppressed because `acf[k]` is the index-aware access we want.
    #[allow(clippy::needless_range_loop)]
    for k in 1..=max_lag {
        let k_f = k as f64;
        let denom = n - k_f;
        acc += acf[k] * acf[k] / denom;
        let qk = n * (n + 2.0) * acc;
        q.push(qk);
        // ChiSquared::new(k) requires k > 0; our debug_assert(max_lag >= 1)
        // makes k >= 1 inside this loop, so the construction always succeeds.
        let chi = ChiSquared::new(k_f).expect("k >= 1 yields a valid ChiSquared");
        p.push(1.0 - chi.cdf(qk));
    }
    (q, p)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::cast_lossless,
    clippy::needless_range_loop,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_closure
)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    // -----------------------------------------------------------------------
    // log_returns — Phase 3 tests moved to
    // `crate::scan::primitives::returns::tests` (Plan 04-02 / D4-06 lift). The
    // byte-identical-move regression gate lives in the primitive's test block
    // (`log_returns_matches_ljung_box_kernel`) and the Phase 3 statsmodels
    // integration test (`scan_ljung_box.rs`) continues to pass byte-identically.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // biased_acf
    // -----------------------------------------------------------------------

    #[test]
    fn biased_acf_lag0_is_one() {
        let x = [1.0, 2.0, 3.0, 2.5, 2.0];
        let acf = biased_acf(&x, 2);
        assert!(approx_eq(acf[0], 1.0, TOL), "acf[0]={}", acf[0]);
    }

    #[test]
    fn biased_acf_constant_series() {
        // denom == 0 for a constant series; kernel returns 0 at lag>=1
        // (statsmodels' practical handling of zero-variance input).
        let acf = biased_acf(&[5.0; 10], 3);
        assert_eq!(acf.len(), 4);
        assert!(approx_eq(acf[0], 1.0, TOL));
        for k in 1..=3 {
            assert!(
                approx_eq(acf[k], 0.0, TOL),
                "acf[{k}]={} should be 0.0 for constant series",
                acf[k]
            );
        }
    }

    #[test]
    fn biased_acf_known_input() {
        // Hand-computed reference for x = [1.0, 1.5, 2.0, 1.8, 1.6, 2.2, 2.8, 2.5].
        // Precomputed via reference Python with the same algorithm; pinned here
        // within 1e-12. The Plan 06 golden test pins byte-exact statsmodels
        // equality at the envelope level.
        let x = [1.0_f64, 1.5, 2.0, 1.8, 1.6, 2.2, 2.8, 2.5];
        let acf = biased_acf(&x, 3);
        assert_eq!(acf.len(), 4);
        let expected = [
            1.0_f64,
            0.448_340_471_092_077_1,
            -0.086_188_436_830_835_02,
            -0.009_368_308_351_177_7,
        ];
        for (i, (got, want)) in acf.iter().zip(expected.iter()).enumerate() {
            assert!(
                approx_eq(*got, *want, TOL),
                "acf[{i}]={} vs expected {} (diff {})",
                got,
                want,
                (got - want).abs()
            );
        }
    }

    #[test]
    fn biased_acf_length_is_max_lag_plus_one() {
        let x = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(biased_acf(&x, 0).len(), 1);
        assert_eq!(biased_acf(&x, 2).len(), 3);
        assert_eq!(biased_acf(&x, 4).len(), 5);
    }

    // -----------------------------------------------------------------------
    // ljung_box_q_and_p
    // -----------------------------------------------------------------------

    #[test]
    fn ljung_box_q_monotone() {
        // Use a positive-autocorrelation seed; Q-stats must be non-decreasing
        // (each contribution is acf[k]^2/(n-k) >= 0).
        let x = [1.0_f64, 1.5, 2.0, 1.8, 1.6, 2.2, 2.8, 2.5];
        let acf = biased_acf(&x, 3);
        let (q, _p) = ljung_box_q_and_p(x.len(), &acf, 3);
        assert_eq!(q.len(), 3);
        assert!(q[0] <= q[1], "Q monotone: q[0]={} q[1]={}", q[0], q[1]);
        assert!(q[1] <= q[2], "Q monotone: q[1]={} q[2]={}", q[1], q[2]);
    }

    #[test]
    fn ljung_box_q_and_p_known_input() {
        // Same fixture as biased_acf_known_input; precomputed Q-stats + p-values.
        let x = [1.0_f64, 1.5, 2.0, 1.8, 1.6, 2.2, 2.8, 2.5];
        let acf = biased_acf(&x, 3);
        let (q, p) = ljung_box_q_and_p(x.len(), &acf, 3);
        let expected_q = [
            2.297_247_748_789_321_3_f64,
            2.396_293_704_033_892_5,
            2.397_697_947_255_696_5,
        ];
        // p-values come from chi2 CDF; tolerate slightly larger numeric error
        // because chi2.cdf goes through a transcendental.
        let expected_p = [
            0.129_603_468_052_337_58_f64,
            0.301_752_886_852_302_5,
            0.494_063_292_970_136_3,
        ];
        for i in 0..3 {
            assert!(
                approx_eq(q[i], expected_q[i], TOL),
                "q[{i}]={} expected {}",
                q[i],
                expected_q[i]
            );
            assert!(
                approx_eq(p[i], expected_p[i], 1e-10),
                "p[{i}]={} expected {}",
                p[i],
                expected_p[i]
            );
        }
    }

    #[test]
    fn ljung_box_p_in_unit_interval() {
        // For any Q >= 0 and df >= 1, p must land in [0, 1].
        let x = [1.0_f64, 1.5, 2.0, 1.8, 1.6, 2.2, 2.8, 2.5, 3.0, 2.7];
        let acf = biased_acf(&x, 5);
        let (_q, p) = ljung_box_q_and_p(x.len(), &acf, 5);
        for (i, pv) in p.iter().enumerate() {
            assert!((0.0..=1.0).contains(pv), "p[{i}]={pv} must be in [0, 1]");
        }
    }

    #[test]
    #[should_panic(expected = "max_lag must be >= 1")]
    fn ljung_box_invalid_lag_panics() {
        // debug_assert fires under cfg(test).
        let acf = [1.0, 0.5];
        let _ = ljung_box_q_and_p(10, &acf, 0);
    }
}

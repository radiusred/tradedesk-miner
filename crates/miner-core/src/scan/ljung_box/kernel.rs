//! Pure Ljung-Box kernel — `log_returns`, `biased_acf`, `ljung_box_q_and_p`.
//!
//! Pattern analog: `aggregator.rs::{align_down, validate_range_alignment, emit_bucket}`
//! (lines 235-253, 258, 375) — private `#[inline]` pure functions on primitive
//! types with a sibling `#[cfg(test)] mod tests` block. No IO, no `serde_json`,
//! no `Reader` calls — pure kernels callable by [`super::LjungBoxScan::run`].
//!
//! Wave 0 scaffold: signature only. Plan 04 fills the bodies verbatim from
//! 03-RESEARCH §"Code Examples" lines 644-685.

#![allow(dead_code, unused_variables)]

/// Compute log returns from a `close` price series: `returns[t] = ln(close[t] / close[t-1])`
/// for `t = 1..n`. Returns a `Vec<f64>` of length `n - 1` (or empty when `n < 2`).
///
/// Pure function — no allocations beyond the output `Vec`. Plan 04 fills the body.
#[inline]
pub(super) fn log_returns(close: &[f64]) -> Vec<f64> {
    unimplemented!(
        "Plan 04 (03-04-PLAN) implements log_returns(close) per 03-RESEARCH lines 644-650"
    )
}

/// Biased sample autocorrelation up to `max_lag` lags.
///
/// Returns a `Vec<f64>` of length `max_lag + 1` where `acf[0] == 1.0` and `acf[k]`
/// is the biased ACF estimator at lag `k`. The "biased" estimator divides by `n`
/// (not `n - k`) so it matches `statsmodels.tsa.stattools.acf(..., adjusted=False)`.
/// See D3-05 — statsmodels golden comparison.
///
/// Pure function. Plan 04 fills the body.
#[inline]
pub(super) fn biased_acf(x: &[f64], max_lag: usize) -> Vec<f64> {
    unimplemented!(
        "Plan 04 (03-04-PLAN) implements biased_acf per 03-RESEARCH lines 652-672"
    )
}

/// Ljung-Box Q-statistic + chi-squared p-values for lags `1..=max_lag`.
///
/// `returns_n` is the sample size of the returns series (length of the input
/// to `biased_acf`). `acf` is the biased-ACF output (length `max_lag + 1`,
/// `acf[0]` is unused).
///
/// Returns a pair `(q_stats, p_values)` each of length `max_lag` (one element
/// per lag in `1..=max_lag`). `q_stats[k-1]` is the cumulative Q-statistic at
/// lag `k`; `p_values[k-1]` is the chi-squared(k) tail probability.
///
/// Pure function except for the `statrs::distribution::ChiSquared::new(k)` +
/// `.sf(q)` calls (no IO). Plan 04 fills the body.
#[inline]
pub(super) fn ljung_box_q_and_p(
    returns_n: usize,
    acf: &[f64],
    max_lag: usize,
) -> (Vec<f64>, Vec<f64>) {
    unimplemented!(
        "Plan 04 (03-04-PLAN) implements ljung_box_q_and_p per 03-RESEARCH lines 674-685; \
         uses statrs::distribution::ChiSquared for the p-value"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    // Plan 04 fills:
    // - `log_returns_basic` — handcrafted 4-element input, expected outputs.
    // - `acf_matches_statsmodels_at_k1` — tiny vector, biased-ACF byte equality.
    // - `ljung_box_matches_statsmodels_q_stat` — same fixture, Q-stat byte equality.
    //
    // Wave 0 ships an empty module so the `#[cfg(test)] mod tests` discipline
    // (clippy.toml + RESEARCH §"Pure-kernel pattern") holds the scaffold.
}

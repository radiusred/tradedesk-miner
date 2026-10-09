//! Pure returns-profile kernel — `dispatch_variant`, `mean_and_std`.
//!
//! Pattern analog: `crates/miner-core/src/scan/ljung_box/kernel.rs` — private
//! `#[inline] pub(super)` pure functions on primitive types with a sibling
//! `#[cfg(test)] mod tests` block. No IO, no `serde_json`, no `Reader` calls.
//!
//! ## Implementation source
//!
//! - `dispatch_variant` is a thin enum-switch over
//!   `crate::scan::primitives::returns::{log_returns, simple_returns,
//!   intraday_returns, overnight_returns}` per Plan 04-03 RESEARCH §1.4 (one
//!   callable scan, four primitive variants).
//! - `mean_and_std` is a two-pass mean + sample standard deviation (ddof=1).
//!   Sequential summation order pins cross-platform determinism (Pitfall 4).
//!   Constant-input branch returns `(mean, 0.0)` instead of `NaN`.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use chrono::{DateTime, Utc};

use crate::scan::primitives::returns::{
    intraday_returns, log_returns, overnight_returns, simple_returns,
};

/// Variant dispatch enum for `stats.returns.profile@1`. Wire-form maps to
/// `params.variant` (the four lowercase strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReturnsVariant {
    Log,
    Simple,
    Intraday,
    Overnight,
}

impl ReturnsVariant {
    /// `snake_case` wire-form label echoed into `effect.extra.variant_label`.
    #[inline]
    pub(super) fn as_str(self) -> &'static str {
        match self {
            ReturnsVariant::Log => "log",
            ReturnsVariant::Simple => "simple",
            ReturnsVariant::Intraday => "intraday",
            ReturnsVariant::Overnight => "overnight",
        }
    }
}

/// Dispatch on `variant` and call the corresponding primitive returns kernel.
///
/// `ts` is unused for `Log` / `Simple` (they read only `closes`) but is
/// required for `Intraday` / `Overnight` which partition on UTC date.
///
/// # Panics
/// Panics via `debug_assert` when `closes.len() != ts.len()` (the
/// intraday/overnight primitives assert this themselves; the assertion is
/// hoisted to the dispatch site so log/simple variants get the same gate).
#[inline]
pub(super) fn dispatch_variant(
    closes: &[f64],
    ts: &[DateTime<Utc>],
    variant: ReturnsVariant,
) -> Vec<f64> {
    debug_assert_eq!(
        closes.len(),
        ts.len(),
        "dispatch_variant: closes.len() must equal ts.len()"
    );
    match variant {
        ReturnsVariant::Log => log_returns(closes),
        ReturnsVariant::Simple => simple_returns(closes),
        ReturnsVariant::Intraday => intraday_returns(closes, ts),
        ReturnsVariant::Overnight => overnight_returns(closes, ts),
    }
}

/// Compute the arithmetic mean and sample standard deviation (ddof=1) of a
/// returns slice via a two-pass scheme. Sequential summation order pins
/// cross-platform determinism (Pitfall 4).
///
/// Returns `(mean, std)`. The constant-input branch returns `(mean, 0.0)`
/// instead of `NaN` to keep the downstream envelope finite.
///
/// # Panics
/// Panics via `debug_assert` when `values.len() < 2` (the caller is expected
/// to have emitted `Finding::ScanError` with `InsufficientData` for N<2).
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "values.len() is a bar count; fits in f64's 52-bit mantissa for any realistic OHLCV series"
)]
pub(super) fn mean_and_std(values: &[f64]) -> (f64, f64) {
    debug_assert!(
        values.len() >= 2,
        "mean_and_std: need >= 2 samples; got {}",
        values.len()
    );
    let n = values.len();
    let n_f = n as f64;
    // Pass 1 — mean. Sequential summation order matches Welford/numpy default.
    let sum: f64 = values.iter().copied().sum();
    let mean = sum / n_f;
    // Pass 2 — sum of squared deviations. Constant-input branch returns 0.
    let mut sq_dev: f64 = 0.0;
    for v in values {
        let d = v - mean;
        sq_dev += d * d;
    }
    // Sample variance (ddof=1).
    let var = sq_dev / (n_f - 1.0);
    let std = var.sqrt();
    (mean, std)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    // -----------------------------------------------------------------------
    // dispatch_variant
    // -----------------------------------------------------------------------

    #[test]
    fn dispatch_variant_log_matches_primitive() {
        let closes = [1.0_f64, 1.1, 1.21];
        let ts = (0..3)
            .map(|i| {
                Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
                    + chrono::Duration::minutes(15 * i)
            })
            .collect::<Vec<_>>();
        let got = dispatch_variant(&closes, &ts, ReturnsVariant::Log);
        let want = log_returns(&closes);
        assert_eq!(got.len(), want.len());
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(approx_eq(*g, *w, TOL), "log[{i}] mismatch: {g} vs {w}");
        }
    }

    #[test]
    fn dispatch_variant_simple_matches_primitive() {
        let closes = [1.0_f64, 1.1, 1.21];
        let ts = (0..3)
            .map(|i| {
                Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
                    + chrono::Duration::minutes(15 * i)
            })
            .collect::<Vec<_>>();
        let got = dispatch_variant(&closes, &ts, ReturnsVariant::Simple);
        let want = simple_returns(&closes);
        assert_eq!(got.len(), want.len());
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(approx_eq(*g, *w, TOL), "simple[{i}] mismatch: {g} vs {w}");
        }
    }

    #[test]
    fn dispatch_variant_intraday_partitions_on_date() {
        // Four bars: two on 2024-01-01, two on 2024-01-02. Three transitions:
        // intraday, overnight, intraday. Intraday returns has 2 elements.
        let day0 = Utc.with_ymd_and_hms(2024, 1, 1, 23, 0, 0).unwrap();
        let ts = vec![
            day0,
            day0 + chrono::Duration::minutes(15),
            day0 + chrono::Duration::hours(1),
            day0 + chrono::Duration::hours(2),
        ];
        let closes = vec![1.0_f64, 1.1, 1.2, 1.3];
        let intra = dispatch_variant(&closes, &ts, ReturnsVariant::Intraday);
        assert_eq!(intra.len(), 2);
    }

    #[test]
    fn dispatch_variant_overnight_partitions_on_date() {
        let day0 = Utc.with_ymd_and_hms(2024, 1, 1, 23, 0, 0).unwrap();
        let ts = vec![
            day0,
            day0 + chrono::Duration::minutes(15),
            day0 + chrono::Duration::hours(1),
            day0 + chrono::Duration::hours(2),
        ];
        let closes = vec![1.0_f64, 1.1, 1.2, 1.3];
        let over = dispatch_variant(&closes, &ts, ReturnsVariant::Overnight);
        assert_eq!(over.len(), 1);
    }

    // -----------------------------------------------------------------------
    // mean_and_std
    // -----------------------------------------------------------------------

    #[test]
    fn mean_and_std_known_input() {
        // [1, 2, 3, 4, 5]: mean = 3.0, sample-std (ddof=1) = sqrt(10/4) = sqrt(2.5).
        let values = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        let (mean, std) = mean_and_std(&values);
        assert!(approx_eq(mean, 3.0, TOL), "mean={mean}");
        assert!(approx_eq(std, 2.5_f64.sqrt(), TOL), "std={std}");
    }

    #[test]
    fn mean_and_std_constant_input_zero_std() {
        let values = [3.7_f64; 5];
        let (mean, std) = mean_and_std(&values);
        assert!(approx_eq(mean, 3.7, TOL));
        assert!(approx_eq(std, 0.0, TOL), "constant input -> std == 0.0");
    }

    #[test]
    fn mean_and_std_two_samples() {
        let values = [1.0_f64, 3.0];
        let (mean, std) = mean_and_std(&values);
        assert!(approx_eq(mean, 2.0, TOL));
        // ddof=1: var = ((1-2)^2 + (3-2)^2) / (2-1) = 2; std = sqrt(2)
        assert!(approx_eq(std, 2.0_f64.sqrt(), TOL));
    }

    #[test]
    #[should_panic(expected = "mean_and_std: need >= 2 samples")]
    fn mean_and_std_panics_below_two_samples() {
        let _ = mean_and_std(&[42.0_f64]);
    }
}

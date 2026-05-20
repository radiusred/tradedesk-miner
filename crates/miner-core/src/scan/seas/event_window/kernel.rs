//! Pure `event_window` kernel — caller-supplied event-timestamp aligned
//! pre/post window aggregation.
//!
//! Pattern analog: `seas/hour_of_day/kernel.rs` (kernel-only file with
//! pure functions + sibling tests block). No IO, no `serde_json`, no
//! `Reader` calls.
//!
//! ## Semantics
//!
//! For each event timestamp:
//!
//! 1. Resolve its bar index by `timestamps_ms.partition_point(|&t| t <= event)`
//!    minus one — i.e. the most-recent bar whose timestamp is at-or-before
//!    the event. An event before the first bar timestamp has no bar index;
//!    an event after the last bar timestamp uses the last bar index.
//! 2. If the resolved bar is unusable (index outside `[pre_bars, n - post_bars]`)
//!    the event is SKIPPED.
//! 3. Otherwise: pre window = `returns[idx - pre_bars .. idx]`,
//!    post window = `returns[idx .. idx + post_bars]`. The event bar
//!    itself is included as the FIRST bar of the post window (consistent
//!    with "event triggers — observe the immediate post-event response").
//!
//! ## Note on event-bar exclusion/inclusion
//!
//! The Plan picks: the event bar IS the first bar of the post window. This
//! lets the post window capture the immediate-post-event return. The pre
//! window stops one bar BEFORE the event bar.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// Output of [`event_window_stats`]. Parallel vectors of length
/// `event_count` (events whose pre/post windows fit inside the bar range);
/// `event_count` itself is exposed for convenience.
#[derive(Debug, Clone)]
pub(super) struct EventWindowResult {
    pub pre_means: Vec<f64>,
    pub post_means: Vec<f64>,
    pub pre_stds: Vec<f64>,
    pub post_stds: Vec<f64>,
    pub event_count: usize,
}

/// Compute per-event pre/post window aggregate statistics.
///
/// `returns[i]` is the log-return between bar `i` and bar `i+1`; `timestamps_ms[i]`
/// is the timestamp of bar `i+1` in milliseconds since UNIX epoch. The
/// `event_timestamps_ms` are caller-supplied; events outside the bar range
/// or with insufficient pre/post bars are silently skipped.
///
/// Returns parallel vectors of length `event_count` (the number of events
/// that passed the boundary check).
///
/// # Panics
/// Panics via `debug_assert` when `returns.len() != timestamps_ms.len()`,
/// or when `pre_bars == 0` or `post_bars == 0`.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "n is bar count; fits f64 mantissa for realistic OHLCV slices"
)]
pub(super) fn event_window_stats(
    returns: &[f64],
    timestamps_ms: &[i64],
    event_timestamps_ms: &[i64],
    pre_bars: usize,
    post_bars: usize,
) -> EventWindowResult {
    debug_assert_eq!(
        returns.len(),
        timestamps_ms.len(),
        "event_window_stats: returns and timestamps_ms length mismatch"
    );
    debug_assert!(pre_bars >= 1, "event_window_stats: pre_bars must be >= 1");
    debug_assert!(post_bars >= 1, "event_window_stats: post_bars must be >= 1");

    let n = returns.len();
    let mut pre_means = Vec::new();
    let mut post_means = Vec::new();
    let mut pre_stds = Vec::new();
    let mut post_stds = Vec::new();

    for &event_ts in event_timestamps_ms {
        // Resolve the bar index for the event. Use binary search: the bar
        // whose timestamp is >= event timestamp is the first post-event bar.
        // `partition_point(|&t| t < event_ts)` returns the count of bars
        // strictly before the event — equivalently the index of the first
        // bar at-or-after the event.
        let idx = timestamps_ms.partition_point(|&t| t < event_ts);
        // Skip if event before first bar (no pre window) OR if pre window
        // would underflow OR if post window would overflow.
        if idx < pre_bars || idx + post_bars > n {
            continue;
        }
        let pre_slice = &returns[idx - pre_bars..idx];
        let post_slice = &returns[idx..idx + post_bars];
        let (pre_mean, pre_std) = mean_and_std(pre_slice);
        let (post_mean, post_std) = mean_and_std(post_slice);
        pre_means.push(pre_mean);
        post_means.push(post_mean);
        pre_stds.push(pre_std);
        post_stds.push(post_std);
    }

    let event_count = pre_means.len();
    EventWindowResult {
        pre_means,
        post_means,
        pre_stds,
        post_stds,
        event_count,
    }
}

/// Sample mean + Bessel-corrected (ddof=1) standard deviation. For `n < 2`
/// returns `(NaN, NaN)`; for `n == 2..` returns the standard estimators.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "values.len() is the window size (typ. 3..512); fits f64 mantissa"
)]
fn mean_and_std(values: &[f64]) -> (f64, f64) {
    let n = values.len();
    if n < 2 {
        if n == 1 {
            return (values[0], f64::NAN);
        }
        return (f64::NAN, f64::NAN);
    }
    let nf = n as f64;
    let mean = values.iter().copied().sum::<f64>() / nf;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (nf - 1.0);
    (mean, var.sqrt())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Hand-derived: 20-bar series, event timestamp lands at bar index 5
    /// (`timestamps_ms`[5] == `event_ts`). `pre_bars=3` -> returns[2..5];
    /// `post_bars=3` -> returns[5..8]. Verify the means manually.
    #[test]
    fn one_event_at_index_5_hand_derived() {
        // returns[0..20] = [0.0, 0.1, 0.2, ..., 1.9].
        let returns: Vec<f64> = (0..20).map(|i| f64::from(i) * 0.1).collect();
        let timestamps_ms: Vec<i64> = (0..20).map(|i| 1_000 + 100 * i64::from(i)).collect();
        // Event at exactly timestamps_ms[5].
        let event_ts = vec![timestamps_ms[5]];
        let r = event_window_stats(&returns, &timestamps_ms, &event_ts, 3, 3);
        assert_eq!(r.event_count, 1);
        // pre = returns[2..5] = [0.2, 0.3, 0.4]; mean = 0.3.
        assert!(approx_eq(r.pre_means[0], 0.3, TOL), "pre_mean={}", r.pre_means[0]);
        // post = returns[5..8] = [0.5, 0.6, 0.7]; mean = 0.6.
        assert!(
            approx_eq(r.post_means[0], 0.6, TOL),
            "post_mean={}",
            r.post_means[0]
        );
        // pre std (ddof=1) over [0.2, 0.3, 0.4]: variance = 0.01/2 = 0.01 wait
        // (0.2-0.3)^2 + (0.3-0.3)^2 + (0.4-0.3)^2 = 0.01 + 0 + 0.01 = 0.02;
        // /(n-1) = 0.02/2 = 0.01; sqrt = 0.1.
        assert!(approx_eq(r.pre_stds[0], 0.1, TOL), "pre_std={}", r.pre_stds[0]);
        assert!(approx_eq(r.post_stds[0], 0.1, TOL), "post_std={}", r.post_stds[0]);
    }

    /// Event outside the bar range -> skipped, `event_count` == 0.
    #[test]
    fn event_after_last_bar_skipped() {
        let returns: Vec<f64> = (0..20).map(|i| f64::from(i) * 0.1).collect();
        let timestamps_ms: Vec<i64> = (0..20).map(|i| 1_000 + 100 * i64::from(i)).collect();
        // Event way after the last bar -> idx = n; post window overflows; skip.
        let event_ts = vec![timestamps_ms[19] + 10_000];
        let r = event_window_stats(&returns, &timestamps_ms, &event_ts, 3, 3);
        assert_eq!(r.event_count, 0);
    }

    /// Event at index 1 with `pre_bars=3` -> idx < `pre_bars`; skipped.
    #[test]
    fn event_near_start_skipped_pre_underflow() {
        let returns: Vec<f64> = (0..20).map(|i| f64::from(i) * 0.1).collect();
        let timestamps_ms: Vec<i64> = (0..20).map(|i| 1_000 + 100 * i64::from(i)).collect();
        let event_ts = vec![timestamps_ms[1]];
        let r = event_window_stats(&returns, &timestamps_ms, &event_ts, 3, 3);
        assert_eq!(r.event_count, 0);
    }

    /// Event at index 18 with `post_bars=3` -> idx + post > n; skipped.
    #[test]
    fn event_near_end_skipped_post_overflow() {
        let returns: Vec<f64> = (0..20).map(|i| f64::from(i) * 0.1).collect();
        let timestamps_ms: Vec<i64> = (0..20).map(|i| 1_000 + 100 * i64::from(i)).collect();
        // idx 18 with post=3 -> needs indices 18, 19, 20; 20 is OOB.
        let event_ts = vec![timestamps_ms[18]];
        let r = event_window_stats(&returns, &timestamps_ms, &event_ts, 3, 3);
        assert_eq!(r.event_count, 0);
    }

    /// Three events; all three aggregate.
    #[test]
    fn three_events_aggregated() {
        let returns: Vec<f64> = (0..30).map(|i| f64::from(i) * 0.01).collect();
        let timestamps_ms: Vec<i64> = (0..30).map(|i| 1_000 + 100 * i64::from(i)).collect();
        let event_ts = vec![timestamps_ms[5], timestamps_ms[15], timestamps_ms[25]];
        let r = event_window_stats(&returns, &timestamps_ms, &event_ts, 3, 3);
        assert_eq!(r.event_count, 3);
        assert_eq!(r.pre_means.len(), 3);
        assert_eq!(r.post_means.len(), 3);
        assert_eq!(r.pre_stds.len(), 3);
        assert_eq!(r.post_stds.len(), 3);
    }

    /// Event timestamp BETWEEN bar timestamps — resolves to the next bar.
    /// For `event_ts` = 1550 (between bar 5 @1500 and bar 6 @1600), idx = 6.
    /// With pre=3 post=3, pre = returns[3..6], post = returns[6..9].
    #[test]
    fn event_between_bars_resolves_to_next_bar() {
        let returns: Vec<f64> = (0..15).map(|i| f64::from(i) * 0.1).collect();
        let timestamps_ms: Vec<i64> = (0..15).map(|i| 1_000 + 100 * i64::from(i)).collect();
        // Between timestamps_ms[5] (1500) and timestamps_ms[6] (1600).
        let event_ts = vec![1_550_i64];
        let r = event_window_stats(&returns, &timestamps_ms, &event_ts, 3, 3);
        assert_eq!(r.event_count, 1);
        // pre = returns[3..6] = [0.3, 0.4, 0.5]; mean = 0.4.
        assert!(approx_eq(r.pre_means[0], 0.4, TOL), "pre_mean={}", r.pre_means[0]);
        // post = returns[6..9] = [0.6, 0.7, 0.8]; mean = 0.7.
        assert!(
            approx_eq(r.post_means[0], 0.7, TOL),
            "post_mean={}",
            r.post_means[0]
        );
    }

    /// Empty event list -> all-zero output (handled by the caller as
    /// "no events met the boundary check"; the kernel allows it).
    #[test]
    fn empty_event_list_returns_empty_vectors() {
        let returns: Vec<f64> = (0..20).map(|i| f64::from(i) * 0.1).collect();
        let timestamps_ms: Vec<i64> = (0..20).map(|i| 1_000 + 100 * i64::from(i)).collect();
        let r = event_window_stats(&returns, &timestamps_ms, &[], 3, 3);
        assert_eq!(r.event_count, 0);
        assert!(r.pre_means.is_empty());
    }

    #[test]
    #[should_panic(expected = "pre_bars must be >= 1")]
    fn pre_bars_zero_panics() {
        let _ = event_window_stats(&[0.0, 0.1], &[1, 2], &[], 0, 1);
    }

    #[test]
    #[should_panic(expected = "post_bars must be >= 1")]
    fn post_bars_zero_panics() {
        let _ = event_window_stats(&[0.0, 0.1], &[1, 2], &[], 1, 0);
    }
}

//! Pure drawdown-profile kernels for ANOM-11 —
//! `cumulative_log_equity`, `compute_drawdown_profile`.
//!
//! Pattern analog: `crates/miner-core/src/scan/ljung_box/kernel.rs` and the
//! sibling `summary/kernel.rs` / `outliers/kernel.rs` — private `#[inline]
//! pub(super)` pure functions on `&[f64]` with a sibling `#[cfg(test)] mod
//! tests` block.
//!
//! ## Algorithm
//!
//! Drawdown is a cumulative-equity concept. Given a series of log returns
//! `r_0..r_{n-1}` we build the equity curve `E_t = sum_{i<=t} r_i` (a single
//! cumulative sum) — equivalent to `ln(close_t / close_0)` on the underlying
//! price series. We then sweep through the equity curve maintaining a
//! running peak and recording each drawdown episode:
//!
//! - When `E_t < running_peak`, we are inside a drawdown; track the trough.
//! - When `E_t >= running_peak` (water-mark recovered), close the current
//!   episode and record its peak index, trough index, duration (ms from
//!   peak ts to trough ts), and time-to-recover (ms from trough ts to
//!   recovery ts).
//! - The largest negative `(E_t - running_peak)` ever seen is the
//!   `max_drawdown` headline scalar.
//!
//! Reference: there is no statsmodels/scipy primitive for drawdown — this
//! is the kernel hand-derived from the "running peak" / "underwater curve"
//! formulation common in quantitative finance (e.g., Bacon 2008 chap 7).
//!
//! Drawdown values are always `<= 0.0` by construction (an underwater
//! quantity); the scan body asserts that `effect.value <= 0.0`.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use chrono::{DateTime, Utc};

/// Output of [`compute_drawdown_profile`].
#[derive(Debug, Clone)]
pub(super) struct DrawdownProfile {
    /// The most-negative drawdown magnitude ever reached. Always `<= 0.0`.
    pub max_dd: f64,
    /// Cumulative log-equity curve. Length == `returns.len()`.
    pub equity_curve: Vec<f64>,
    /// Peak indices per closed drawdown episode (one per episode).
    pub peaks: Vec<usize>,
    /// Trough indices per closed drawdown episode (one per episode; same
    /// length as `peaks`).
    pub troughs: Vec<usize>,
    /// Duration in milliseconds from each episode's peak timestamp to its
    /// trough timestamp.
    pub durations_ms: Vec<i64>,
    /// Time-to-recover in milliseconds from each episode's trough timestamp
    /// to the bar where the running peak was first re-attained.
    pub time_to_recover_ms: Vec<i64>,
    /// Percentiles of the recorded drawdown magnitudes (absolute values) at
    /// p50, p95, p99. Empty-episode list yields [0.0, 0.0, 0.0].
    pub dd_dist_percentiles: [f64; 3],
}

/// Cumulative sum of `log_returns`: `equity_curve[t] = sum_{i<=t} log_returns[i]`.
/// Length == `log_returns.len()`. Equity at t=0 is `log_returns[0]` (we use
/// the log-return path; the absolute price level is irrelevant — drawdowns
/// are translation-invariant in log-space).
///
/// # Panics
/// Panics via `debug_assert` when input is empty.
#[inline]
pub(super) fn cumulative_log_equity(log_returns: &[f64]) -> Vec<f64> {
    debug_assert!(!log_returns.is_empty(), "cumulative_log_equity: empty slice");
    let mut out = Vec::with_capacity(log_returns.len());
    let mut acc = 0.0_f64;
    for r in log_returns {
        acc += r;
        out.push(acc);
    }
    out
}

/// Compute a drawdown profile over an equity curve + parallel timestamps.
///
/// Single-pass O(n) algorithm:
/// - Track `running_peak` (max equity so far) and the index where the
///   running peak was set.
/// - When `equity[t] < running_peak` (underwater): we are inside a
///   drawdown; track the trough's value + index.
/// - When `equity[t] >= running_peak` (water-mark recovered) AND a
///   drawdown episode was opened: close the episode and record peak index,
///   trough index, duration (peak->trough ms), and time-to-recover
///   (trough->now ms). Reset the episode state.
/// - Finally compute p50/p95/p99 of the recorded drawdown magnitudes
///   (absolute values, monotone-positive). If no episode is recorded,
///   percentiles are all 0.0.
///
/// `max_dd` is the most-negative `(equity[t] - running_peak)` ever observed
/// (always `<= 0.0`). Note the value is signed (negative magnitude); the
/// percentile vector uses absolute values.
///
/// # Panics
/// Panics via `debug_assert` when `equity_curve.len() != ts.len()` or when
/// the input is empty.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "epoch-ms fits in f64; bar counts fit; index arithmetic is exact for realistic OHLCV"
)]
pub(super) fn compute_drawdown_profile(
    equity_curve: &[f64],
    ts: &[DateTime<Utc>],
) -> DrawdownProfile {
    debug_assert_eq!(
        equity_curve.len(),
        ts.len(),
        "compute_drawdown_profile: equity_curve.len() must equal ts.len()"
    );
    debug_assert!(
        !equity_curve.is_empty(),
        "compute_drawdown_profile: empty input"
    );

    let n = equity_curve.len();
    let mut peaks: Vec<usize> = Vec::new();
    let mut troughs: Vec<usize> = Vec::new();
    let mut durations_ms: Vec<i64> = Vec::new();
    let mut time_to_recover_ms: Vec<i64> = Vec::new();
    let mut dd_magnitudes: Vec<f64> = Vec::new(); // positive (absolute) magnitudes.

    let mut running_peak = equity_curve[0];
    let mut running_peak_idx: usize = 0;
    let mut max_dd: f64 = 0.0;
    // Episode state: in_drawdown flips true at the first bar where equity
    // drops below the running peak; clears when the running peak is recovered.
    let mut in_drawdown = false;
    let mut trough_value = 0.0_f64;
    let mut trough_idx: usize = 0;
    let mut episode_peak_idx: usize = 0;

    for t in 0..n {
        let e = equity_curve[t];
        if e >= running_peak {
            // New peak (or equal). If we were in a drawdown, close the
            // episode here.
            if in_drawdown {
                peaks.push(episode_peak_idx);
                troughs.push(trough_idx);
                let dur = ts[trough_idx]
                    .timestamp_millis()
                    - ts[episode_peak_idx].timestamp_millis();
                durations_ms.push(dur);
                let rec = ts[t].timestamp_millis() - ts[trough_idx].timestamp_millis();
                time_to_recover_ms.push(rec);
                dd_magnitudes.push((running_peak - trough_value).abs());
                in_drawdown = false;
            }
            running_peak = e;
            running_peak_idx = t;
        } else {
            // Underwater.
            let dd = e - running_peak; // negative.
            if dd < max_dd {
                max_dd = dd;
            }
            if !in_drawdown {
                // First bar of a new episode.
                in_drawdown = true;
                episode_peak_idx = running_peak_idx;
                trough_value = e;
                trough_idx = t;
            } else if e < trough_value {
                trough_value = e;
                trough_idx = t;
            }
        }
    }

    // Percentiles of recorded drawdown magnitudes. If empty (no episode
    // closed — series monotone non-decreasing OR ends underwater without
    // recovery), report zeros.
    let percentiles = if dd_magnitudes.is_empty() {
        [0.0, 0.0, 0.0]
    } else {
        let mut sorted = dd_magnitudes.clone();
        sorted.sort_by(f64::total_cmp);
        [
            quantile_linear(&sorted, 0.50),
            quantile_linear(&sorted, 0.95),
            quantile_linear(&sorted, 0.99),
        ]
    };

    DrawdownProfile {
        max_dd,
        equity_curve: equity_curve.to_vec(),
        peaks,
        troughs,
        durations_ms,
        time_to_recover_ms,
        dd_dist_percentiles: percentiles,
    }
}

/// Linear-interpolation quantile on a pre-sorted slice.
#[inline]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n is bounded by the input length; quantile index arithmetic is exact for realistic sizes"
)]
fn quantile_linear(sorted: &[f64], q: f64) -> f64 {
    debug_assert!(!sorted.is_empty(), "quantile_linear: empty slice");
    debug_assert!(
        (0.0..=1.0).contains(&q),
        "quantile_linear: q must be in [0, 1]"
    );
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let h = q * (n as f64 - 1.0);
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(n - 1);
    let frac = h - h.floor();
    sorted[lo] * (1.0 - frac) + sorted[hi] * frac
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    fn equispaced_ts(n: usize) -> Vec<DateTime<Utc>> {
        let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("fits in i64");
                t0 + Duration::minutes(15 * i_i64)
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // cumulative_log_equity
    // -----------------------------------------------------------------------

    #[test]
    fn cumulative_log_equity_known_input() {
        // [0.1, 0.2, 0.3] -> [0.1, 0.3, 0.6]
        let e = cumulative_log_equity(&[0.1_f64, 0.2, 0.3]);
        assert_eq!(e.len(), 3);
        assert!(approx_eq(e[0], 0.1, TOL));
        assert!(approx_eq(e[1], 0.3, TOL));
        assert!(approx_eq(e[2], 0.6, TOL));
    }

    #[test]
    fn cumulative_log_equity_zero_returns_is_zero_curve() {
        let e = cumulative_log_equity(&[0.0_f64; 5]);
        for v in e {
            assert!(approx_eq(v, 0.0, TOL));
        }
    }

    #[test]
    fn cumulative_log_equity_length_invariant() {
        let n = 7;
        let e = cumulative_log_equity(&vec![0.01_f64; n]);
        assert_eq!(e.len(), n);
    }

    // -----------------------------------------------------------------------
    // compute_drawdown_profile — hand-derived V-shape
    // -----------------------------------------------------------------------

    /// Hand-derived V-shape: closes [10, 5, 10] -> log returns
    /// [ln(0.5), ln(2)] = [-0.6931..., +0.6931...]. Equity curve:
    /// [-0.6931..., 0.0]. `running_peak` starts at -0.6931... at t=0, then
    /// at t=1 equity = 0.0 >= peak, so the peak updates AND the previous
    /// drawdown closes — except the drawdown opened at t=0 implicitly
    /// (equity dropped below the `running_peak` at t=0). Since we initialize
    /// `running_peak` = `equity_curve`[0] at t=0, the very first bar can never
    /// be underwater. So the V-shape requires AT LEAST 3 points in equity
    /// space: [E0, E1, E2] with E0 > E1, E2 >= E0.
    ///
    /// Simplest unambiguous V test: `equity_curve` = [0.0, -0.693147, 0.0]
    /// (we construct it directly, bypassing log-returns).
    #[test]
    fn drawdown_profile_v_shape() {
        let equity = vec![0.0_f64, -std::f64::consts::LN_2, 0.0];
        let ts = equispaced_ts(3);
        let profile = compute_drawdown_profile(&equity, &ts);
        // Max drawdown is the dip at t=1.
        assert!(approx_eq(profile.max_dd, -std::f64::consts::LN_2, TOL));
        // One closed episode: peak at 0, trough at 1, recovery at 2.
        assert_eq!(profile.peaks, vec![0]);
        assert_eq!(profile.troughs, vec![1]);
        // Duration = ts[1] - ts[0] = 15 minutes = 900_000 ms.
        assert_eq!(profile.durations_ms, vec![900_000]);
        // Recovery = ts[2] - ts[1] = 15 minutes = 900_000 ms.
        assert_eq!(profile.time_to_recover_ms, vec![900_000]);
        // Percentiles of single-element [ln(2)]: all three == that value.
        for p in profile.dd_dist_percentiles {
            assert!(approx_eq(p, std::f64::consts::LN_2, TOL));
        }
    }

    /// Monotone-increasing equity: no drawdown.
    #[test]
    fn drawdown_profile_monotone_increasing_has_no_dd() {
        let equity = vec![0.0_f64, 0.1, 0.2, 0.3, 0.4];
        let ts = equispaced_ts(5);
        let profile = compute_drawdown_profile(&equity, &ts);
        assert!(approx_eq(profile.max_dd, 0.0, TOL));
        assert!(profile.peaks.is_empty());
        assert!(profile.troughs.is_empty());
        assert!(profile.durations_ms.is_empty());
        assert!(profile.time_to_recover_ms.is_empty());
        for p in profile.dd_dist_percentiles {
            assert!(approx_eq(p, 0.0, TOL));
        }
    }

    /// Compound V-shape: equity = [10, 8, 5, 7, 10, 9, 11].
    /// - Peak at 0 (10). Underwater at t=1 (8), t=2 (5, max trough), t=3 (7).
    /// - At t=4 (10) we equal/exceed the running peak — close episode 1:
    ///   peak=0, trough=2, dur = ts[2]-ts[0], recovery = ts[4]-ts[2].
    /// - At t=5 (9) we go underwater again from new peak (10 at t=4).
    /// - At t=6 (11) we exceed running peak — close episode 2: peak=4,
    ///   trough=5, dur = ts[5]-ts[4], recovery = ts[6]-ts[5].
    /// `max_dd` = 5 - 10 = -5.0.
    #[test]
    fn drawdown_profile_compound_v() {
        let equity = vec![10.0_f64, 8.0, 5.0, 7.0, 10.0, 9.0, 11.0];
        let ts = equispaced_ts(7);
        let profile = compute_drawdown_profile(&equity, &ts);
        assert!(approx_eq(profile.max_dd, -5.0, TOL));
        assert_eq!(profile.peaks, vec![0, 4]);
        assert_eq!(profile.troughs, vec![2, 5]);
        // dur1 = (2 - 0) * 15min = 1_800_000ms; dur2 = (5 - 4) * 15min = 900_000ms.
        assert_eq!(profile.durations_ms, vec![1_800_000, 900_000]);
        // rec1 = (4 - 2) * 15min = 1_800_000ms; rec2 = (6 - 5) * 15min = 900_000ms.
        assert_eq!(profile.time_to_recover_ms, vec![1_800_000, 900_000]);
        // Magnitudes: episode 1 = 10-5 = 5; episode 2 = 10-9 = 1.
        // Sorted: [1, 5]. p50 (q=0.5 -> h=0.5, frac=0.5, lo=0): 1+0.5*(5-1) = 3.
        // p95 (q=0.95 -> h=0.95, frac=0.95, lo=0): 1+0.95*(5-1) = 4.8.
        // p99 (q=0.99 -> h=0.99, frac=0.99, lo=0): 1+0.99*(5-1) = 4.96.
        assert!(approx_eq(profile.dd_dist_percentiles[0], 3.0, TOL));
        assert!(approx_eq(profile.dd_dist_percentiles[1], 4.8, TOL));
        assert!(approx_eq(profile.dd_dist_percentiles[2], 4.96, TOL));
    }

    /// Equity that ends underwater never closes the episode — no peaks
    /// recorded, but `max_dd` is still tracked.
    #[test]
    fn drawdown_profile_ends_underwater_no_recovery() {
        let equity = vec![5.0_f64, 3.0, 1.0];
        let ts = equispaced_ts(3);
        let profile = compute_drawdown_profile(&equity, &ts);
        assert!(approx_eq(profile.max_dd, -4.0, TOL));
        // No closed episode -> empty vectors.
        assert!(profile.peaks.is_empty());
        assert!(profile.troughs.is_empty());
        assert!(profile.durations_ms.is_empty());
        assert!(profile.time_to_recover_ms.is_empty());
        for p in profile.dd_dist_percentiles {
            assert!(approx_eq(p, 0.0, TOL));
        }
    }

    /// `max_dd` <= 0.0 invariant (drawdown is always negative or zero).
    #[test]
    fn drawdown_profile_max_dd_is_non_positive() {
        // Arbitrary non-monotone equity.
        let equity = vec![1.0_f64, 0.5, 0.8, 0.3, 1.5];
        let ts = equispaced_ts(5);
        let profile = compute_drawdown_profile(&equity, &ts);
        assert!(
            profile.max_dd <= 0.0,
            "max_dd must be <= 0.0; got {}",
            profile.max_dd
        );
    }
}

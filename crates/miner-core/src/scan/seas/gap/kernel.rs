//! Pure `seas.gap.overnight` kernel — session/overnight gap detection,
//! size-bucketing, and gap-fill evaluation over an OHLC bar series.
//!
//! A *gap* is a close→next-open jump that straddles a **session boundary**.
//! Boundaries are detected purely from the inter-bar open-timestamp delta so
//! the scan is resolution-parameterized: overnight / weekend / holiday breaks
//! all surface as a time delta STRICTLY GREATER than `boundary_gap_minutes`.
//! On a continuous 24x5 series (FX) consecutive bars are exactly one timeframe
//! apart, so no boundary fires and the scan reports zero gaps (the caller's
//! `sparse_gaps` flag then trips) — never a spurious gap (D-03 / RAD-3840 AC-3).
//!
//! Gap-fill convention: an up-gap (post-gap open above the prior close) *fills*
//! when a later bar's low retraces down to the prior close; a down-gap fills
//! when a later bar's high retraces up to the prior close. The fill search runs
//! over the forward window `[i+1, i+fill_lookahead_bars]` (clamped to the
//! series end) inclusive of the post-gap bar itself.
//!
//! Pattern analog: `seas/session/kernel.rs` — kernel-only file with `pub(crate)`
//! fns over primitive types plus a sibling `#[cfg(test)] mod tests`.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use chrono::{DateTime, Utc};

/// Resolved gap-detection configuration (parsed from `--params` by the scan
/// body with defaults applied).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GapConfig {
    /// Inter-bar open-timestamp delta in minutes; a delta STRICTLY GREATER than
    /// this marks a session/overnight boundary.
    pub boundary_gap_minutes: i64,
    /// Minimum absolute relative gap for a boundary to count as a gap event — a
    /// boundary whose `|gap|` does not exceed this is skipped.
    pub min_gap_threshold: f64,
    /// Ascending, strictly-positive size-bucket edges over `|relative gap|`.
    /// `num_buckets == size_bucket_edges.len() + 1`.
    pub size_bucket_edges: Vec<f64>,
    /// Forward window length in bars (clamped to `>= 1`) over which gap-fill is
    /// evaluated, inclusive of the post-gap (open) bar itself.
    pub fill_lookahead_bars: usize,
}

impl GapConfig {
    /// Number of size buckets = edge count + 1.
    pub(crate) fn num_buckets(&self) -> usize {
        self.size_bucket_edges.len() + 1
    }
}

/// One detected session/overnight gap event.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GapEvent {
    /// Epoch-milliseconds of the post-gap bar's open timestamp.
    pub ts_open_ms: i64,
    /// Signed relative gap `(open[i+1] - close[i]) / close[i]`.
    pub size: f64,
    /// `+1` for an up-gap (`open > prev close`), `-1` for a down-gap.
    pub direction: i8,
    /// Size-bucket index in `0..num_buckets`.
    pub bucket: usize,
    /// Number of bars from the post-gap bar (1-indexed) to the bar that fills
    /// the gap; `None` when the gap did not fill within the lookahead window.
    pub bars_to_fill: Option<usize>,
}

impl GapEvent {
    /// Did the gap fill within the lookahead window?
    pub(crate) fn filled(&self) -> bool {
        self.bars_to_fill.is_some()
    }
}

/// Classify `abs_gap` into a size bucket given ascending positive `edges`.
///
/// Returns the first index `i` where `abs_gap < edges[i]`, else `edges.len()`
/// (the open-ended top bucket). With no edges every gap lands in bucket 0.
pub(crate) fn size_bucket(abs_gap: f64, edges: &[f64]) -> usize {
    for (i, &e) in edges.iter().enumerate() {
        if abs_gap < e {
            return i;
        }
    }
    edges.len()
}

/// Detect every session/overnight gap event in the supplied OHLC series.
///
/// For each adjacent bar pair `(i, i+1)` whose open-timestamp delta exceeds
/// `cfg.boundary_gap_minutes`, the close→next-open jump is measured relative to
/// `close[i]`. A boundary becomes a gap event only when `|gap|` strictly
/// exceeds `cfg.min_gap_threshold`. Non-finite prices and a zero prior close
/// are skipped defensively.
pub(crate) fn detect_gaps(
    ts_open: &[DateTime<Utc>],
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    cfg: &GapConfig,
) -> Vec<GapEvent> {
    let n = close.len();
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    let lookahead = cfg.fill_lookahead_bars.max(1);
    for i in 0..n - 1 {
        let delta_min = (ts_open[i + 1] - ts_open[i]).num_minutes();
        if delta_min <= cfg.boundary_gap_minutes {
            continue;
        }
        let prev_close = close[i];
        if !prev_close.is_finite() || prev_close == 0.0 {
            continue;
        }
        let g = (open[i + 1] - prev_close) / prev_close;
        let abs_g = g.abs();
        if !abs_g.is_finite() || abs_g <= cfg.min_gap_threshold {
            continue;
        }
        let direction: i8 = if g > 0.0 { 1 } else { -1 };
        let bucket = size_bucket(abs_g, &cfg.size_bucket_edges);
        // Fill search over [i+1, i+lookahead] clamped to the series end.
        let last = i.saturating_add(lookahead).min(n - 1);
        let mut bars_to_fill: Option<usize> = None;
        for j in (i + 1)..=last {
            let filled = if direction > 0 {
                low[j] <= prev_close
            } else {
                high[j] >= prev_close
            };
            if filled {
                bars_to_fill = Some(j - i);
                break;
            }
        }
        out.push(GapEvent {
            ts_open_ms: ts_open[i + 1].timestamp_millis(),
            size: g,
            direction,
            bucket,
            bars_to_fill,
        });
    }
    out
}

/// Linear-interpolation quantile over a pre-sorted ascending slice (numpy /
/// pandas default `method="linear"`). `p` must be in `[0, 1]`; `sorted` must be
/// non-empty and sorted ascending.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "n is bounded by realistic gap counts; the floor/ceil indices come from the linear-interpolation quantile algorithm and are non-negative integers within slice bounds"
)]
pub(crate) fn linear_quantile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    debug_assert!(n >= 1, "linear_quantile: sorted must be non-empty");
    if n == 1 {
        return sorted[0];
    }
    let h = (n - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }
    let frac = h - h.floor();
    sorted[lo] * (1.0 - frac) + sorted[hi] * frac
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    const TOL: f64 = 1e-12;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() <= TOL
    }

    fn cfg(edges: Vec<f64>, lookahead: usize) -> GapConfig {
        GapConfig {
            boundary_gap_minutes: 23,
            min_gap_threshold: 0.0,
            size_bucket_edges: edges,
            fill_lookahead_bars: lookahead,
        }
    }

    #[test]
    fn size_bucket_classifies_against_ascending_edges() {
        let edges = [0.0005_f64, 0.001, 0.002];
        assert_eq!(size_bucket(0.0001, &edges), 0);
        assert_eq!(size_bucket(0.0005, &edges), 1, "edge is the bucket floor");
        assert_eq!(size_bucket(0.0007, &edges), 1);
        assert_eq!(size_bucket(0.001, &edges), 2);
        assert_eq!(size_bucket(0.0015, &edges), 2);
        assert_eq!(size_bucket(0.002, &edges), 3);
        assert_eq!(size_bucket(0.01, &edges), 3, "open-ended top bucket");
    }

    #[test]
    fn size_bucket_no_edges_is_single_bucket() {
        assert_eq!(size_bucket(0.0, &[]), 0);
        assert_eq!(size_bucket(123.0, &[]), 0);
    }

    /// Continuous series (every bar one timeframe apart) → zero gaps. RAD-3840
    /// AC-3: never a spurious gap on a gapless series.
    #[test]
    fn detect_gaps_continuous_series_has_no_gaps() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let n = 20;
        let ts: Vec<_> = (0..n)
            .map(|i| start + Duration::minutes(15 * i as i64))
            .collect();
        // Prices jump bar-to-bar but there is NO session boundary, so no gaps.
        let close: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
        let open = close.clone();
        let high: Vec<f64> = close.iter().map(|c| c + 1.0).collect();
        let low: Vec<f64> = close.iter().map(|c| c - 1.0).collect();
        let events = detect_gaps(&ts, &open, &high, &low, &close, &cfg(vec![0.0005], 4));
        assert!(events.is_empty(), "no boundary => no gaps");
    }

    /// Two sessions separated by a one-day break: a single up-gap that fills on
    /// the post-gap bar.
    #[test]
    fn detect_gaps_up_gap_fills_first_bar() {
        let d0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let d1 = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let ts = vec![d0, d1];
        let close = vec![100.0, 100.20];
        // post-gap bar opens at 100.20 (up gap), low dips to 99.99 <= 100 -> fill.
        let open = vec![100.0, 100.20];
        let high = vec![100.05, 100.30];
        let low = vec![99.95, 99.99];
        let events = detect_gaps(
            &ts,
            &open,
            &high,
            &low,
            &close,
            &cfg(vec![0.0005, 0.001], 4),
        );
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.direction, 1);
        assert!(e.filled());
        assert_eq!(e.bars_to_fill, Some(1));
        // (100.20 - 100.0) / 100.0 = 0.002 -> top bucket (>= last edge 0.001).
        assert_eq!(e.bucket, 2);
        assert!(approx_eq(e.size, (100.20 - 100.0) / 100.0));
        assert_eq!(e.ts_open_ms, d1.timestamp_millis());
    }

    /// A down-gap that never retraces within the lookahead window is unfilled.
    #[test]
    fn detect_gaps_down_gap_unfilled() {
        let d0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let d1 = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let d2 = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let ts = vec![d0, d1, d2];
        let close = vec![100.0, 99.0, 98.0];
        // down gap at d1: open 99.0 < prev close 100.0; high never reaches 100.
        let open = vec![100.0, 99.0, 98.0];
        let high = vec![100.05, 99.20, 98.20];
        let low = vec![99.95, 98.80, 97.80];
        let events = detect_gaps(&ts, &open, &high, &low, &close, &cfg(vec![0.0005], 1));
        // Two boundaries (d0->d1, d1->d2) => two down-gaps, neither fills.
        assert_eq!(events.len(), 2);
        for e in &events {
            assert_eq!(e.direction, -1);
            assert!(!e.filled());
            assert_eq!(e.bars_to_fill, None);
        }
    }

    /// Lookahead > 1: the gap fills on a later bar, not the post-gap bar.
    #[test]
    fn detect_gaps_fills_on_later_bar_within_lookahead() {
        let d0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let d1 = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        // Within-session bars 15m apart after the gap.
        let b1 = d1 + Duration::minutes(15);
        let b2 = d1 + Duration::minutes(30);
        let ts = vec![d0, d1, b1, b2];
        let close = vec![100.0, 100.5, 100.4, 100.3];
        let open = vec![100.0, 100.5, 100.45, 100.35];
        let high = vec![100.05, 100.6, 100.5, 100.4];
        // Lows: post-gap bar 100.30 (>100, no fill), next 100.10 (>100), third 99.90 (<=100 fill).
        let low = vec![99.95, 100.30, 100.10, 99.90];
        let events = detect_gaps(&ts, &open, &high, &low, &close, &cfg(vec![0.0005], 4));
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.direction, 1);
        assert!(e.filled());
        // post-gap bar = bars_to_fill 1; the fill is on the 3rd bar -> 3.
        assert_eq!(e.bars_to_fill, Some(3));
    }

    /// `min_gap_threshold` filters out boundaries with a tiny jump.
    #[test]
    fn detect_gaps_min_gap_threshold_filters_small_jumps() {
        let d0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let d1 = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        let ts = vec![d0, d1];
        let close = vec![100.0, 100.01];
        let open = vec![100.0, 100.01]; // |gap| = 0.0001
        let high = vec![100.05, 100.05];
        let low = vec![99.95, 99.95];
        let mut c = cfg(vec![0.0005], 1);
        c.min_gap_threshold = 0.0005;
        let events = detect_gaps(&ts, &open, &high, &low, &close, &c);
        assert!(events.is_empty(), "0.0001 jump below 0.0005 threshold");
    }

    #[test]
    fn linear_quantile_matches_numpy_linear() {
        let v = [0.0007_f64, 0.0015, 0.0015, 0.003, 0.003];
        assert!(approx_eq(linear_quantile(&v, 0.25), 0.0015));
        assert!(approx_eq(linear_quantile(&v, 0.5), 0.0015));
        assert!(approx_eq(linear_quantile(&v, 0.75), 0.003));
        // p=0.1 -> h=0.4 -> 0.6*0.0007 + 0.4*0.0015
        assert!(approx_eq(
            linear_quantile(&v, 0.1),
            0.6 * 0.0007 + 0.4 * 0.0015
        ));
    }

    #[test]
    fn linear_quantile_single_element() {
        assert!(approx_eq(linear_quantile(&[42.0], 0.5), 42.0));
    }
}

//! Return-series kernels — log / simple / intraday / overnight.
//!
//! Plan 04-02 / D4-06 — the `log_returns` body is a BYTE-IDENTICAL move of
//! the Phase 3 [`crate::scan::ljung_box`] `log_returns` function (Pitfall 9:
//! "move, do not rewrite"). `LjungBoxScan` post-Phase-4 imports this primitive
//! instead of carrying the body privately; the existing statsmodels golden
//! integration test (`crates/miner-core/tests/scan_ljung_box.rs`) pins the
//! byte-identical equality at the envelope level.
//!
//! ## Discipline
//!
//! - `#[inline]` on every kernel.
//! - Pure functions over `&[f64]` (and the timestamp slice for intraday /
//!   overnight); no IO, no allocations beyond the output `Vec`.
//! - Empty / singleton inputs return empty `Vec<f64>` naturally (via
//!   `slice::windows(2)` yielding zero elements).
//!
//! ## Intraday / overnight semantics
//!
//! `intraday_returns` and `overnight_returns` are defined relative to UTC date
//! (the bar-open timestamp's calendar date in UTC). Callers are responsible
//! for any timezone-shifting if they need session-local boundaries — primitives
//! stay timezone-neutral; the Phase 4 SEAS family will compose with
//! `calendar.rs` for session-aware buckets.
//!
//! Pattern analog: `crate::scan::ljung_box::kernel` — kernel-only file with a
//! sibling `#[cfg(test)] mod tests` block exercising the same edge cases
//! (`<fn>_basic`, `_empty`, `_singleton`, `_length_invariant`).

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use chrono::{DateTime, Datelike, Utc};

/// Compute log returns from a `close` price series:
/// `returns[t] = ln(close[t] / close[t-1])` for `t = 1..n`. Returns a
/// `Vec<f64>` of length `n - 1` (empty when `n < 2`).
///
/// Byte-identical move of the Phase 3 [`crate::scan::ljung_box`] `log_returns`
/// kernel (Plan 04-02 / D4-06 / Pitfall 9). The Phase 3 statsmodels golden
/// continues to pass after the `LjungBoxScan` refactor calls this primitive —
/// that equality is the regression guard.
#[inline]
#[must_use]
pub fn log_returns(close: &[f64]) -> Vec<f64> {
    close.windows(2).map(|w| (w[1] / w[0]).ln()).collect()
}

/// Compute simple (arithmetic) returns from a `close` price series:
/// `returns[t] = (close[t] - close[t-1]) / close[t-1]` for `t = 1..n`.
/// Returns a `Vec<f64>` of length `n - 1` (empty when `n < 2`).
///
/// Note: callers that need *log* returns should prefer [`log_returns`] —
/// log returns are time-additive across bar resolutions, simple returns are
/// not. Simple returns are kept here because some ANOM/SEAS surfaces use the
/// arithmetic form by convention.
#[inline]
#[must_use]
pub fn simple_returns(close: &[f64]) -> Vec<f64> {
    close.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect()
}

/// Compute intraday log returns: like [`log_returns`] but ONLY across bars
/// that share the same UTC date. When `ts[t-1]` and `ts[t]` fall on different
/// UTC dates the return at position `t-1` is skipped (overnight returns are
/// returned by the sibling [`overnight_returns`] kernel).
///
/// Returns a `Vec<f64>` whose length is at most `n - 1` (length depends on
/// the number of within-day bar transitions). Empty input or singleton input
/// returns the empty `Vec`. Slice lengths must match (`debug_assert!`).
#[inline]
#[must_use]
pub fn intraday_returns(close: &[f64], ts: &[DateTime<Utc>]) -> Vec<f64> {
    debug_assert_eq!(
        close.len(),
        ts.len(),
        "intraday_returns: close.len() must equal ts.len()"
    );
    if close.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(close.len() - 1);
    for i in 1..close.len() {
        if ts[i - 1].num_days_from_ce() == ts[i].num_days_from_ce() {
            out.push((close[i] / close[i - 1]).ln());
        }
    }
    out
}

/// Compute overnight log returns: like [`log_returns`] but ONLY across bars
/// that span a UTC date boundary. When `ts[t-1]` and `ts[t]` fall on different
/// UTC dates the return at position `t-1` is emitted (intraday returns are
/// returned by the sibling [`intraday_returns`] kernel).
///
/// Returns a `Vec<f64>` whose length is at most `n - 1` (one per date
/// transition observed). Empty input or singleton input returns the empty
/// `Vec`. Slice lengths must match (`debug_assert!`).
#[inline]
#[must_use]
pub fn overnight_returns(close: &[f64], ts: &[DateTime<Utc>]) -> Vec<f64> {
    debug_assert_eq!(
        close.len(),
        ts.len(),
        "overnight_returns: close.len() must equal ts.len()"
    );
    if close.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(close.len() - 1);
    for i in 1..close.len() {
        if ts[i - 1].num_days_from_ce() != ts[i].num_days_from_ce() {
            out.push((close[i] / close[i - 1]).ln());
        }
    }
    out
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

    // Local copy of the Phase 3 LjungBox `ar1_bar_frame_seeded`'s close-vector
    // builder — produces the same LCG-seeded series the byte-identical-move
    // test (Behavior Test 1) compares the lifted primitive's output against.
    //
    // Constants: Numerical Recipes LCG (a = 1664525, c = 1013904223, m = 2^32);
    // output scaled to `1.0 + (s / 2^32)` so closes land in (1.0, 2.0).
    #[allow(clippy::cast_possible_truncation)]
    fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed as u32;
        let mut closes = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            closes.push(1.0 + frac);
        }
        closes
    }

    /// Old ljung_box body — held here ONLY so the byte-identical move can be
    /// asserted at the kernel level. This is the literal Phase 3 body. If a
    /// future refactor changes the primitive, this test catches it.
    fn old_ljung_box_log_returns(close: &[f64]) -> Vec<f64> {
        close.windows(2).map(|w| (w[1] / w[0]).ln()).collect()
    }

    // -----------------------------------------------------------------------
    // log_returns — Behavior Tests 1, 3, 4 + sanity tests
    // -----------------------------------------------------------------------

    /// Behavior Test 1 — `log_returns_matches_ljung_box_kernel`. The lifted
    /// primitive must return a Vec byte-identical (tolerance 0.0) to the
    /// Phase 3 `ljung_box::kernel::log_returns` body for the AR(1) seeded
    /// series Phase 3 uses in `ar1_bar_frame_seeded`. D4-06 / Pitfall 9
    /// invariant — the merge requires a literal byte-for-byte equal Vec.
    #[test]
    fn log_returns_matches_ljung_box_kernel() {
        let closes = lcg_closes(256, 1);
        let primitive = log_returns(&closes);
        let phase3 = old_ljung_box_log_returns(&closes);
        assert_eq!(
            primitive.len(),
            phase3.len(),
            "lengths must be byte-identical"
        );
        // BYTE-EXACT equality (tolerance 0.0) per D4-06 / Pitfall 9.
        for (i, (got, want)) in primitive.iter().zip(phase3.iter()).enumerate() {
            assert!(
                got.to_bits() == want.to_bits(),
                "log_returns[{i}] byte-identical move failed: got {got} ({got_bits:#x}) vs want {want} ({want_bits:#x})",
                got_bits = got.to_bits(),
                want_bits = want.to_bits(),
            );
        }
    }

    #[test]
    fn log_returns_basic() {
        // close[t+1]/close[t] = e^1 each step -> ln = 1.0 twice.
        let e = std::f64::consts::E;
        let r = log_returns(&[1.0, e, e * e]);
        assert_eq!(r.len(), 2);
        assert!(approx_eq(r[0], 1.0, TOL));
        assert!(approx_eq(r[1], 1.0, TOL));
    }

    /// Behavior Test 3 — empty input returns empty Vec.
    #[test]
    fn log_returns_empty() {
        let r = log_returns(&[]);
        assert!(r.is_empty());
    }

    /// Behavior Test 4 — single-element input returns empty Vec.
    #[test]
    fn log_returns_singleton() {
        let r = log_returns(&[42.0]);
        assert!(r.is_empty());
    }

    #[test]
    fn log_returns_length_invariant() {
        // n closes -> n-1 returns.
        let closes: Vec<f64> = (1..=10).map(|i| f64::from(i)).collect();
        let r = log_returns(&closes);
        assert_eq!(r.len(), closes.len() - 1);
    }

    // -----------------------------------------------------------------------
    // simple_returns — Behavior Test 2 + sanity tests
    // -----------------------------------------------------------------------

    /// Behavior Test 2 — `simple_returns(&[1.0, 1.1, 1.21])` returns
    /// `[0.1, 0.1]` within 1e-12.
    #[test]
    fn simple_returns_basic() {
        let r = simple_returns(&[1.0, 1.1, 1.21]);
        assert_eq!(r.len(), 2);
        assert!(approx_eq(r[0], 0.1, TOL), "r[0]={}", r[0]);
        assert!(approx_eq(r[1], 0.1, TOL), "r[1]={}", r[1]);
    }

    #[test]
    fn simple_returns_empty() {
        assert!(simple_returns(&[]).is_empty());
    }

    #[test]
    fn simple_returns_singleton() {
        assert!(simple_returns(&[42.0]).is_empty());
    }

    // -----------------------------------------------------------------------
    // intraday_returns / overnight_returns — partition-on-date sanity
    // -----------------------------------------------------------------------

    #[test]
    fn intraday_and_overnight_partition_at_date_boundary() {
        // Build a 4-bar series: two bars on 2024-01-01, two on 2024-01-02. The
        // three transitions split as: intraday, overnight, intraday.
        let day0 = Utc.with_ymd_and_hms(2024, 1, 1, 23, 0, 0).unwrap();
        let ts = vec![
            day0,                         // 2024-01-01 23:00
            day0 + Duration::minutes(15), // 2024-01-01 23:15
            day0 + Duration::hours(1),    // 2024-01-02 00:00
            day0 + Duration::hours(2),    // 2024-01-02 01:00
        ];
        let close = vec![1.0_f64, 1.1, 1.2, 1.3];
        let intra = intraday_returns(&close, &ts);
        let over = overnight_returns(&close, &ts);
        // Three transitions total; one crosses the date boundary (between
        // 23:15 of day 0 and 00:00 of day 1 — i.e. position 1->2). So:
        // intraday returns transition 0->1 and 2->3; overnight returns 1->2.
        assert_eq!(intra.len(), 2, "two intraday transitions");
        assert_eq!(over.len(), 1, "one overnight transition");
    }

    #[test]
    fn intraday_returns_empty_and_singleton() {
        let now = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        assert!(intraday_returns(&[], &[]).is_empty());
        assert!(intraday_returns(&[1.0], &[now]).is_empty());
    }

    #[test]
    fn overnight_returns_empty_and_singleton() {
        let now = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        assert!(overnight_returns(&[], &[]).is_empty());
        assert!(overnight_returns(&[1.0], &[now]).is_empty());
    }
}

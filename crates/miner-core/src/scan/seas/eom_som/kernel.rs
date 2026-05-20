//! Pure `eom_som` kernel — trading-day-of-month bucket-key derivation.
//!
//! Pattern analog: `seas/hour_of_day/kernel.rs` — kernel-only file with
//! `#[inline] pub(super) fn` over primitive types and a sibling
//! `#[cfg(test)] mod tests` block. No IO, no `serde_json`, no `Reader` calls.
//!
//! ## Bucket scheme (SEAS-04 / RESEARCH §Section 2)
//!
//! `cutoff_n` controls the count of trading days at each month-edge. The output
//! is `Some(0..cutoff_n)` for the last `cutoff_n` trading days of the month
//! (`Some(0) == EOM-N`, `Some(cutoff_n - 1) == EOM-1`); `Some(cutoff_n..2*cutoff_n)`
//! for the first `cutoff_n` trading days (`Some(cutoff_n) == SOM-1`,
//! `Some(2*cutoff_n - 1) == SOM-N`); and `None` when the bar's date is more
//! than `cutoff_n` trading days from BOTH the month start and the month end
//! (middle-of-month — excluded from all buckets).
//!
//! Bucket index ordering (the assignment to `effect.extra` arrays) is:
//!
//! - indices `0..cutoff_n`: EOM-N, EOM-(N-1), ..., EOM-1
//! - indices `cutoff_n..2*cutoff_n`: SOM-1, SOM-2, ..., SOM-N
//!
//! so that "earliest in the calendar month" is to the right of the array and
//! "latest in the calendar month" is to the left. The corresponding label
//! strings are computed by the scan body (`mod.rs`) and emitted as
//! `effect.extra.bucket_labels` (UTF-8 JSON-encoded array bytes, mirroring
//! the SEAS-03 session bucket-labels encoding from Plan 04-09).
//!
//! ## Trading-day enumeration
//!
//! The trading-day list for the bar's month comes from the `Calendar` API
//! (Phase 2 D2-08). We enumerate `day=1..=days_in_month(year, month)` and
//! filter via `Calendar::is_open_at(midday)` — using a fixed 12:00 UTC time
//! anchor inside each candidate day so the FX-major Friday-22:00 close and
//! Sunday-22:00 open boundaries don't accidentally include weekend days that
//! only briefly overlap with the Sunday-evening session.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};

use crate::calendar::Calendar;

/// Compute the bucket index for `ts` under a trading-day-of-month scheme with
/// `cutoff_n` trading days at each edge.
///
/// Returns:
/// - `None` if `ts` lies on a non-trading day OR is in the middle of the
///   month (more than `cutoff_n` trading days from both the start and end);
/// - `Some(0..cutoff_n)` for the last `cutoff_n` trading days
///   (index 0 = EOM-N, index `cutoff_n - 1` = EOM-1);
/// - `Some(cutoff_n..2*cutoff_n)` for the first `cutoff_n` trading days
///   (index `cutoff_n` = SOM-1, index `2*cutoff_n - 1` = SOM-N).
///
/// # Panics
/// Panics via `debug_assert` when `cutoff_n < 1`.
#[inline]
#[must_use]
pub(crate) fn trading_day_of_month_bucket(
    ts: DateTime<Utc>,
    cutoff_n: usize,
    calendar: &Calendar,
) -> Option<usize> {
    debug_assert!(
        cutoff_n >= 1,
        "trading_day_of_month_bucket: cutoff_n must be >= 1"
    );

    let year = ts.year();
    let month = ts.month();
    let day = ts.day();

    // Enumerate trading days of the month (ascending). A "trading day" is any
    // calendar day for which `calendar.is_open_at(midday)` returns true. The
    // 12:00 UTC anchor is far enough inside each day to be unambiguous wrt
    // the Fri-22:00 close / Sun-22:00 open boundaries.
    let trading_days = trading_days_in_month(year, month, calendar);

    // Find the bar's day position in the trading-day list (if present).
    let pos = trading_days.iter().position(|d| *d == day)?;
    let n_trading = trading_days.len();
    // pos is in 0..n_trading.
    // From start: 0-indexed position is `pos`. From end: n_trading - 1 - pos.
    let from_end = n_trading - 1 - pos;

    if from_end < cutoff_n {
        // Last cutoff_n trading days. EOM-(from_end + 1) is at bucket index
        // cutoff_n - 1 - from_end (i.e. EOM-1 -> idx cutoff_n-1; EOM-N -> idx 0).
        Some(cutoff_n - 1 - from_end)
    } else if pos < cutoff_n {
        // First cutoff_n trading days. SOM-(pos + 1) -> bucket cutoff_n + pos.
        Some(cutoff_n + pos)
    } else {
        // Middle of month — excluded.
        None
    }
}

/// Enumerate the trading-day `u32` day-of-month values for the supplied
/// `(year, month)` (ascending). A trading day is a calendar day whose 12:00 UTC
/// timestamp satisfies `Calendar::is_open_at`. Returns an empty `Vec` for
/// months where every day is closed (extremely rare — every realistic month
/// has at least one weekday).
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    reason = "year fits i32; month/day fit u32 — Datelike API contract"
)]
fn trading_days_in_month(year: i32, month: u32, calendar: &Calendar) -> Vec<u32> {
    let mut days = Vec::with_capacity(31);
    for day in 1_u32..=31 {
        // Some months have < 31 days; from_ymd_opt returns None for invalid.
        let Some(naive) = NaiveDate::from_ymd_opt(year, month, day) else {
            continue;
        };
        // Anchor at 12:00 UTC (midday) on this day.
        let midday = naive.and_hms_opt(12, 0, 0).expect("12:00:00 is valid");
        let ts = Utc.from_utc_datetime(&midday);
        if calendar.is_open_at(ts) {
            days.push(day);
        }
        let _ = naive; // silence unused warning under cfg.
    }
    days
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Hand-derived — January 2024 is a 22-trading-day month (Jan 1 is a New
    /// Year's Day holiday so the first trading day is Jan 2; the last
    /// trading day is Jan 31, a Wednesday). With `cutoff_n=3` the last 3
    /// trading days are Jan 29 / 30 / 31 (Mon/Tue/Wed) and the first 3 are
    /// Jan 2 / 3 / 4 (Tue/Wed/Thu). Jan 15 is exactly middle-of-month and
    /// should fall in NO bucket.
    #[test]
    fn jan_2024_trading_days_count() {
        let cal = Calendar::fx_major();
        let days = trading_days_in_month(2024, 1, &cal);
        // Jan 1 is a Mon but is a New Year's Day holiday -> excluded.
        // Jan 6 (Sat), Jan 7 (Sun), Jan 13 (Sat), Jan 14 (Sun), Jan 20 (Sat),
        // Jan 21 (Sun), Jan 27 (Sat), Jan 28 (Sun) — 8 weekend days excluded.
        // Total: 31 - 1 (NYD) - 8 (weekends) = 22.
        assert_eq!(days.len(), 22, "January 2024 has 22 trading days");
        // First trading day is Jan 2.
        assert_eq!(days[0], 2);
        // Last trading day is Jan 31.
        assert_eq!(*days.last().unwrap(), 31);
    }

    /// EOM-3 / EOM-2 / EOM-1 for Jan 2024 are Jan 29 / 30 / 31.
    #[test]
    fn jan_2024_eom_buckets_cutoff_3() {
        let cal = Calendar::fx_major();
        let cutoff = 3;
        // Jan 31 = EOM-1 -> idx cutoff_n - 1 = 2.
        let ts = Utc.with_ymd_and_hms(2024, 1, 31, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, cutoff, &cal), Some(2));
        // Jan 30 = EOM-2 -> idx 1.
        let ts = Utc.with_ymd_and_hms(2024, 1, 30, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, cutoff, &cal), Some(1));
        // Jan 29 = EOM-3 -> idx 0.
        let ts = Utc.with_ymd_and_hms(2024, 1, 29, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, cutoff, &cal), Some(0));
    }

    /// SOM-1 / SOM-2 / SOM-3 for Jan 2024 (because Jan 1 is NYD-closed) are
    /// Jan 2 / 3 / 4.
    #[test]
    fn jan_2024_som_buckets_cutoff_3() {
        let cal = Calendar::fx_major();
        let cutoff = 3;
        // Jan 2 = SOM-1 -> idx cutoff_n + 0 = 3.
        let ts = Utc.with_ymd_and_hms(2024, 1, 2, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, cutoff, &cal), Some(3));
        // Jan 3 = SOM-2 -> idx 4.
        let ts = Utc.with_ymd_and_hms(2024, 1, 3, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, cutoff, &cal), Some(4));
        // Jan 4 = SOM-3 -> idx 5.
        let ts = Utc.with_ymd_and_hms(2024, 1, 4, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, cutoff, &cal), Some(5));
    }

    /// Jan 15 2024 is mid-month — falls in NO bucket with `cutoff_n=3`.
    #[test]
    fn jan_2024_middle_of_month_is_none() {
        let cal = Calendar::fx_major();
        let cutoff = 3;
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, cutoff, &cal), None);
    }

    /// A bar on a non-trading day (Saturday) returns None regardless of its
    /// proximity to the month edge.
    #[test]
    fn weekend_day_is_none() {
        let cal = Calendar::fx_major();
        // 2024-01-06 is a Saturday.
        let ts = Utc.with_ymd_and_hms(2024, 1, 6, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, 3, &cal), None);
    }

    /// New Year's Day (Jan 1) is a holiday so it does not get bucket 3
    /// (SOM-1); SOM-1 falls on Jan 2 instead (covered by
    /// `jan_2024_som_buckets_cutoff_3`).
    #[test]
    fn new_years_day_is_none() {
        let cal = Calendar::fx_major();
        let ts = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, 3, &cal), None);
    }

    /// With `cutoff_n=5` the EOM/SOM windows widen — for Jan 2024 the first 5
    /// trading days are Jan 2, 3, 4, 5, 8 (skipping the weekend) and the
    /// last 5 are Jan 25, 26, 29, 30, 31. With `cutoff_n=5` a 22-trading-day
    /// month puts Jan 8 at idx `cutoff_n` + 4 = 9 (SOM-5) and Jan 25 at
    /// idx `cutoff_n` - 1 - 4 = 0 (EOM-5).
    #[test]
    fn jan_2024_cutoff_5_widens_windows() {
        let cal = Calendar::fx_major();
        // SOM-5 = Jan 8 (5th trading day of month).
        let ts = Utc.with_ymd_and_hms(2024, 1, 8, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, 5, &cal), Some(9));
        // EOM-5 = Jan 25 (5th from end).
        let ts = Utc.with_ymd_and_hms(2024, 1, 25, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, 5, &cal), Some(0));
        // Jan 15 is now closer to middle — but with 22 trading days and
        // cutoff_n=5 the middle is days 6..=17 (positions 5..=16). Jan 15 is
        // a Monday; position 10 in the trading-day list. 5 <= 10 < 17 ->
        // None.
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, 5, &cal), None);
    }

    /// `Cutoff_n` = 1 collapses the scheme to "first vs last trading day of
    /// month". Two buckets total (EOM-1 -> idx 0; SOM-1 -> idx 1).
    #[test]
    fn cutoff_1_two_buckets() {
        let cal = Calendar::fx_major();
        // Jan 31 = EOM-1 -> idx 0.
        let ts = Utc.with_ymd_and_hms(2024, 1, 31, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, 1, &cal), Some(0));
        // Jan 2 = SOM-1 -> idx 1.
        let ts = Utc.with_ymd_and_hms(2024, 1, 2, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, 1, &cal), Some(1));
        // Jan 3 is now middle.
        let ts = Utc.with_ymd_and_hms(2024, 1, 3, 12, 0, 0).unwrap();
        assert_eq!(trading_day_of_month_bucket(ts, 1, &cal), None);
    }

    #[test]
    #[should_panic(expected = "cutoff_n must be >= 1")]
    fn cutoff_zero_panics_under_debug() {
        let cal = Calendar::fx_major();
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
        let _ = trading_day_of_month_bucket(ts, 0, &cal);
    }
}

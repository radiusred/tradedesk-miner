//! Pure `day_of_week` kernel — bucket-key derivation from chrono weekday.
//!
//! Pattern analog: `seas/hour_of_day/kernel.rs` (sibling Plan 04-09 Task 1).
//! Differs only in the bucket-key derivation: `Datelike::weekday()` +
//! `Weekday::num_days_from_monday()` (0=Mon..6=Sun per RESEARCH §Section 2 row).

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use chrono::{DateTime, Datelike, Utc};

/// Derive the per-bar bucket key from a sequence of UTC bar-open timestamps.
///
/// Returns a `Vec<usize>` of length `ts.len()` whose `i`-th entry is
/// `ts[i].weekday().num_days_from_monday()` — chrono guarantees the value is
/// in `0..=6` (0 = Monday, 6 = Sunday) per the SEAS-02 RESEARCH §Section 2
/// row.
#[inline]
#[must_use]
pub(crate) fn weekday_keys(ts: &[DateTime<Utc>]) -> Vec<usize> {
    ts.iter()
        .map(|dt| dt.weekday().num_days_from_monday() as usize)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    /// 2024-01-01 (a Monday in real calendars) is bucket 0; 2024-01-07 (Sunday)
    /// is bucket 6.
    #[test]
    fn weekday_keys_monday_is_zero_sunday_is_six() {
        // Build 7 daily timestamps starting at 2024-01-01.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..7).map(|i| start + Duration::days(i)).collect();
        let keys = weekday_keys(&ts);
        assert_eq!(keys, vec![0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn weekday_keys_empty() {
        assert!(weekday_keys(&[]).is_empty());
    }

    #[test]
    fn weekday_keys_length_invariant() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..96)
            .map(|i| start + Duration::minutes(15 * i64::from(i)))
            .collect();
        let keys = weekday_keys(&ts);
        assert_eq!(keys.len(), ts.len());
        for k in &keys {
            assert!(*k <= 6, "weekday key {k} must be in 0..=6");
        }
    }
}

//! Pure `hour_of_day` kernel — bucket-key derivation from chrono UTC hour.
//!
//! Pattern analog: `ljung_box/kernel.rs` — kernel-only file with `#[inline]
//! pub(super) fn` over primitive types and a sibling `#[cfg(test)] mod tests`
//! block. No IO, no `serde_json`, no `Reader` calls.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use chrono::{DateTime, Timelike, Utc};

/// Derive the per-bar bucket key from a sequence of UTC bar-open timestamps.
///
/// Returns a `Vec<usize>` of length `ts.len()` whose `i`-th entry is `ts[i].hour()`
/// — chrono guarantees the value is in `0..=23`. Per the SEAS-01 contract
/// (RESEARCH §1.3 / §Section 2) the bucket key is the UTC hour of the bar
/// whose close produced the return; callers using `log_returns(close)` index
/// timestamps starting at `ts[1..]` to align with the returns vector.
///
/// `#[inline]` for the same hot-path discipline as `ljung_box::kernel::log_returns`.
#[inline]
#[must_use]
pub(super) fn hour_keys(ts: &[DateTime<Utc>]) -> Vec<usize> {
    ts.iter()
        .map(|dt| dt.hour() as usize)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    /// Hand-derived 4 bars at 00:00, 00:15, 00:30, 00:45 + 1 bar at 01:00 ->
    /// hour keys [0, 0, 0, 0, 1].
    #[test]
    fn hour_keys_15m_within_hour() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..5)
            .map(|i| start + Duration::minutes(15 * i as i64))
            .collect();
        let keys = hour_keys(&ts);
        assert_eq!(keys, vec![0, 0, 0, 0, 1]);
    }

    /// Edge timestamps — 23:59 -> hour 23; 00:00 next day -> hour 0.
    #[test]
    fn hour_keys_midnight_boundary() {
        let t = Utc.with_ymd_and_hms(2024, 1, 1, 23, 59, 0).unwrap();
        let ts = vec![t, t + Duration::minutes(1)];
        let keys = hour_keys(&ts);
        assert_eq!(keys, vec![23, 0]);
    }

    /// Empty input -> empty output.
    #[test]
    fn hour_keys_empty() {
        assert!(hour_keys(&[]).is_empty());
    }

    /// Length invariant — `ts.len() == out.len()`.
    #[test]
    fn hour_keys_length_invariant() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..96)
            .map(|i| start + Duration::minutes(15 * i as i64))
            .collect();
        let keys = hour_keys(&ts);
        assert_eq!(keys.len(), ts.len());
        for k in &keys {
            assert!(*k <= 23, "hour key {k} must be in 0..=23");
        }
    }
}

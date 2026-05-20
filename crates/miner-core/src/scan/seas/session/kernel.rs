//! Pure `session` kernel — FX-major trading-session defaults + wrap-around-aware
//! UTC-hour membership predicate.
//!
//! Per RESEARCH §1.8 the four FX-major trading sessions in UTC are:
//! - Asia: 22:00 — 07:00 (wraps midnight)
//! - London: 07:00 — 16:00
//! - NY: 12:00 — 21:00
//! - Overlap (London + NY): 12:00 — 16:00
//!
//! Sessions are **independent** buckets: a bar at 13:00 UTC is in London, NY,
//! and Overlap simultaneously (NOT exclusive). The `hour_in_session` predicate
//! handles the Asia midnight-wrap case explicitly.
//!
//! Pattern analog: `seas/hour_of_day/kernel.rs` — kernel-only file with
//! `#[inline] pub(super) fn` over primitive types and a sibling
//! `#[cfg(test)] mod tests` block.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

/// One trading-session definition. `name` is the wire-form bucket label;
/// `start_utc_h` / `end_utc_h` are UTC hour-of-day in `0..=23`. The half-open
/// interval `[start, end)` is the in-session range; when `start > end` the
/// interval wraps midnight (e.g., Asia 22-07 means 22:00..23:59 OR 00:00..06:59).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionDef {
    pub name: &'static str,
    pub start_utc_h: u32,
    pub end_utc_h: u32,
}

/// FX-major default trading-session definitions per RESEARCH §1.8.
///
/// 4 sessions: Asia (wraps midnight), London, NY, Overlap (the London-NY
/// overlap window). The caller may override via `--params sessions` JSON.
pub const FX_MAJOR_DEFAULTS: &[SessionDef] = &[
    SessionDef {
        name: "asia",
        start_utc_h: 22,
        end_utc_h: 7,
    },
    SessionDef {
        name: "london",
        start_utc_h: 7,
        end_utc_h: 16,
    },
    SessionDef {
        name: "ny",
        start_utc_h: 12,
        end_utc_h: 21,
    },
    SessionDef {
        name: "overlap",
        start_utc_h: 12,
        end_utc_h: 16,
    },
];

/// Is the supplied UTC hour inside a session's `[start, end)` half-open
/// interval (wrap-around aware)?
///
/// - If `start <= end`: the interval is `[start, end)` directly — e.g.,
///   London 07-16 includes 07..15 (inclusive) and excludes 16.
/// - If `start > end`: the interval wraps midnight — e.g., Asia 22-07
///   includes `[22, 24)` ∪ `[0, 7)`: hour ≥ 22 OR hour < 7.
///
/// Empty intervals (`start == end`) match the empty set (no hour is in
/// session). This is the natural half-open convention and matches the
/// `[start, end)` rule for `start == end == 0`.
#[inline]
#[must_use]
pub(crate) fn hour_in_session(hour: u32, start: u32, end: u32) -> bool {
    if start <= end {
        // Half-open [start, end). Equal -> empty set.
        hour >= start && hour < end
    } else {
        // Wraps midnight: [start, 24) ∪ [0, end).
        hour >= start || hour < end
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 04-09 Task 3 hand-derived — Asia 22-07 (wraps midnight).
    #[test]
    fn hour_in_session_asia_wraps_midnight() {
        // Pre-midnight: 22, 23 -> in.
        assert!(hour_in_session(22, 22, 7));
        assert!(hour_in_session(23, 22, 7));
        // Post-midnight: 0..6 -> in.
        for h in 0..7 {
            assert!(hour_in_session(h, 22, 7), "hour {h} should be in Asia");
        }
        // Out: 7..=21 -> not in.
        for h in 7..=21 {
            assert!(!hour_in_session(h, 22, 7), "hour {h} should NOT be in Asia");
        }
        // Specific spec cases.
        assert!(hour_in_session(3, 22, 7));
        assert!(!hour_in_session(10, 22, 7));
    }

    /// London 07-16 (no wrap).
    #[test]
    fn hour_in_session_london_no_wrap() {
        for h in 0..7 {
            assert!(!hour_in_session(h, 7, 16));
        }
        for h in 7..16 {
            assert!(hour_in_session(h, 7, 16), "hour {h} should be in London");
        }
        // 16 is the half-open end -> NOT in.
        assert!(!hour_in_session(16, 7, 16));
        for h in 17..=23 {
            assert!(!hour_in_session(h, 7, 16));
        }
    }

    /// NY 12-21.
    #[test]
    fn hour_in_session_ny() {
        assert!(hour_in_session(12, 12, 21));
        assert!(hour_in_session(15, 12, 21));
        assert!(hour_in_session(20, 12, 21));
        assert!(!hour_in_session(21, 12, 21));
        assert!(!hour_in_session(11, 12, 21));
    }

    /// Overlap 12-16.
    #[test]
    fn hour_in_session_overlap() {
        assert!(hour_in_session(12, 12, 16));
        assert!(hour_in_session(15, 12, 16));
        assert!(!hour_in_session(16, 12, 16));
        assert!(!hour_in_session(11, 12, 16));
    }

    /// A bar at 13:00 UTC falls in BOTH London (07-16), NY (12-21), and
    /// Overlap (12-16) — confirming sessions are independent buckets.
    #[test]
    fn hour_13_falls_in_three_sessions() {
        assert!(hour_in_session(13, 7, 16), "13:00 ∈ London");
        assert!(hour_in_session(13, 12, 21), "13:00 ∈ NY");
        assert!(hour_in_session(13, 12, 16), "13:00 ∈ Overlap");
        assert!(!hour_in_session(13, 22, 7), "13:00 ∉ Asia");
    }

    #[test]
    fn fx_major_defaults_match_research_section_1_8() {
        assert_eq!(FX_MAJOR_DEFAULTS.len(), 4);
        let names: Vec<&str> = FX_MAJOR_DEFAULTS.iter().map(|d| d.name).collect();
        assert_eq!(names, vec!["asia", "london", "ny", "overlap"]);
        // (start, end) tuples
        let bounds: Vec<(u32, u32)> = FX_MAJOR_DEFAULTS
            .iter()
            .map(|d| (d.start_utc_h, d.end_utc_h))
            .collect();
        assert_eq!(bounds, vec![(22, 7), (7, 16), (12, 21), (12, 16)]);
    }
}

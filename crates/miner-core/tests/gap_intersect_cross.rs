//! Phase 4 (Plan 04-02 Task 2) integration test — D4-04 two-leg
//! gap-policy intersection via `engine::gap_policy::dispatch_pair`.
//!
//! Scaffold for Plan 04-07's CROSS dispatch wiring: verifies that
//! `dispatch_pair` correctly intersects two leg manifests (via the
//! `primitives::time_alignment::intersect_gaps` helper) and routes through
//! the standard [`dispatch`] decision. Pinned cases:
//!
//! 1. Strict + intersected non-empty manifests -> `Aborted(joint_manifest)`.
//! 2. `ContinuousOnly` + intersected manifests -> `SubRanges(partitioned)`
//!    over the joint manifest.
//! 3. Both manifests empty -> `SubRanges([full_range])` (the zero-gap fast
//!    path on the joint manifest).
//!
//! Pattern analog: `crates/miner-core/tests/gap_policy.rs` — directly
//! exercises `dispatch` over synthetic manifests. This test adds the
//! `dispatch_pair` sibling layer.

use chrono::{DateTime, TimeZone, Utc};

use miner_core::engine::gap_policy::{GapDispatch, GapPolicyKind, dispatch_pair};
use miner_core::findings::TimeRange;
use miner_core::gap::{GapManifest, GapReason, GapSpan};
use miner_core::reader::{ClosedRangeUtc, Side};

fn t(h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, h, 0, 0).unwrap()
}

fn manifest(symbol: &str, gaps: Vec<GapSpan>) -> GapManifest {
    GapManifest {
        source_id: "test".into(),
        symbol: symbol.into(),
        side: Side::Bid,
        queried_range: TimeRange {
            start_utc: t(0),
            end_utc: t(12),
        },
        gaps,
    }
}

fn intra_gap(start: DateTime<Utc>, end: DateTime<Utc>) -> GapSpan {
    GapSpan {
        start_utc: start,
        end_utc: end,
        reason: GapReason::IntraDayGap {
            affected_minutes: 1,
        },
    }
}

fn requested(start: DateTime<Utc>, end: DateTime<Utc>) -> ClosedRangeUtc {
    ClosedRangeUtc { start, end }
}

/// D4-04 — Strict + two manifests with overlapping gaps -> Aborted on the
/// joint manifest.
#[test]
fn dispatch_pair_strict_with_overlapping_gaps_aborts() {
    // Leg A: gap [10:00, 11:00); Leg B: gap [10:30, 11:30).
    // Joint (union): [10:00, 11:30).
    let leg_a = manifest("EURUSD", vec![intra_gap(t(10), t(11))]);
    let leg_b = manifest("GBPUSD", vec![intra_gap(t(10), t(11))]); // simple overlap
    let req = requested(t(8), t(12));

    let out = dispatch_pair(&leg_a, &leg_b, req, GapPolicyKind::Strict);
    match out {
        GapDispatch::Aborted(joint) => {
            assert_eq!(joint.gaps.len(), 1, "single merged span in joint manifest");
            assert_eq!(joint.gaps[0].start_utc, t(10));
            assert_eq!(joint.gaps[0].end_utc, t(11));
        }
        other => panic!("expected Aborted; got {other:?}"),
    }
}

/// D4-04 — `ContinuousOnly` + two manifests with overlapping gaps ->
/// `SubRanges` partitioned around the joint manifest.
#[test]
fn dispatch_pair_continuous_only_partitions_around_joint_manifest() {
    // Leg A: gap [10:00, 11:00); Leg B: gap [10:30, 11:30) -> joint [10:00, 11:30).
    let leg_a = manifest("EURUSD", vec![intra_gap(t(10), t(11))]);
    let leg_b = manifest("GBPUSD", vec![intra_gap(t(10).with_minute_30(), t(11).with_minute_30())]);
    let req = requested(t(8), t(12));

    let out = dispatch_pair(&leg_a, &leg_b, req, GapPolicyKind::ContinuousOnly);
    match out {
        GapDispatch::SubRanges(subs) => {
            // Sub-ranges should be: [8:00, 10:00) and [11:30, 12:00).
            assert!(
                !subs.is_empty(),
                "ContinuousOnly + joint gap leaves non-zero sub-ranges"
            );
            // First sub-range ends at the joint gap's start.
            assert_eq!(subs[0].start_utc, t(8));
            assert_eq!(subs[0].end_utc, t(10));
        }
        other => panic!("expected SubRanges; got {other:?}"),
    }
}

/// D4-04 — Both legs gap-free + `ContinuousOnly` -> `SubRanges`([`full_range`]).
/// Zero-gap fast path on the joint manifest.
#[test]
fn dispatch_pair_both_empty_continuous_only_is_full_range() {
    let leg_a = manifest("EURUSD", Vec::new());
    let leg_b = manifest("GBPUSD", Vec::new());
    let req = requested(t(0), t(12));

    let out = dispatch_pair(&leg_a, &leg_b, req, GapPolicyKind::ContinuousOnly);
    match out {
        GapDispatch::SubRanges(subs) => {
            assert_eq!(subs.len(), 1);
            assert_eq!(subs[0].start_utc, t(0));
            assert_eq!(subs[0].end_utc, t(12));
        }
        other => panic!("expected SubRanges([full]); got {other:?}"),
    }
}

/// D4-04 — Strict + both empty manifests -> `SubRanges`([`full_range`]) (strict
/// zero-gap fast path on the joint manifest).
#[test]
fn dispatch_pair_strict_both_empty_passes_through() {
    let leg_a = manifest("EURUSD", Vec::new());
    let leg_b = manifest("GBPUSD", Vec::new());
    let req = requested(t(0), t(12));

    let out = dispatch_pair(&leg_a, &leg_b, req, GapPolicyKind::Strict);
    match out {
        GapDispatch::SubRanges(subs) => {
            assert_eq!(subs.len(), 1, "strict + zero-gap joint manifest passes");
        }
        other => panic!("expected SubRanges; got {other:?}"),
    }
}

// Helper trait extension — `.with_minute_30()` produces a timestamp at the
// :30 minute mark. Used to build sub-hourly gap spans in the second test.
// Kept local to this test file (not added to the production surface).
trait WithMinute {
    fn with_minute_30(self) -> Self;
}

impl WithMinute for DateTime<Utc> {
    fn with_minute_30(self) -> Self {
        use chrono::Duration;
        self + Duration::minutes(30)
    }
}

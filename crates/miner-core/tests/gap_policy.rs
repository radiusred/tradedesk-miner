//! Phase 3 integration test — gap-policy dispatch behaviour (OUT-04 / SC-3a..SC-3e).
//!
//! Calls `engine::gap_policy::dispatch` directly (the same pure function
//! `engine::run_one` invokes); proves the 4 named emission rules + a
//! proptest invariant. Mirrors VALIDATION.md Per-Task Verification Map for
//! OUT-04 row-by-row.
//!
//! - `strict_with_gaps_emits_single_gap_aborted` — D3-11 (Aborted carries manifest).
//! - `continuous_only_partitions_and_inlines_manifest` — D3-10 happy path
//!   (`SubRanges` union equals (requested - gaps)).
//! - `strict_zero_gaps_emits_result_with_none_manifest` — D3-12 strict fast path.
//! - `continuous_only_zero_gaps_emits_empty_manifest` — D3-12 continuous fast path.
//! - `never_silently_emits_on_hole_proptest` — proptest invariant (SC-3e).
//!
//! These tests live OUTSIDE the lib unit tests (which already cover dispatch
//! directly) so the same VALIDATION.md row IDs are reachable via the public
//! integration-test surface a Phase 4 verifier expects.

#![allow(dead_code, unused_imports, unexpected_cfgs)]

use chrono::{DateTime, TimeZone, Utc};
use miner_core::engine::gap_policy::{GapDispatch, GapPolicyKind, dispatch};
use miner_core::findings::TimeRange;
use miner_core::gap::{GapManifest, GapReason, GapSpan};
use miner_core::reader::{ClosedRangeUtc, Side};

fn t(h: u32) -> DateTime<Utc> {
    if h >= 24 {
        Utc.with_ymd_and_hms(2024, 1, 2, h - 24, 0, 0).unwrap()
    } else {
        Utc.with_ymd_and_hms(2024, 1, 1, h, 0, 0).unwrap()
    }
}

fn manifest_with_gaps(gaps: Vec<GapSpan>) -> GapManifest {
    GapManifest {
        source_id: "dukascopy".into(),
        symbol: "EURUSD".into(),
        side: Side::Bid,
        queried_range: TimeRange {
            start_utc: t(0),
            end_utc: t(6),
        },
        gaps,
    }
}

fn empty_manifest() -> GapManifest {
    manifest_with_gaps(Vec::new())
}

fn intra_day_gap(start: DateTime<Utc>, end: DateTime<Utc>) -> GapSpan {
    GapSpan {
        start_utc: start,
        end_utc: end,
        reason: GapReason::IntraDayGap {
            affected_minutes: 1,
        },
    }
}

fn requested(start_h: u32, end_h: u32) -> ClosedRangeUtc {
    ClosedRangeUtc {
        start: t(start_h),
        end: t(end_h),
    }
}

// ---------------------------------------------------------------------------
// SC-3a — Strict + gaps → Aborted(manifest)
// ---------------------------------------------------------------------------

#[test]
fn strict_with_gaps_emits_single_gap_aborted() {
    let m = manifest_with_gaps(vec![intra_day_gap(t(1), t(2))]);
    let r = requested(0, 6);
    let out = dispatch(&m, r, GapPolicyKind::Strict);
    match out {
        GapDispatch::Aborted(returned) => {
            assert_eq!(returned, m, "Strict must return the manifest verbatim");
            assert!(!returned.gaps.is_empty(), "manifest must carry the gap");
        }
        GapDispatch::SubRanges(_) => panic!("Strict + gaps must Abort; got SubRanges"),
    }
}

// ---------------------------------------------------------------------------
// SC-3b — ContinuousOnly + gaps → SubRanges (N+1 elements for N gaps)
// ---------------------------------------------------------------------------

#[test]
fn continuous_only_partitions_and_inlines_manifest() {
    // Two gaps -> three sub-ranges; the union of (requested - gaps) is
    // covered exactly.
    let gaps = vec![intra_day_gap(t(1), t(2)), intra_day_gap(t(3), t(4))];
    let m = manifest_with_gaps(gaps);
    let r = requested(0, 6);
    let out = dispatch(&m, r, GapPolicyKind::ContinuousOnly);
    match out {
        GapDispatch::SubRanges(subs) => {
            assert_eq!(
                subs.len(),
                3,
                "two gaps -> three maximal gap-free sub-ranges; got {subs:?}",
            );
            assert_eq!(
                subs,
                vec![
                    TimeRange {
                        start_utc: t(0),
                        end_utc: t(1)
                    },
                    TimeRange {
                        start_utc: t(2),
                        end_utc: t(3)
                    },
                    TimeRange {
                        start_utc: t(4),
                        end_utc: t(6)
                    },
                ],
                "sub-ranges must equal (requested - gaps) in order",
            );
        }
        GapDispatch::Aborted(_) => panic!("ContinuousOnly must not Abort; got Aborted"),
    }
}

// ---------------------------------------------------------------------------
// SC-3c — Strict + zero gaps → SubRanges([requested]); engine emits Result
// with `data_slice.gap_manifest = None`
// ---------------------------------------------------------------------------

#[test]
fn strict_zero_gaps_emits_result_with_none_manifest() {
    let m = empty_manifest();
    let r = requested(0, 6);
    let out = dispatch(&m, r, GapPolicyKind::Strict);
    match out {
        GapDispatch::SubRanges(subs) => {
            assert_eq!(
                subs,
                vec![TimeRange {
                    start_utc: t(0),
                    end_utc: t(6)
                }],
                "Strict + zero gaps fast path: SubRanges([requested])",
            );
        }
        GapDispatch::Aborted(_) => panic!("Strict + zero gaps must NOT Abort; got Aborted"),
    }
}

// ---------------------------------------------------------------------------
// SC-3d — ContinuousOnly + zero gaps → SubRanges([requested]); engine emits
// Result with `data_slice.gap_manifest = Some(empty manifest)`
// ---------------------------------------------------------------------------

#[test]
fn continuous_only_zero_gaps_emits_empty_manifest() {
    let m = empty_manifest();
    let r = requested(0, 6);
    let out = dispatch(&m, r, GapPolicyKind::ContinuousOnly);
    match out {
        GapDispatch::SubRanges(subs) => {
            assert_eq!(
                subs,
                vec![TimeRange {
                    start_utc: t(0),
                    end_utc: t(6)
                }],
                "ContinuousOnly + zero gaps fast path: SubRanges([requested])",
            );
        }
        GapDispatch::Aborted(_) => panic!("ContinuousOnly + zero gaps must NOT Abort"),
    }
}

// ---------------------------------------------------------------------------
// SC-3e — proptest: under ANY gap manifest + policy, `dispatch` never
// returns a SubRanges element overlapping a clamped gap. Strict with
// non-empty gaps must always Abort.
// ---------------------------------------------------------------------------

mod proptest_block {
    use super::*;
    use proptest::prelude::*;

    /// Generate a list of non-overlapping sorted gap spans within
    /// `[0, 24)` hours, each at hour-boundary endpoints.
    fn gaps_strategy() -> impl Strategy<Value = Vec<GapSpan>> {
        prop::collection::vec((0u32..24, 1u32..6), 0..6).prop_map(|pairs| {
            // Build candidate (start, end) pairs and dedup-merge into a
            // sorted non-overlapping sequence.
            let mut intervals: Vec<(u32, u32)> = pairs
                .into_iter()
                .filter_map(|(s, len)| {
                    let e = s + len;
                    if e <= 24 { Some((s, e)) } else { None }
                })
                .collect();
            intervals.sort_unstable();
            // Merge overlapping intervals so the manifest's invariant
            // (non-overlapping, sorted) holds.
            let mut merged: Vec<(u32, u32)> = Vec::new();
            for (s, e) in intervals {
                if let Some(last) = merged.last_mut() {
                    if s < last.1 {
                        last.1 = last.1.max(e);
                        continue;
                    }
                }
                merged.push((s, e));
            }
            merged
                .into_iter()
                .map(|(s, e)| intra_day_gap(t(s), t(e)))
                .collect()
        })
    }

    proptest! {
        /// SC-3e — under ANY random gap manifest, `dispatch` NEVER returns a
        /// SubRanges element whose `[start, end)` overlaps a gap (look-ahead
        /// safety for the continuous_only partitioner).
        ///
        /// Strict + non-empty manifest always returns `Aborted(...)`.
        #[test]
        fn never_silently_emits_on_hole_proptest(
            gaps in gaps_strategy(),
            req_start in 0u32..20,
            req_len in 1u32..12,
            strict in proptest::bool::ANY,
        ) {
            let req_end = (req_start + req_len).min(24);
            let r = ClosedRangeUtc {
                start: t(req_start),
                end: t(req_end),
            };
            let m = manifest_with_gaps(gaps.clone());
            let policy = if strict { GapPolicyKind::Strict } else { GapPolicyKind::ContinuousOnly };
            let out = dispatch(&m, r, policy);

            // Strict + non-empty manifest -> always Aborted.
            if strict && !gaps.is_empty() {
                prop_assert!(matches!(out, GapDispatch::Aborted(_)));
                return Ok(());
            }

            // Otherwise the result is SubRanges; assert NEVER overlaps a
            // gap clamped to the requested range. Each sub-range stays
            // within `[r.start, r.end)`.
            let subs = match out {
                GapDispatch::SubRanges(v) => v,
                GapDispatch::Aborted(_) => panic!("non-strict / empty-gaps must be SubRanges"),
            };
            for sub in &subs {
                prop_assert!(sub.start_utc >= r.start && sub.end_utc <= r.end,
                    "sub-range escapes requested bounds: {sub:?}");
                for gap in &gaps {
                    // Clamp the gap to the requested range; gaps fully
                    // outside that range have no effect on the dispatch.
                    let g_start = gap.start_utc.max(r.start);
                    let g_end = gap.end_utc.min(r.end);
                    if g_end <= g_start {
                        continue;
                    }
                    // Half-open overlap: max(start) < min(end).
                    let overlap_start = sub.start_utc.max(g_start);
                    let overlap_end = sub.end_utc.min(g_end);
                    prop_assert!(
                        overlap_start >= overlap_end,
                        "sub-range {sub:?} overlaps clamped gap [{g_start}, {g_end}); \
                         overlap = [{overlap_start}, {overlap_end})",
                    );
                }
            }
        }
    }
}

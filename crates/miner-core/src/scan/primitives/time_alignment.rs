//! Two-leg time-alignment primitives (CROSS-01 + D4-04).
//!
//! Plan 04-02 picks this module as the home for both helpers per PATTERNS.md
//! Pattern I — co-located with the inner-join primitive that CROSS-01 owns.
//!
//! - [`inner_join`] — inner-join two `BarFrame`s on common `ts_open_utc`.
//!   Returns an [`AlignedPair`] whose `timestamps_ms`, `close_a`, `close_b`
//!   vectors are the same length and parallel.
//! - [`intersect_gaps`] — interval-intersection on two `GapManifest`s' spans.
//!   The output manifest's `gaps` cover the union of timestamps that are
//!   missing in EITHER leg (the joint "do not run" set CROSS scans dispatch
//!   on). Per PATTERNS.md Pattern I, the home was Plan-chosen between (a)
//!   here and (b) `gap.rs::GapManifest::intersect`; (a) wins because both
//!   helpers are owned by the CROSS-01 surface.
//!
//! Both helpers are pure functions — no IO, no allocations beyond their
//! output collections.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use crate::aggregator::BarFrame;
use crate::findings::TimeRange;
use crate::gap::{GapManifest, GapReason, GapSpan};

/// Output of [`inner_join`] — three parallel vectors with the joint
/// timestamp index (epoch-ms) and per-leg close prices at each joint
/// timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignedPair {
    /// Joint timestamp index in epoch-ms (UTC). Strictly ascending — the
    /// invariant is inherited from the input `BarFrame.ts_open_utc` vectors
    /// (Phase 2 `aggregator` produces sorted ascending output).
    pub timestamps_ms: Vec<i64>,
    /// `close` value from leg A at each joint timestamp.
    pub close_a: Vec<f64>,
    /// `close` value from leg B at each joint timestamp.
    pub close_b: Vec<f64>,
}

/// Inner-join two `BarFrame`s on common `ts_open_utc`. Two-pointer sweep over
/// both sorted timestamp vectors; when timestamps match, push the joint
/// timestamp (as epoch-ms) plus each leg's `close[i]` to the output.
///
/// Body per RESEARCH.md "Two-leg time alignment via inner-join" code block
/// at lines 596-628. Returns an `AlignedPair` directly (no `Result`) — the
/// primitive cannot fail; if either input is empty or the timestamp
/// intersection is empty the output's three vectors are zero-length.
///
/// Invariants:
/// - The input `BarFrame`s' `ts_open_utc` vectors must be sorted ascending
///   (Phase 2 invariant — `aggregator.rs` guarantees this for any frame
///   produced by `BarCache::get_or_build`). The output's `timestamps_ms` is
///   therefore also sorted ascending.
/// - `bars_a.close.len() == bars_a.ts_open_utc.len()` and the same for
///   `bars_b` (Phase 2 column-length invariant — see `aggregator::BarFrame`
///   doc-comment).
#[must_use]
pub fn inner_join(bars_a: &BarFrame, bars_b: &BarFrame) -> AlignedPair {
    debug_assert_eq!(
        bars_a.ts_open_utc.len(),
        bars_a.close.len(),
        "BarFrame column-length invariant: ts_open_utc and close must match"
    );
    debug_assert_eq!(
        bars_b.ts_open_utc.len(),
        bars_b.close.len(),
        "BarFrame column-length invariant: ts_open_utc and close must match"
    );

    let mut ia = 0usize;
    let mut ib = 0usize;
    let mut out_ts: Vec<i64> = Vec::new();
    let mut out_a: Vec<f64> = Vec::new();
    let mut out_b: Vec<f64> = Vec::new();
    while ia < bars_a.ts_open_utc.len() && ib < bars_b.ts_open_utc.len() {
        let ta = bars_a.ts_open_utc[ia].timestamp_millis();
        let tb = bars_b.ts_open_utc[ib].timestamp_millis();
        match ta.cmp(&tb) {
            std::cmp::Ordering::Less => ia += 1,
            std::cmp::Ordering::Greater => ib += 1,
            std::cmp::Ordering::Equal => {
                out_ts.push(ta);
                out_a.push(bars_a.close[ia]);
                out_b.push(bars_b.close[ib]);
                ia += 1;
                ib += 1;
            }
        }
    }
    AlignedPair {
        timestamps_ms: out_ts,
        close_a: out_a,
        close_b: out_b,
    }
}

/// Intersect two gap manifests at the interval level (D4-04 helper).
///
/// Produces a manifest whose `gaps` cover the UNION of timestamps that are
/// missing in EITHER leg — the joint "do not run" set the engine's two-leg
/// gap dispatch hands to `gap_policy::dispatch` for the Pair branch.
///
/// Algorithm (O(n+m) sweep over sorted spans):
/// 1. Collect both manifests' spans, tagging each by its origin leg.
/// 2. Sweep them in `start_utc` order, merging overlapping or touching spans
///    into a single span. The resulting `GapSpan::reason` is the more
///    conservative of the two contributors — we reuse the existing variants
///    rather than adding `GapReason::EitherLeg`, matching PATTERNS.md
///    Pattern I's "pick the more conservative reason per span" guidance.
///
/// The output manifest's identity fields (`source_id`, `symbol`, `side`,
/// `queried_range`) are taken from leg A — CROSS dispatch builds a fresh
/// `GapAborted` envelope that carries both legs' provenance via
/// `data_slice.sources` (D4-03), so the manifest's own identity is just an
/// administrative label for downstream tooling.
///
/// Invariants:
/// - Both inputs must have `gaps` sorted ascending by `start_utc` (Phase 2
///   `GapDetector` invariant — `debug_assert!` confirms it in debug builds).
/// - The output `gaps` are also sorted ascending and non-overlapping.
#[must_use]
pub fn intersect_gaps(a: &GapManifest, b: &GapManifest) -> GapManifest {
    debug_assert!(
        a.gaps.windows(2).all(|w| w[0].start_utc <= w[1].start_utc),
        "intersect_gaps: input A must be sorted by start_utc"
    );
    debug_assert!(
        b.gaps.windows(2).all(|w| w[0].start_utc <= w[1].start_utc),
        "intersect_gaps: input B must be sorted by start_utc"
    );

    // Collect all spans into one Vec, then sort by start_utc and merge
    // overlapping/touching spans. O((n+m) log (n+m)) — the sort is the
    // upper bound; for typical manifest sizes (< a few hundred spans) the
    // constant factor is irrelevant.
    let mut spans: Vec<GapSpan> = Vec::with_capacity(a.gaps.len() + b.gaps.len());
    spans.extend(a.gaps.iter().cloned());
    spans.extend(b.gaps.iter().cloned());
    spans.sort_by_key(|s| (s.start_utc, s.end_utc));

    let mut out: Vec<GapSpan> = Vec::new();
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start_utc <= last.end_utc => {
                // Overlap or touching — extend the last span's end_utc.
                if span.end_utc > last.end_utc {
                    last.end_utc = span.end_utc;
                }
                // Pick the more conservative reason: MissingSourceFile > IntraDayGap.
                // (CorruptSourceFile sits between but is rare; v1 prefers it
                // over IntraDay and below MissingSourceFile per their
                // discriminant ords.)
                last.reason = conservative_reason(&last.reason, &span.reason);
            }
            _ => out.push(span),
        }
    }

    // Identity: use leg A's metadata. The queried_range covers the union of
    // both legs' queried ranges so consumers see the full joint window.
    let queried_range = TimeRange {
        start_utc: a.queried_range.start_utc.min(b.queried_range.start_utc),
        end_utc: a.queried_range.end_utc.max(b.queried_range.end_utc),
    };

    GapManifest {
        source_id: a.source_id.clone(),
        symbol: a.symbol.clone(),
        side: a.side,
        queried_range,
        gaps: out,
    }
}

/// Pick the more conservative `GapReason` between two contributors when
/// merging overlapping spans. Order (most conservative first):
/// `MissingSourceFile > CorruptSourceFile > IntraDayGap` — matches the
/// discriminant-ord values declared in `gap.rs::GapReason::discriminant_ord`.
fn conservative_reason(left: &GapReason, right: &GapReason) -> GapReason {
    if left.discriminant_ord() <= right.discriminant_ord() {
        left.clone()
    } else {
        right.clone()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::Timeframe;
    use crate::reader::Side;
    use chrono::{DateTime, Duration, TimeZone, Utc};

    fn t(min: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + Duration::minutes(min)
    }

    fn build_bars(symbol: &str, ts: &[DateTime<Utc>], closes: &[f64]) -> BarFrame {
        assert_eq!(ts.len(), closes.len(), "test fixture mismatch");
        BarFrame {
            source_id: "test".into(),
            symbol: symbol.into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts.to_vec(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
            open: closes.to_vec(),
            high: closes.iter().map(|c| c + 0.001).collect(),
            low: closes.iter().map(|c| c - 0.001).collect(),
            close: closes.to_vec(),
            tick_volume: vec![1.0; ts.len()],
        }
    }

    // -----------------------------------------------------------------------
    // inner_join — Behavior Tests 5, 6
    // -----------------------------------------------------------------------

    /// Behavior Test 5 — two `BarFrames` with zero overlapping `ts_open_utc`
    /// return an `AlignedPair` with len 0.
    #[test]
    fn inner_join_disjoint_returns_empty() {
        let a = build_bars("A", &[t(0), t(15), t(30)], &[1.0, 1.1, 1.2]);
        let b = build_bars("B", &[t(7), t(22), t(37)], &[2.0, 2.1, 2.2]);
        let aligned = inner_join(&a, &b);
        assert_eq!(aligned.timestamps_ms.len(), 0);
        assert_eq!(aligned.close_a.len(), 0);
        assert_eq!(aligned.close_b.len(), 0);
    }

    /// Behavior Test 6 — partial overlap with one shared sub-window. Frame A
    /// covers timestamps `[1,2,3]`, frame B covers `[2,3,4]` (units of 15
    /// minutes); the join produces `[2,3]` with parallel close values from
    /// each leg at those joint timestamps.
    #[test]
    fn inner_join_partial_overlap() {
        let ta = [t(15), t(30), t(45)]; // positions 1,2,3
        let tb = [t(30), t(45), t(60)]; // positions 2,3,4
        let a_closes = [10.0, 11.0, 12.0];
        let b_closes = [20.0, 21.0, 22.0];
        let a = build_bars("A", &ta, &a_closes);
        let b = build_bars("B", &tb, &b_closes);
        let aligned = inner_join(&a, &b);
        assert_eq!(aligned.timestamps_ms.len(), 2);
        assert_eq!(aligned.timestamps_ms[0], t(30).timestamp_millis());
        assert_eq!(aligned.timestamps_ms[1], t(45).timestamp_millis());
        // close_a indices [1, 2] (the positions in `ta` that match).
        assert_eq!(aligned.close_a, vec![a_closes[1], a_closes[2]]);
        // close_b indices [0, 1] (the positions in `tb` that match).
        assert_eq!(aligned.close_b, vec![b_closes[0], b_closes[1]]);
    }

    #[test]
    fn inner_join_full_overlap() {
        // Two frames sharing every timestamp.
        let ts = [t(0), t(15), t(30)];
        let a = build_bars("A", &ts, &[1.0, 1.1, 1.2]);
        let b = build_bars("B", &ts, &[2.0, 2.1, 2.2]);
        let aligned = inner_join(&a, &b);
        assert_eq!(aligned.timestamps_ms.len(), 3);
        assert_eq!(aligned.close_a, vec![1.0, 1.1, 1.2]);
        assert_eq!(aligned.close_b, vec![2.0, 2.1, 2.2]);
    }

    #[test]
    fn inner_join_empty_inputs() {
        let a = build_bars("A", &[], &[]);
        let b = build_bars("B", &[t(0)], &[1.0]);
        let aligned = inner_join(&a, &b);
        assert_eq!(aligned.timestamps_ms.len(), 0);
        let aligned2 = inner_join(&b, &a);
        assert_eq!(aligned2.timestamps_ms.len(), 0);
    }

    // -----------------------------------------------------------------------
    // intersect_gaps — Behavior Tests 7, 8
    // -----------------------------------------------------------------------

    fn manifest(symbol: &str, gaps: Vec<GapSpan>) -> GapManifest {
        GapManifest {
            source_id: "test".into(),
            symbol: symbol.into(),
            side: Side::Bid,
            queried_range: TimeRange {
                start_utc: t(0),
                end_utc: t(120),
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

    /// Behavior Test 7 — two `GapManifests` with non-overlapping spans
    /// intersect to a NON-empty manifest containing BOTH (the union of
    /// timestamps missing in EITHER leg). Re-read: "`intersect_gaps`" here
    /// means "intersection of running-windows" = "union of gap intervals"
    /// in the joint two-leg manifest.
    #[test]
    fn intersect_gaps_no_overlap_keeps_both_intervals() {
        let a = manifest("A", vec![intra_gap(t(10), t(20))]);
        let b = manifest("B", vec![intra_gap(t(30), t(40))]);
        let joint = intersect_gaps(&a, &b);
        assert_eq!(joint.gaps.len(), 2, "non-overlapping gaps stay separate");
        assert_eq!(joint.gaps[0].start_utc, t(10));
        assert_eq!(joint.gaps[0].end_utc, t(20));
        assert_eq!(joint.gaps[1].start_utc, t(30));
        assert_eq!(joint.gaps[1].end_utc, t(40));
    }

    /// Behavior Test 7b — two `GapManifests` with empty gap-lists intersect to
    /// an empty manifest.
    #[test]
    fn intersect_gaps_both_empty_is_empty() {
        let a = manifest("A", Vec::new());
        let b = manifest("B", Vec::new());
        let joint = intersect_gaps(&a, &b);
        assert!(joint.gaps.is_empty());
    }

    /// Behavior Test 8 — manifests with overlapping spans intersect to a
    /// MERGED span covering the union of both contributors.
    #[test]
    fn intersect_gaps_partial_overlap_merges() {
        let a = manifest("A", vec![intra_gap(t(10), t(25))]);
        let b = manifest("B", vec![intra_gap(t(20), t(35))]);
        let joint = intersect_gaps(&a, &b);
        assert_eq!(joint.gaps.len(), 1, "overlap merges to one span");
        assert_eq!(joint.gaps[0].start_utc, t(10));
        assert_eq!(joint.gaps[0].end_utc, t(35));
    }

    #[test]
    fn intersect_gaps_touching_spans_merge() {
        let a = manifest("A", vec![intra_gap(t(10), t(20))]);
        let b = manifest("B", vec![intra_gap(t(20), t(30))]);
        let joint = intersect_gaps(&a, &b);
        // Spans touch at t(20); merge into a single span [10, 30).
        assert_eq!(joint.gaps.len(), 1);
        assert_eq!(joint.gaps[0].start_utc, t(10));
        assert_eq!(joint.gaps[0].end_utc, t(30));
    }

    #[test]
    fn intersect_gaps_conservative_reason() {
        // Leg A has a MissingSourceFile gap; leg B has an overlapping
        // IntraDayGap. The merged span must adopt the more conservative
        // reason (MissingSourceFile, discriminant_ord 0).
        let date = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let missing = GapSpan {
            start_utc: t(10),
            end_utc: t(20),
            reason: GapReason::MissingSourceFile { date },
        };
        let intra = intra_gap(t(15), t(25));
        let a = manifest("A", vec![missing.clone()]);
        let b = manifest("B", vec![intra]);
        let joint = intersect_gaps(&a, &b);
        assert_eq!(joint.gaps.len(), 1);
        assert!(matches!(
            joint.gaps[0].reason,
            GapReason::MissingSourceFile { .. }
        ));
    }
}

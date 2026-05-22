//! Gap-policy dispatch: `strict` / `continuous_only` → finding-emission plan.
//!
//! Pattern analog: `gap.rs:152-187` ([`crate::gap::GapDetector`]) — stateless
//! function dispatch (no fields, no configuration) + tagged-enum policy kind
//! mirroring `GapReason` shape (`gap.rs:117-130`).
//!
//! ## Phase 3 contract (D3-08..D3-12)
//!
//! - `strict` + gaps present → one `Finding::GapAborted` (D3-11), NO `Result`.
//! - `strict` + zero gaps → fast path: scan runs, `data_slice.gap_manifest = None`.
//! - `continuous_only` + gaps → partition the requested range into
//!   maximal gap-free sub-ranges (D3-10); one `Finding::Result` per sub-range
//!   with the FULL gap manifest inlined in `data_slice.gap_manifest`.
//! - `continuous_only` + zero gaps → one `Result` with
//!   `data_slice.gap_manifest = Some(GapManifest { gaps: vec![] })` (D3-12).
//!
//! [`dispatch`] is a PURE function — given the same `(manifest, requested,
//! policy)` triple it returns the same [`GapDispatch`]. No clock reads, no
//! file IO, no allocations beyond the returned `Vec<TimeRange>`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::aggregator::{Timeframe, align_down, align_up};
use crate::findings::TimeRange;
use crate::gap::GapManifest;
use crate::reader::ClosedRangeUtc;

// ---------------------------------------------------------------------------
// GapPolicyKind — tagged enum, mirrors gap.rs:117-130 GapReason shape.
// ---------------------------------------------------------------------------

/// Gap-handling policy for a scan invocation (D3-19 CLI surface).
///
/// `#[serde(rename_all = "snake_case")]` so JSON wire form is
/// `"strict"` / `"continuous_only"` (matches the user-facing `--gap-policy`
/// flag spelling).
///
/// Pattern analog: `reader.rs::Side` (`Bid`/`Ask`) unit-variant enum with
/// `as_str(&self) -> &'static str`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GapPolicyKind {
    /// Reject any window touching a gap — emit one `Finding::GapAborted`,
    /// NO `Result`. Exit 0 (per D-08 — strict + gaps is NOT a preflight
    /// failure, it's a documented outcome).
    Strict,
    /// Partition the window into maximal gap-free sub-ranges — emit one
    /// `Finding::Result` per sub-range with the full manifest inlined.
    ContinuousOnly,
}

impl GapPolicyKind {
    /// Wire form, matching `--gap-policy <VALUE>`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            GapPolicyKind::Strict => "strict",
            GapPolicyKind::ContinuousOnly => "continuous_only",
        }
    }

    /// Inverse of [`GapPolicyKind::as_str`] — parse the canonical CLI / wire
    /// form (`"strict"` / `"continuous_only"`) into a [`GapPolicyKind`]. Used
    /// by Plan 03-05 to convert the clap-parsed `--gap-policy` string into
    /// the typed enum at the CLI preflight boundary.
    ///
    /// # Errors
    /// Returns the input `&str` unchanged when it is not one of the two
    /// canonical forms; callers convert the error into a typed `WireError`
    /// with appropriate context.
    ///
    /// We do NOT implement `std::str::FromStr` because that trait's
    /// `Err: Display` requirement would force allocation; the borrowed `&str`
    /// is exactly what the preflight wrapper site needs.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, &str> {
        match s {
            "strict" => Ok(Self::Strict),
            "continuous_only" => Ok(Self::ContinuousOnly),
            _ => Err(s),
        }
    }
}

// ---------------------------------------------------------------------------
// GapDispatch — what the policy dispatcher returns to the facade.
// ---------------------------------------------------------------------------

/// Output of [`dispatch`] — drives the facade's finding-emission decision.
///
/// `Aborted(manifest)` carries the manifest the `Finding::GapAborted` envelope
/// will inline (D3-11). `SubRanges(Vec<TimeRange>)` carries the gap-free
/// partition the facade iterates over, calling `Scan::run` once per sub-range
/// (D3-10).
///
/// No `Serialize` derive — this is an internal facade type (Plan 04 consumes
/// via match).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapDispatch {
    /// Strict policy + gaps present — emit one `GapAborted` and stop.
    Aborted(GapManifest),
    /// Continuous-only OR strict-with-zero-gaps — run the scan over these
    /// (possibly single-element) maximal gap-free sub-ranges.
    SubRanges(Vec<TimeRange>),
}

// ---------------------------------------------------------------------------
// dispatch — the stateless function the facade calls.
// ---------------------------------------------------------------------------

/// Compute the gap-policy dispatch for a `(manifest, requested, policy)` triple.
///
/// Pattern analog: `gap.rs:188-279` `GapDetector::detect` — pure function with
/// a numbered algorithm doc.
///
/// ## Algorithm
///
/// 1. If `manifest.gaps` is empty:
///    - Either policy → return `SubRanges(vec![requested as TimeRange])`
///      (single pass-through sub-range; the zero-gap fast path for both
///      strict — D3-12 — and `continuous_only` — D3-12).
/// 2. Else if `policy == Strict`:
///    - Return `Aborted(manifest.clone())` (D3-11).
/// 3. Else (`policy == ContinuousOnly`):
///    - Walk the (sorted, per Phase 2 `GapManifest` invariant) gaps and
///      compute maximal gap-free sub-ranges inside `requested`. Gaps
///      entirely outside `[requested.start, requested.end)` are ignored.
///      Gaps clamped at the boundaries split the requested range
///      accordingly. If the union of clamped gaps covers `requested`
///      entirely, return `SubRanges(vec![])` (no sub-ranges; the engine
///      emits zero `Result` findings).
///
/// `dispatch` NEVER silently produces a `SubRanges` element overlapping a
/// gap — the [`never_silently_emits_on_hole_proptest`] regression pins this.
#[must_use]
pub fn dispatch(
    manifest: &GapManifest,
    requested: ClosedRangeUtc,
    policy: GapPolicyKind,
) -> GapDispatch {
    // Step 1 — zero-gap fast path for both policies (D3-12).
    if manifest.gaps.is_empty() {
        return GapDispatch::SubRanges(vec![TimeRange {
            start_utc: requested.start,
            end_utc: requested.end,
        }]);
    }

    // Step 2 — strict + gaps present aborts (D3-11).
    if matches!(policy, GapPolicyKind::Strict) {
        return GapDispatch::Aborted(manifest.clone());
    }

    // Step 3 — continuous_only: sweep gaps and emit maximal gap-free sub-ranges.
    // Clamp each gap to [requested.start, requested.end) before consideration;
    // gaps entirely outside the requested range have no effect.
    let mut subs: Vec<TimeRange> = Vec::new();
    let mut cursor = requested.start;
    for gap in &manifest.gaps {
        // Skip gaps entirely outside [requested.start, requested.end).
        if gap.end_utc <= requested.start || gap.start_utc >= requested.end {
            continue;
        }
        // Clamp the gap to the requested boundary.
        let g_start = gap.start_utc.max(requested.start);
        let g_end = gap.end_utc.min(requested.end);
        if g_start > cursor {
            subs.push(TimeRange {
                start_utc: cursor,
                end_utc: g_start,
            });
        }
        if g_end > cursor {
            cursor = g_end;
        }
    }
    if cursor < requested.end {
        subs.push(TimeRange {
            start_utc: cursor,
            end_utc: requested.end,
        });
    }
    GapDispatch::SubRanges(subs)
}

// ---------------------------------------------------------------------------
// snap_subranges_to_timeframe — RAD-2351 partitioner/aggregator alignment fix.
// ---------------------------------------------------------------------------

/// Snap each sub-range to the requested timeframe's bucket boundary.
///
/// `start_utc` is rounded UP to the next bucket boundary; `end_utc` is rounded
/// DOWN to the previous bucket boundary. Sub-ranges that collapse to empty
/// (`snapped_start >= snapped_end`) are dropped. The relative order of the
/// surviving sub-ranges is preserved.
///
/// ## Why this exists (RAD-2351)
///
/// Under `--gap-policy continuous_only`, [`dispatch`] partitions the requested
/// window into maximal gap-free sub-ranges at the **gap detector's**
/// resolution — currently 1-minute. The aggregator
/// ([`crate::aggregator::aggregate`]) validates that `range.start` is aligned
/// to the target timeframe's bucket boundary (15m, 1h, 1d) and rejects
/// unaligned starts with [`crate::aggregator::AggregateError::MisalignedRange`].
///
/// On real data the *next* sub-range after a single-minute intra-day gap
/// starts at the minute immediately following the gap (e.g.
/// `2024-01-02 23:39:00 UTC` after a 23:38→23:39 hole) — which is NOT on a
/// 15-minute boundary. Before this snap the aggregator rejected every
/// post-gap sub-range with `cache: aggregator error: range.start ... is not
/// aligned to ... boundary`, dropping every continuous slice except the
/// first.
///
/// ## Trade-off
///
/// Bars in the partial-coverage bucket at each gap edge are dropped. This is
/// the correct semantics: a 15-minute bar cannot be computed from <15
/// minutes of coverage. Use `--timeframe 1m` to retain the partial slices.
///
/// ## Determinism
///
/// Pure function: no IO, no clock reads. Given the same `(subs, tf)` pair the
/// output is byte-stable across re-runs (CACHE-04).
#[must_use]
pub fn snap_subranges_to_timeframe(subs: Vec<TimeRange>, tf: Timeframe) -> Vec<TimeRange> {
    let mut out = Vec::with_capacity(subs.len());
    for sub in subs {
        let snapped_start = align_up(sub.start_utc, tf);
        let snapped_end = align_down(sub.end_utc, tf);
        if snapped_start < snapped_end {
            out.push(TimeRange {
                start_utc: snapped_start,
                end_utc: snapped_end,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// dispatch_pair — Phase 4 (Plan 04-02 / D4-04). Two-leg gap-policy dispatch.
// ---------------------------------------------------------------------------

/// Two-leg gap-policy dispatch — Phase 4 (Plan 04-02 / D4-04).
///
/// Computes the joint manifest via
/// [`crate::scan::primitives::time_alignment::intersect_gaps`] (the
/// UNION of per-leg gap intervals — the joint "do not run" set for CROSS
/// scans), then dispatches through the existing [`dispatch`] function on
/// the joint manifest. CROSS scans (registered by Plan 04-07) call this
/// helper from the engine's Pair branch.
///
/// Pattern analog: existing `dispatch` — same shape; the only delta is the
/// `intersect_gaps` step that precedes the dispatch decision.
///
/// `manifest_a` and `manifest_b` are the per-leg gap manifests produced by
/// `GapDetector::detect` for each leg of a CROSS request; `policy` and
/// `requested` are the same shared values the engine already carries.
#[must_use]
pub fn dispatch_pair(
    manifest_a: &GapManifest,
    manifest_b: &GapManifest,
    requested: ClosedRangeUtc,
    policy: GapPolicyKind,
) -> GapDispatch {
    let joint = crate::scan::primitives::time_alignment::intersect_gaps(manifest_a, manifest_b);
    dispatch(&joint, requested, policy)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::match_wildcard_for_single_variants, clippy::similar_names)]
mod tests {
    use super::*;
    use crate::findings::TimeRange;
    use crate::gap::{GapReason, GapSpan};
    use crate::reader::Side;
    use chrono::{DateTime, TimeZone, Utc};
    use proptest::prelude::*;

    fn t(h: u32) -> DateTime<Utc> {
        // Helper: an hour-of-day on 2024-01-01 UTC. Hour 24 maps to
        // 2024-01-02 00:00:00 UTC (chrono rejects literal hour 24); this
        // lets the proptest sweep range endpoints up to 24 inclusive.
        if h >= 24 {
            Utc.with_ymd_and_hms(2024, 1, 2, h - 24, 0, 0).unwrap()
        } else {
            Utc.with_ymd_and_hms(2024, 1, 1, h, 0, 0).unwrap()
        }
    }

    fn empty_manifest() -> GapManifest {
        GapManifest {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            queried_range: TimeRange {
                start_utc: t(0),
                end_utc: t(6),
            },
            gaps: Vec::new(),
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

    fn gap(start: DateTime<Utc>, end: DateTime<Utc>) -> GapSpan {
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

    // -----------------------------------------------------------------------
    // GapPolicyKind::from_str round-trip (Plan 03-05)
    // -----------------------------------------------------------------------

    #[test]
    fn gap_policy_kind_from_str_round_trip() {
        for k in [GapPolicyKind::Strict, GapPolicyKind::ContinuousOnly] {
            assert_eq!(GapPolicyKind::from_str(k.as_str()).unwrap(), k);
        }
    }

    #[test]
    fn gap_policy_kind_from_str_rejects_unknown() {
        let err = GapPolicyKind::from_str("lax").expect_err("must reject");
        assert_eq!(err, "lax");
    }

    // -----------------------------------------------------------------------
    // strict policy
    // -----------------------------------------------------------------------

    #[test]
    fn strict_with_gaps_aborts() {
        let m = manifest_with_gaps(vec![gap(t(2), t(3))]);
        let d = dispatch(&m, requested(t(0), t(6)), GapPolicyKind::Strict);
        match d {
            GapDispatch::Aborted(returned) => assert_eq!(returned, m),
            other => panic!("expected Aborted, got {other:?}"),
        }
    }

    #[test]
    fn strict_zero_gaps_passes_through() {
        let m = empty_manifest();
        let req = requested(t(0), t(6));
        let d = dispatch(&m, req, GapPolicyKind::Strict);
        match d {
            GapDispatch::SubRanges(v) => {
                assert_eq!(v.len(), 1);
                assert_eq!(
                    v[0],
                    TimeRange {
                        start_utc: req.start,
                        end_utc: req.end,
                    }
                );
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // continuous_only policy
    // -----------------------------------------------------------------------

    #[test]
    fn continuous_only_partitions_around_gaps() {
        // Requested [0, 6), one gap [2, 3) -> SubRanges([0, 2), [3, 6)).
        let m = manifest_with_gaps(vec![gap(t(2), t(3))]);
        let req = requested(t(0), t(6));
        let d = dispatch(&m, req, GapPolicyKind::ContinuousOnly);
        match d {
            GapDispatch::SubRanges(v) => {
                assert_eq!(v.len(), 2, "expected two sub-ranges around the gap");
                assert_eq!(
                    v[0],
                    TimeRange {
                        start_utc: t(0),
                        end_utc: t(2),
                    }
                );
                assert_eq!(
                    v[1],
                    TimeRange {
                        start_utc: t(3),
                        end_utc: t(6),
                    }
                );
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    #[test]
    fn continuous_only_zero_gaps_fast_path() {
        let m = empty_manifest();
        let req = requested(t(0), t(6));
        let d = dispatch(&m, req, GapPolicyKind::ContinuousOnly);
        match d {
            GapDispatch::SubRanges(v) => {
                assert_eq!(v.len(), 1);
                assert_eq!(
                    v[0],
                    TimeRange {
                        start_utc: req.start,
                        end_utc: req.end,
                    }
                );
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    #[test]
    fn continuous_only_gap_at_boundary() {
        // Gap at start [0, 1), requested [0, 6) -> SubRanges([1, 6)).
        let m = manifest_with_gaps(vec![gap(t(0), t(1))]);
        let req = requested(t(0), t(6));
        let d = dispatch(&m, req, GapPolicyKind::ContinuousOnly);
        match d {
            GapDispatch::SubRanges(v) => {
                assert_eq!(v.len(), 1, "expected one sub-range after the leading gap");
                assert_eq!(
                    v[0],
                    TimeRange {
                        start_utc: t(1),
                        end_utc: t(6),
                    }
                );
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    #[test]
    fn continuous_only_gap_consumes_whole_range() {
        // Gap covering [0, 6), requested [0, 6) -> SubRanges([]).
        let m = manifest_with_gaps(vec![gap(t(0), t(6))]);
        let req = requested(t(0), t(6));
        let d = dispatch(&m, req, GapPolicyKind::ContinuousOnly);
        match d {
            GapDispatch::SubRanges(v) => {
                assert!(
                    v.is_empty(),
                    "expected zero sub-ranges when gap covers the whole range; got {v:?}"
                );
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    #[test]
    fn continuous_only_multiple_gaps() {
        // Gaps [1,2), [3,4) inside [0, 6) -> SubRanges([0,1), [2,3), [4,6)).
        let m = manifest_with_gaps(vec![gap(t(1), t(2)), gap(t(3), t(4))]);
        let req = requested(t(0), t(6));
        let d = dispatch(&m, req, GapPolicyKind::ContinuousOnly);
        match d {
            GapDispatch::SubRanges(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(
                    v[0],
                    TimeRange {
                        start_utc: t(0),
                        end_utc: t(1),
                    }
                );
                assert_eq!(
                    v[1],
                    TimeRange {
                        start_utc: t(2),
                        end_utc: t(3),
                    }
                );
                assert_eq!(
                    v[2],
                    TimeRange {
                        start_utc: t(4),
                        end_utc: t(6),
                    }
                );
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    #[test]
    fn continuous_only_gap_outside_requested() {
        // Gap [10, 11) is fully after requested [0, 6) — should be ignored.
        let m = manifest_with_gaps(vec![gap(t(10), t(11))]);
        let req = requested(t(0), t(6));
        let d = dispatch(&m, req, GapPolicyKind::ContinuousOnly);
        match d {
            GapDispatch::SubRanges(v) => {
                assert_eq!(v.len(), 1, "out-of-range gap must be ignored");
                assert_eq!(
                    v[0],
                    TimeRange {
                        start_utc: t(0),
                        end_utc: t(6),
                    }
                );
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Proptest — under random sorted non-overlapping gaps, `dispatch` NEVER
    // returns a SubRanges element overlapping a gap. (Strict + non-empty
    // always returns Aborted; ContinuousOnly's union is a subset of
    // requested - union(gaps).)
    // -----------------------------------------------------------------------

    proptest! {
        #[test]
        fn never_silently_emits_on_hole_proptest(
            // Generate non-overlapping sorted gaps within [0, 24) hours and a
            // requested range also within [0, 24). Using hour offsets keeps
            // the values small and the proptest fast.
            offsets in proptest::collection::vec(0u32..24, 0..6),
            req_start_h in 0u32..23,
            req_end_offset in 1u32..24,
        ) {
            // Build gaps from sorted unique offsets, pairing consecutive
            // values as [g_start, g_end). This produces a valid sorted
            // non-overlapping gap sequence.
            let mut sorted: Vec<u32> = offsets;
            sorted.sort_unstable();
            sorted.dedup();
            // Make sure pairs of consecutive offsets form valid half-open
            // ranges; we discard the last unpaired element.
            if sorted.len() % 2 == 1 {
                sorted.pop();
            }
            let gap_spans: Vec<GapSpan> = sorted
                .chunks_exact(2)
                .filter(|c| c[0] < c[1])
                .map(|c| gap(t(c[0]), t(c[1])))
                .collect();
            let manifest = manifest_with_gaps(gap_spans.clone());
            let req_end_h = (req_start_h + req_end_offset).min(24);
            if req_end_h <= req_start_h {
                return Ok(());
            }
            let req = requested(t(req_start_h), t(req_end_h));

            // Strict + non-empty -> always Aborted.
            if !gap_spans.is_empty() {
                let d_strict = dispatch(&manifest, req, GapPolicyKind::Strict);
                prop_assert!(
                    matches!(d_strict, GapDispatch::Aborted(_)),
                    "Strict + non-empty gaps must abort"
                );
            }

            // ContinuousOnly -> sub-ranges never overlap a gap.
            let d_cont = dispatch(&manifest, req, GapPolicyKind::ContinuousOnly);
            if let GapDispatch::SubRanges(subs) = d_cont {
                for sub in &subs {
                    // sub.start < sub.end (well-formed half-open range).
                    prop_assert!(sub.start_utc < sub.end_utc, "subrange must be non-empty: {sub:?}");
                    // sub does NOT overlap any gap.
                    for g in &gap_spans {
                        let clamped_start = g.start_utc.max(req.start);
                        let clamped_end = g.end_utc.min(req.end);
                        if clamped_start >= clamped_end {
                            continue; // out of range
                        }
                        let overlaps = sub.start_utc < clamped_end && clamped_start < sub.end_utc;
                        prop_assert!(
                            !overlaps,
                            "subrange {sub:?} overlaps gap [{clamped_start}, {clamped_end})"
                        );
                    }
                    // sub is within requested range.
                    prop_assert!(sub.start_utc >= req.start);
                    prop_assert!(sub.end_utc <= req.end);
                }
            } else {
                panic!("ContinuousOnly must always return SubRanges");
            }
        }
    }

    // -----------------------------------------------------------------------
    // snap_subranges_to_timeframe (RAD-2351)
    // -----------------------------------------------------------------------

    fn ts(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 2, h, m, 0).unwrap()
    }

    fn sub(start: DateTime<Utc>, end: DateTime<Utc>) -> TimeRange {
        TimeRange {
            start_utc: start,
            end_utc: end,
        }
    }

    #[test]
    fn snap_15m_already_aligned_passthrough() {
        // [00:00, 23:45) on a 15m grid — both bounds already on boundary.
        let subs = vec![sub(ts(0, 0), ts(23, 45))];
        let out = snap_subranges_to_timeframe(subs.clone(), Timeframe::Tf15m);
        assert_eq!(out, subs);
    }

    #[test]
    fn snap_15m_rounds_start_up_after_gap() {
        // RAD-2351 repro shape: a post-gap sub-range starting at 23:39 (the
        // minute after a single-minute hole) must snap forward to the next
        // 15m bucket boundary at 23:45 so the aggregator's
        // validate_range_alignment guard accepts it.
        let out = snap_subranges_to_timeframe(
            vec![sub(
                ts(23, 39),
                Utc.with_ymd_and_hms(2024, 1, 3, 2, 0, 0).unwrap(),
            )],
            Timeframe::Tf15m,
        );
        assert_eq!(
            out,
            vec![sub(
                ts(23, 45),
                Utc.with_ymd_and_hms(2024, 1, 3, 2, 0, 0).unwrap(),
            )],
            "post-gap start must snap UP to next 15m boundary"
        );
    }

    #[test]
    fn snap_15m_rounds_end_down() {
        // [00:00, 23:53) → [00:00, 23:45) on 15m grid.
        let out = snap_subranges_to_timeframe(vec![sub(ts(0, 0), ts(23, 53))], Timeframe::Tf15m);
        assert_eq!(out, vec![sub(ts(0, 0), ts(23, 45))]);
    }

    #[test]
    fn snap_15m_drops_subrange_collapsing_to_empty() {
        // [23:50, 23:59) → snap_up(start) = 24:00 (next day 00:00),
        // snap_down(end) = 23:45 → start > end → DROP.
        let out = snap_subranges_to_timeframe(vec![sub(ts(23, 50), ts(23, 59))], Timeframe::Tf15m);
        assert!(out.is_empty(), "expected drop, got {out:?}");
    }

    #[test]
    fn snap_15m_drops_subrange_smaller_than_one_bucket() {
        // [10:07, 10:12) — both inside the same 15m bucket. snap_up(10:07) =
        // 10:15, snap_down(10:12) = 10:00 → DROP.
        let out = snap_subranges_to_timeframe(vec![sub(ts(10, 7), ts(10, 12))], Timeframe::Tf15m);
        assert!(out.is_empty(), "expected drop, got {out:?}");
    }

    #[test]
    fn snap_1h_rounds_around_gap() {
        // Post-gap shape from the issue: 21:45 → 22:00.
        let out = snap_subranges_to_timeframe(
            vec![
                sub(ts(0, 0), ts(21, 30)),   // pre-gap, snaps to [00:00, 21:00)
                sub(ts(21, 45), ts(23, 30)), // post-gap, snaps to [22:00, 23:00)
            ],
            Timeframe::Tf1h,
        );
        assert_eq!(
            out,
            vec![sub(ts(0, 0), ts(21, 0)), sub(ts(22, 0), ts(23, 0)),],
            "1h snap must round both bounds: pre-gap end down, post-gap start up"
        );
    }

    #[test]
    fn snap_preserves_relative_order() {
        // Three sub-ranges with varied (un)alignments; surviving outputs keep
        // their original order.
        let out = snap_subranges_to_timeframe(
            vec![
                sub(ts(0, 0), ts(0, 30)),  // already aligned → unchanged
                sub(ts(1, 7), ts(2, 53)),  // → [01:15, 02:45)
                sub(ts(3, 0), ts(3, 5)),   // collapses → dropped
                sub(ts(4, 30), ts(5, 30)), // already aligned → unchanged
            ],
            Timeframe::Tf15m,
        );
        assert_eq!(
            out,
            vec![
                sub(ts(0, 0), ts(0, 30)),
                sub(ts(1, 15), ts(2, 45)),
                sub(ts(4, 30), ts(5, 30)),
            ]
        );
    }

    #[test]
    fn snap_empty_input_returns_empty() {
        let out = snap_subranges_to_timeframe(Vec::new(), Timeframe::Tf15m);
        assert!(out.is_empty());
    }

    proptest! {
        #[test]
        fn snap_output_always_aligned_or_dropped(
            // Two minute offsets within a day, lower first; timeframe enum index.
            a in 0u32..1440,
            len in 1u32..1440,
            tf_idx in 0u8..3,
        ) {
            let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
                + chrono::Duration::minutes(a as i64);
            let end = start + chrono::Duration::minutes(len as i64);
            let tf = match tf_idx {
                0 => Timeframe::Tf15m,
                1 => Timeframe::Tf1h,
                _ => Timeframe::Tf1d,
            };
            let out = snap_subranges_to_timeframe(
                vec![TimeRange { start_utc: start, end_utc: end }],
                tf,
            );
            for tr in &out {
                // Both bounds must be on the timeframe grid.
                let dur = tf.duration().num_minutes();
                let mins_from_epoch = tr.start_utc.timestamp() / 60;
                prop_assert_eq!(mins_from_epoch % dur, 0, "snapped start not aligned: {:?}", tr);
                let mins_end = tr.end_utc.timestamp() / 60;
                prop_assert_eq!(mins_end % dur, 0, "snapped end not aligned: {:?}", tr);
                // Output sub-range must be non-empty.
                prop_assert!(tr.start_utc < tr.end_utc, "snapped sub-range must be non-empty");
                // Snapped sub-range is contained in the input range.
                prop_assert!(tr.start_utc >= start);
                prop_assert!(tr.end_utc <= end);
            }
        }
    }
}

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
use crate::gap::{GapManifest, GapReason, GapSpan};
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
// effective_manifest_for_timeframe — RAD-2642 timeframe-aware gap filter.
// ---------------------------------------------------------------------------

/// Project a 1-minute-resolution [`GapManifest`] onto the requested
/// aggregation timeframe.
///
/// The returned manifest contains only the gaps that are **visible at the
/// requested timeframe** — i.e. the gaps that leave at least one fully
/// uncovered bucket at `tf`. The original manifest is preserved in the
/// caller's hand for use in `Finding::Result.data_slice.gap_manifest`
/// (and in `Finding::GapAborted`), so data-quality information is not lost;
/// only **dispatch decisions** are filtered.
///
/// ## Why this exists (RAD-2642)
///
/// `GapDetector::detect` emits one `IntraDayGap { affected_minutes: 1 }`
/// entry per missing open-hour minute (`gap.rs:253-262`). On real
/// Dukascopy data this produces ~5-15 entries per FX trading day from
/// the low-liquidity 22:00-04:00 UTC window. Pre-RAD-2642, every one of
/// those entries split the requested window — so a 4-year `--timeframe 1d`
/// scan was shredded into a few thousand sub-ranges, then
/// `snap_subranges_to_timeframe` (RAD-2351) dropped every one shorter than
/// one bucket, yielding ~zero useful results.
///
/// Aggregation at `tf` is well-defined whenever **at least one 1-minute
/// bar** falls inside the bucket — OHLC are first/max/min/last over the
/// surviving ticks. So the user-facing notion of a "gap at `tf`" is "a
/// `tf`-bucket whose 1-minute coverage is zero." This function computes
/// that set.
///
/// ## Algorithm
///
/// 1. **Whole-day gaps are unconditionally retained.** A
///    `MissingSourceFile` / `CorruptSourceFile` span covers
///    `[day 00:00, day+1 00:00)` — at least 24 hours — which already
///    covers ≥1 bucket at every supported timeframe (15m / 1h / 1d).
/// 2. **Intra-day gaps are coalesced and snapped.** Contiguous runs of
///    `IntraDayGap` entries (sorted by `start_utc`) are merged into a
///    single half-open interval `[run_start, run_end)`. For each run
///    the fully-covered `tf`-bucket span is computed as
///    `[align_up(run_start, tf), align_down(run_end, tf))`. When this
///    span is non-empty it is retained as a single `IntraDayGap` whose
///    `affected_minutes` equals the span length in minutes. When it is
///    empty the run is **dropped** — it touched no full bucket at `tf`,
///    so the aggregator will silently absorb it.
///
/// ## Properties
///
/// - Pure function: no IO, no clock reads, deterministic.
/// - Whole-day reasons are preserved verbatim, so the original
///   `MissingSourceFile { date }` / `CorruptSourceFile { date, detail }`
///   payload survives.
/// - Output `gaps` are sorted by `start_utc` ascending — the same
///   invariant `GapDetector::detect` guarantees.
/// - For `tf = Tf1m` (not currently a supported wire form, but the
///   function is defined for completeness) the projection is the
///   identity on `IntraDayGap` minutes (`align_up`/`align_down` are
///   identity on minute timestamps; runs collapse back to per-minute
///   spans — semantically equivalent to the original manifest from
///   the dispatch perspective).
#[must_use]
pub fn effective_manifest_for_timeframe(manifest: &GapManifest, tf: Timeframe) -> GapManifest {
    let mut whole_day: Vec<GapSpan> = Vec::new();
    let mut intraday: Vec<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> =
        Vec::new();

    for span in &manifest.gaps {
        match span.reason {
            GapReason::MissingSourceFile { .. } | GapReason::CorruptSourceFile { .. } => {
                whole_day.push(span.clone());
            }
            GapReason::IntraDayGap { .. } => {
                intraday.push((span.start_utc, span.end_utc));
            }
        }
    }

    // Coalesce contiguous (and adjacent) intra-day spans into half-open runs.
    intraday.sort_by_key(|(s, _)| *s);
    let mut coalesced: Vec<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> =
        Vec::with_capacity(intraday.len());
    for (s, e) in intraday {
        match coalesced.last_mut() {
            Some(prev) if s <= prev.1 => {
                if e > prev.1 {
                    prev.1 = e;
                }
            }
            _ => coalesced.push((s, e)),
        }
    }

    // For each coalesced run, retain only the fully-covered tf-bucket span.
    let mut effective_intraday: Vec<GapSpan> = Vec::with_capacity(coalesced.len());
    for (run_start, run_end) in coalesced {
        let bucket_start = align_up(run_start, tf);
        let bucket_end = align_down(run_end, tf);
        if bucket_start >= bucket_end {
            // Run does not fully cover any tf-bucket — the aggregator will
            // absorb it. Drop.
            continue;
        }
        let affected_minutes = (bucket_end - bucket_start).num_minutes();
        let affected_minutes = u32::try_from(affected_minutes).unwrap_or(u32::MAX);
        effective_intraday.push(GapSpan {
            start_utc: bucket_start,
            end_utc: bucket_end,
            reason: GapReason::IntraDayGap { affected_minutes },
        });
    }

    let mut gaps = whole_day;
    gaps.extend(effective_intraday);
    gaps.sort_by(|a, b| {
        a.start_utc
            .cmp(&b.start_utc)
            .then_with(|| a.end_utc.cmp(&b.end_utc))
            .then_with(|| {
                a.reason
                    .discriminant_ord()
                    .cmp(&b.reason.discriminant_ord())
            })
    });

    GapManifest {
        source_id: manifest.source_id.clone(),
        symbol: manifest.symbol.clone(),
        side: manifest.side,
        queried_range: manifest.queried_range.clone(),
        gaps,
    }
}

// ---------------------------------------------------------------------------
// dispatch_at_timeframe — RAD-2642 single-leg timeframe-aware dispatch.
// ---------------------------------------------------------------------------

/// Timeframe-aware single-leg dispatch — projects the manifest through
/// [`effective_manifest_for_timeframe`] before delegating to [`dispatch`].
///
/// New engine call-sites should prefer this entry point over the raw
/// `dispatch` so that `continuous_only` (and `strict`) reflect the user's
/// natural notion of "gap at the requested timeframe" instead of the
/// 1-minute gap-detector resolution. The raw `dispatch` remains the pure
/// primitive that the unit tests pin.
#[must_use]
pub fn dispatch_at_timeframe(
    manifest: &GapManifest,
    requested: ClosedRangeUtc,
    policy: GapPolicyKind,
    tf: Timeframe,
) -> GapDispatch {
    let effective = effective_manifest_for_timeframe(manifest, tf);
    dispatch(&effective, requested, policy)
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
// dispatch_pair_at_timeframe — RAD-2642 two-leg timeframe-aware dispatch.
// ---------------------------------------------------------------------------

/// Timeframe-aware two-leg dispatch — mirror of [`dispatch_at_timeframe`]
/// for CROSS scans (Plan 04-07). Computes the joint manifest via
/// `intersect_gaps`, projects it through [`effective_manifest_for_timeframe`],
/// then delegates to [`dispatch`].
#[must_use]
pub fn dispatch_pair_at_timeframe(
    manifest_a: &GapManifest,
    manifest_b: &GapManifest,
    requested: ClosedRangeUtc,
    policy: GapPolicyKind,
    tf: Timeframe,
) -> GapDispatch {
    let joint = crate::scan::primitives::time_alignment::intersect_gaps(manifest_a, manifest_b);
    let effective = effective_manifest_for_timeframe(&joint, tf);
    dispatch(&effective, requested, policy)
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
    use chrono::{DateTime, Duration, TimeZone, Utc};
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

    // -----------------------------------------------------------------------
    // effective_manifest_for_timeframe + dispatch_at_timeframe (RAD-2642)
    // -----------------------------------------------------------------------

    /// One-minute intra-day hole at 12:00 with `--timeframe 1d` is invisible
    /// at the requested resolution — the day's OHLC is computable from the
    /// remaining 1439 ticks. Effective manifest drops the entry.
    #[test]
    fn effective_manifest_drops_single_minute_intra_day_gap_at_1d() {
        let hole = ts(12, 0);
        let m = manifest_with_gaps(vec![gap(hole, hole + Duration::minutes(1))]);
        let eff = effective_manifest_for_timeframe(&m, Timeframe::Tf1d);
        assert!(
            eff.gaps.is_empty(),
            "1-min intra-day hole must not survive projection to 1d, got: {:?}",
            eff.gaps
        );
        // Original manifest must be untouched (caller still needs it for envelopes).
        assert_eq!(m.gaps.len(), 1, "original manifest must not be mutated");
    }

    /// 16-minute contiguous run at `[10:00, 10:16)` fully covers exactly one
    /// 15m bucket at `[10:00, 10:15)` — that bucket has zero source bars.
    /// Effective manifest retains the bucket.
    #[test]
    fn effective_manifest_retains_full_bucket_run_at_15m() {
        let runs: Vec<GapSpan> = (0..16)
            .map(|i| {
                let s = ts(10, 0) + Duration::minutes(i);
                gap(s, s + Duration::minutes(1))
            })
            .collect();
        let m = manifest_with_gaps(runs);
        let eff = effective_manifest_for_timeframe(&m, Timeframe::Tf15m);
        assert_eq!(eff.gaps.len(), 1, "expected one effective bucket-gap, got {:?}", eff.gaps);
        assert_eq!(eff.gaps[0].start_utc, ts(10, 0));
        assert_eq!(eff.gaps[0].end_utc, ts(10, 15));
        match eff.gaps[0].reason {
            GapReason::IntraDayGap { affected_minutes } => {
                assert_eq!(affected_minutes, 15);
            }
            ref other => panic!("expected IntraDayGap, got {other:?}"),
        }
    }

    /// Missing-source-file / corrupt-source-file spans are unconditionally
    /// retained — they cover ≥ 1 full day, which is ≥ 1 bucket at any
    /// supported timeframe (15m / 1h / 1d).
    #[test]
    fn effective_manifest_preserves_whole_day_reasons_at_all_timeframes() {
        let date = chrono::NaiveDate::from_ymd_opt(2024, 1, 3).unwrap();
        let day_start = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
        let missing = GapSpan {
            start_utc: day_start,
            end_utc: day_start + Duration::hours(24),
            reason: GapReason::MissingSourceFile { date },
        };
        let corrupt = GapSpan {
            start_utc: day_start + Duration::hours(24),
            end_utc: day_start + Duration::hours(48),
            reason: GapReason::CorruptSourceFile {
                date: chrono::NaiveDate::from_ymd_opt(2024, 1, 4).unwrap(),
                detail: "synthetic".into(),
            },
        };
        let m = manifest_with_gaps(vec![missing.clone(), corrupt.clone()]);
        for tf in [Timeframe::Tf15m, Timeframe::Tf1h, Timeframe::Tf1d] {
            let eff = effective_manifest_for_timeframe(&m, tf);
            assert_eq!(eff.gaps.len(), 2, "whole-day reasons must survive at tf={tf:?}, got {:?}", eff.gaps);
            assert_eq!(eff.gaps[0], missing, "MissingSourceFile dropped at tf={tf:?}");
            assert_eq!(eff.gaps[1], corrupt, "CorruptSourceFile dropped at tf={tf:?}");
        }
    }

    /// RAD-2642 repro shape: dozens of scattered 1-min holes across a
    /// trading week, dispatched at `--timeframe 1d`, must produce ONE
    /// pass-through sub-range covering the whole requested window.
    #[test]
    fn dispatch_at_timeframe_collapses_scattered_1min_holes_at_1d() {
        // Make 100 random-looking 1-min holes scattered through Jan 2-31.
        let mut spans = Vec::with_capacity(100);
        let anchor = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
        for i in 0..100 {
            // Stagger each hole by ~7 hours so they are not contiguous.
            let offset_min = (i as i64) * 7 * 60 + (i as i64) * 13;
            let s = anchor + Duration::minutes(offset_min);
            spans.push(gap(s, s + Duration::minutes(1)));
        }
        let m = manifest_with_gaps(spans);
        let req = requested(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap(),
        );
        let d =
            dispatch_at_timeframe(&m, req, GapPolicyKind::ContinuousOnly, Timeframe::Tf1d);
        match d {
            GapDispatch::SubRanges(v) => {
                assert_eq!(v.len(), 1, "all scattered 1-min holes must collapse at 1d, got {v:?}");
                assert_eq!(v[0].start_utc, req.start);
                assert_eq!(v[0].end_utc, req.end);
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    /// `strict + zero effective gaps` must NOT abort, even when the raw
    /// manifest has noise that the requested timeframe absorbs. Avoids
    /// the historical "strict aborts on every multi-day window" trap.
    #[test]
    fn dispatch_at_timeframe_strict_passes_when_only_subbar_holes_present() {
        // A run of 5 consecutive 1-min holes — does not cover a 15m bucket.
        let runs: Vec<GapSpan> = (0..5)
            .map(|i| {
                let s = ts(10, 0) + Duration::minutes(i);
                gap(s, s + Duration::minutes(1))
            })
            .collect();
        let m = manifest_with_gaps(runs);
        let req = requested(t(0), t(6));
        let d = dispatch_at_timeframe(&m, req, GapPolicyKind::Strict, Timeframe::Tf15m);
        match d {
            GapDispatch::SubRanges(v) => {
                assert_eq!(v.len(), 1, "strict must not abort on sub-bar holes; got {v:?}");
                assert_eq!(v[0].start_utc, req.start);
                assert_eq!(v[0].end_utc, req.end);
            }
            other => panic!("expected SubRanges, got {other:?}"),
        }
    }

    /// A run that crosses a bucket boundary but does not fully cover any
    /// bucket is dropped. Run `[10:13, 10:18)` covers minutes 13,14 of
    /// `[10:00, 10:15)` and minutes 15,16,17 of `[10:15, 10:30)` — neither
    /// bucket is fully missing, so the aggregator absorbs the 1m holes.
    #[test]
    fn effective_manifest_drops_run_that_partially_covers_two_buckets() {
        let runs: Vec<GapSpan> = (13..18)
            .map(|i| {
                let s = ts(10, 0) + Duration::minutes(i);
                gap(s, s + Duration::minutes(1))
            })
            .collect();
        let m = manifest_with_gaps(runs);
        let eff = effective_manifest_for_timeframe(&m, Timeframe::Tf15m);
        assert!(
            eff.gaps.is_empty(),
            "partial-coverage run must not produce an effective bucket-gap, got: {:?}",
            eff.gaps
        );
    }

    /// 31-minute contiguous run starting at `[10:00, 10:31)` fully covers
    /// bucket `[10:00, 10:15)` AND bucket `[10:15, 10:30)` (minute 30 spills
    /// into the next bucket but doesn't complete it). Effective manifest
    /// must emit one merged 30-minute span.
    #[test]
    fn effective_manifest_merges_adjacent_full_buckets_at_15m() {
        let runs: Vec<GapSpan> = (0..31)
            .map(|i| {
                let s = ts(10, 0) + Duration::minutes(i);
                gap(s, s + Duration::minutes(1))
            })
            .collect();
        let m = manifest_with_gaps(runs);
        let eff = effective_manifest_for_timeframe(&m, Timeframe::Tf15m);
        assert_eq!(eff.gaps.len(), 1, "expected single merged bucket-gap, got {:?}", eff.gaps);
        assert_eq!(eff.gaps[0].start_utc, ts(10, 0));
        assert_eq!(eff.gaps[0].end_utc, ts(10, 30));
    }

    proptest! {
        /// Effective manifest is a subset of the original at the time-axis
        /// level: every effective intra-day span is contained inside the
        /// union of the original intra-day spans. Whole-day spans are
        /// preserved verbatim. (Strong invariant: the projection can only
        /// drop or shrink intra-day spans, never grow them.)
        #[test]
        fn effective_manifest_is_subset_of_original_proptest(
            offsets in proptest::collection::vec(0u32..1440, 0..32),
            tf_idx in 0u8..3,
        ) {
            let tf = match tf_idx {
                0 => Timeframe::Tf15m,
                1 => Timeframe::Tf1h,
                _ => Timeframe::Tf1d,
            };
            let anchor = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
            let mut sorted: Vec<u32> = offsets;
            sorted.sort_unstable();
            sorted.dedup();
            let spans: Vec<GapSpan> = sorted
                .iter()
                .map(|&m_off| {
                    let s = anchor + Duration::minutes(m_off as i64);
                    gap(s, s + Duration::minutes(1))
                })
                .collect();
            let manifest = manifest_with_gaps(spans.clone());
            let eff = effective_manifest_for_timeframe(&manifest, tf);

            // Build the original intra-day union as a flat minute set.
            let mut original_min: std::collections::BTreeSet<DateTime<Utc>> =
                std::collections::BTreeSet::new();
            for s in &spans {
                let mut cur = s.start_utc;
                while cur < s.end_utc {
                    original_min.insert(cur);
                    cur += Duration::minutes(1);
                }
            }

            for g in &eff.gaps {
                // Every effective intra-day minute must have been in the original union.
                if matches!(g.reason, GapReason::IntraDayGap { .. }) {
                    let mut cur = g.start_utc;
                    while cur < g.end_utc {
                        prop_assert!(
                            original_min.contains(&cur),
                            "effective minute {cur} not in original intra-day union"
                        );
                        cur += Duration::minutes(1);
                    }
                    // Effective intra-day spans are tf-aligned.
                    let dur = tf.duration().num_minutes();
                    prop_assert_eq!(g.start_utc.timestamp() / 60 % dur, 0);
                    prop_assert_eq!(g.end_utc.timestamp() / 60 % dur, 0);
                }
            }
        }
    }
}

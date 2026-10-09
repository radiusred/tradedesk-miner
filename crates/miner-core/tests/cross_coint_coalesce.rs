//! RAD-2397 regression — `cross.cointegration.engle_granger` engine path
//! must coalesce per-sub-range frames into ONE kernel call so the
//! whole-sample `MIN_ALIGNED_N = 30` check evaluates the post-join,
//! gap-removed series length rather than each per-sub-range slice.
//!
//! Pre-fix (post-RAD-2352 / post-RAD-2642 baseline): the partitioner
//! correctly snaps every full-Tf1h-bucket hole to the timeframe boundary
//! and hands the Engle-Granger kernel a sequence of short contiguous
//! sub-ranges. Each per-sub-range call short-circuits with
//! `Engle-Granger needs >= 30 aligned bars; got N`, producing zero
//! `Finding::Result` envelopes for a window that has 40+ post-gap aligned
//! bars in total.
//!
//! Post-fix: `EngleGrangerScan::coalesce_subranges() == true` triggers the
//! engine's Pair-arity coalesce branch in `dispatch_pair_arity_body`. All
//! loaded sub-range frames fuse into one (`leg_a`, `leg_b`) frame pair and
//! one `scan.run` call. The kernel sees the full coalesced series, the
//! min-sample check passes, and exactly one `Finding::Result` envelope
//! lands in the sink.

#![allow(clippy::too_many_lines)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{NaiveDate, TimeZone, Utc};

use miner_core::aggregator::Timeframe;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{RunOutcome, param_hash, run_one_with_registry};
use miner_core::findings::{Finding, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::cross::EngleGrangerScan;
use miner_core::scan::{Registry, ScanRequest};
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, parse_findings, synthetic_cache::SyntheticCache};

/// Four full-Tf1h-bucket holes per leg over a 2-day window. The Tf1h-projected
/// joint manifest partitions the 48-hour window into FIVE short sub-ranges,
/// every one well below the kernel's `MIN_ALIGNED_N = 30` threshold.
/// Coalesced length: 43 aligned bars after inner-join, gap-removal, and
/// timeframe-bucket snapping.
fn day_1() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 6, 12).expect("valid calendar date")
}
fn day_2() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 6, 13).expect("valid calendar date")
}

/// Holes at hours 5 and 17 (one full Tf1h bucket each) on both days.
const HOLE_RANGES: &[std::ops::Range<i64>] = &[300..360, 1020..1080];

fn build_two_day_holed_cache(seed_a: u32, seed_b: u32) -> SyntheticCache {
    SyntheticCache::new()
        .with_day_multi_holed("EURUSD", Side::Bid, day_1(), seed_a, HOLE_RANGES)
        .with_day_multi_holed("EURUSD", Side::Bid, day_2(), seed_a, HOLE_RANGES)
        .with_day_multi_holed("GBPUSD", Side::Bid, day_1(), seed_b, HOLE_RANGES)
        .with_day_multi_holed("GBPUSD", Side::Bid, day_2(), seed_b, HOLE_RANGES)
}

fn engle_granger_request(window: ClosedRangeUtc) -> ScanRequest {
    let resolved = serde_json::json!({});
    let param_hash = param_hash::param_hash(&resolved).expect("param_hash ok");
    ScanRequest {
        scan_id: "cross.cointegration.engle_granger".into(),
        version: 1,
        instruments: vec![
            InstrumentSpec {
                symbol: "EURUSD".into(),
                side: Side::Bid,
            },
            InstrumentSpec {
                symbol: "GBPUSD".into(),
                side: Side::Bid,
            },
        ],
        timeframe: Timeframe::Tf1h,
        window,
        sub_range: TimeRange {
            start_utc: window.start,
            end_utc: window.end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params: resolved,
        param_hash,
        dry_run: false,
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    }
}

fn build_cfg(cache: &SyntheticCache) -> MinerConfig {
    MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    }
}

/// RAD-2397 acceptance gate (1/1): drive the Pair-arity engine path against
/// a synthetic 2-day cache whose Tf1h-projected joint manifest forces a
/// 5-way partition where every sub-range is < `MIN_ALIGNED_N`. Assert:
///
/// 1. `RunOutcome::Ok` — engine produced no fatal failures.
/// 2. Exactly ONE `Finding::Result` envelope — the coalesce branch fused
///    the per-sub-range frames into one kernel call.
/// 3. Zero `Finding::ScanError` envelopes whose message contains
///    `"aligned bars"` — the per-sub-range short-circuit message MUST
///    NOT appear post-fix.
/// 4. The Result envelope's `effect.n` reflects the coalesced sample
///    count (44 aligned bars).
/// 5. `data_slice.gap_manifest` is inlined (ContinuousOnly contract) and
///    enumerates the four 60-minute holes per leg the partitioner cut
///    out.
#[test]
#[serial_test::serial]
fn engle_granger_coalesces_subranges_across_intra_day_holes() {
    let cache = build_two_day_holed_cache(0xCAFE_F00D, 0xBADC_0FFE);
    let cfg = build_cfg(&cache);
    let reader = DukascopyReader::new(cache.cache_root());

    let start = Utc.with_ymd_and_hms(2024, 6, 12, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 6, 14, 0, 0, 0).unwrap();
    let req = engle_granger_request(ClosedRangeUtc { start, end });

    let mut registry = Registry::new();
    registry.register(Box::new(EngleGrangerScan));

    let mut sink = BufferSink::new();
    let outcome = run_one_with_registry(
        &req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
        &registry,
    )
    .expect("run_one_with_registry ok");

    assert_eq!(
        outcome,
        RunOutcome::Ok,
        "expected RunOutcome::Ok — got {outcome:?}; stream:\n{}",
        sink.as_str()
    );

    let findings = parse_findings(&sink.0);

    // Assertion 3 — no per-sub-range min-sample short-circuits.
    let aligned_bar_errors: Vec<&Finding> = findings
        .iter()
        .filter(|f| {
            matches!(
                f,
                Finding::ScanError(se) if se.message.contains("aligned bars")
            )
        })
        .collect();
    assert!(
        aligned_bar_errors.is_empty(),
        "post-fix: NO `aligned bars` ScanError envelopes; got {} — stream:\n{}",
        aligned_bar_errors.len(),
        sink.as_str()
    );

    // Assertion 2 — exactly one Result envelope from the coalesced run.
    let results: Vec<&Finding> = findings
        .iter()
        .filter(|f| matches!(f, Finding::Result(_)))
        .collect();
    assert_eq!(
        results.len(),
        1,
        "expected exactly one coalesced Result envelope; got {} — stream:\n{}",
        results.len(),
        sink.as_str()
    );

    let Finding::Result(r) = results[0] else {
        unreachable!("filtered to Result above")
    };

    // Assertion 4 — effect.n reflects the coalesced sample, well above the
    // kernel's MIN_ALIGNED_N=30 threshold. Observed value is 43 after
    // inner-join, gap-removal, and the aggregator's bucket-snap dropping
    // the trailing day-3 boundary bar from the 48-hour window.
    assert_eq!(
        r.effect.n,
        Some(43),
        "coalesced effect.n must equal post-join, gap-removed length; got {:?}",
        r.effect.n
    );

    // Assertion 1 sanity — scan id pinned for future renames.
    assert_eq!(r.scan_id_at_version, "cross.cointegration.engle_granger@1");

    // Assertion 5 — the inlined manifest is the JOINT manifest produced
    // by `intersect_gaps`, which collapses each leg's per-minute spans into
    // one contiguous span per hole (touching spans merge). 2 days × 2
    // 60-minute holes per day = 4 merged spans.
    let m = r
        .data_slice
        .gap_manifest
        .as_ref()
        .expect("ContinuousOnly inlines gap_manifest in data_slice");
    assert_eq!(
        m.gaps.len(),
        4,
        "expected 4 merged joint-manifest gap spans (one per 60-minute hole); got {} ({:?})",
        m.gaps.len(),
        m.gaps,
    );

    // Sanity: data_slice.range spans the union of the dispatched sub-ranges
    // (first sub-range start .. last sub-range end), not a single per-
    // sub-range slice.
    let range_minutes = (r.data_slice.range.end_utc - r.data_slice.range.start_utc).num_minutes();
    assert!(
        range_minutes >= 24 * 60,
        "coalesced data_slice.range must span >= 24h; got {range_minutes} minutes"
    );
}

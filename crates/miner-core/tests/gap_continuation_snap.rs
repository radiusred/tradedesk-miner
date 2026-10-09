//! RAD-2352 + RAD-2642 regression — gap-continuation handling end-to-end.
//!
//! Drives [`engine::run_one`] against a synthetic Dukascopy cache that
//! contains a hole during open hours, with `--gap-policy continuous_only`
//! and a target timeframe, then asserts the engine emits well-formed
//! `Result` envelopes around the hole with zero `ScanError` envelopes.
//!
//! Two regressions converge here:
//!
//! 1. **RAD-2352 (gap-continuation alignment)**: the pre-fix engine handed
//!    the aggregator a post-gap `range.start` that was not aligned to the
//!    requested timeframe. After the fix in
//!    `gap_policy::snap_subranges_to_timeframe`, the partitioner snaps
//!    sub-range bounds onto the timeframe grid before dispatching.
//! 2. **RAD-2642 (timeframe-aware gap projection)**: the pre-fix engine
//!    partitioned around *every* intra-minute hole — so a single missing
//!    minute mid-bucket shredded a multi-week window. After the fix in
//!    `gap_policy::effective_manifest_for_timeframe`, the engine projects
//!    the manifest onto the requested timeframe before dispatching, so
//!    sub-bucket holes (one missing minute inside a 15m bucket) are
//!    invisible to partitioning. To exercise the RAD-2352 partition path
//!    end-to-end we therefore use a **full-bucket hole** here — 15
//!    consecutive missing minutes at a Tf15m boundary (and 60 consecutive
//!    missing minutes at a Tf1h boundary).

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{NaiveDate, TimeZone, Utc};

use miner_core::aggregator::Timeframe;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{RunOutcome, param_hash, run_one};
use miner_core::findings::{Finding, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::ScanRequest;
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, parse_findings, synthetic_cache::SyntheticCache};

/// 2024-06-12 is a Wednesday — fully open under the FX-major calendar so the
/// gap detector classifies the missing minute as an `IntraDayGap`.
fn open_day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 6, 12).expect("valid calendar date")
}

fn day_window(date: NaiveDate) -> ClosedRangeUtc {
    let start = date.and_hms_opt(0, 0, 0).expect("00:00:00").and_utc();
    let end = start + chrono::Duration::hours(24);
    ClosedRangeUtc { start, end }
}

fn ljung_box_request(window: ClosedRangeUtc, tf: Timeframe) -> ScanRequest {
    let resolved = serde_json::json!({"lags": 5});
    let param_hash = param_hash::param_hash(&resolved).expect("param_hash ok");
    ScanRequest {
        scan_id: "stats.autocorr.ljung_box".into(),
        version: 1,
        instruments: vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }],
        timeframe: tf,
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

/// Run the engine against a 1-day synthetic Dukascopy fixture with a
/// 15-minute hole exactly covering one Tf15m bucket (`[12:00, 12:15)`) and
/// assert: exit OK, two `Result` envelopes (one per side of the bucket),
/// zero `ScanError` envelopes, all 15 `IntraDayGap` minutes preserved in
/// the inlined raw manifest.
///
/// Pre-RAD-2352 the post-gap sub-range started at `12:15:00`, but the
/// aggregator alignment guard rejected anything off the timeframe grid.
/// The `snap_subranges_to_timeframe` fix rounded the post-gap start up to
/// the next 15m boundary, eliminating the ScanError. RAD-2642 leaves that
/// snap path in place but only invokes it when the (now timeframe-aware)
/// effective manifest produces a non-empty partition — full-bucket holes
/// still partition; sub-bucket holes are absorbed.
#[test]
#[serial_test::serial]
fn run_one_emits_results_across_intra_minute_15m_gap() {
    let day = open_day();
    // Full-bucket hole: minutes 720..735 = [12:00, 12:15) — exactly one
    // 15m bucket. Under RAD-2642 this is the smallest hole that still
    // forces partition at Tf15m.
    let cache =
        SyntheticCache::new().with_day_holed("EURUSD", Side::Bid, day, 0xCAFE_F00D, 720..735);
    let cfg = build_cfg(&cache);
    let reader = DukascopyReader::new(cache.cache_root());

    let req = ljung_box_request(day_window(day), Timeframe::Tf15m);

    let mut sink = BufferSink::new();
    let outcome = run_one(
        &req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("run_one ok");

    assert_eq!(
        outcome,
        RunOutcome::Ok,
        "expected RunOutcome::Ok — got {outcome:?}; stdout: {}",
        sink.as_str()
    );

    let findings = parse_findings(&sink.0);

    let scan_errors: Vec<&Finding> = findings
        .iter()
        .filter(|f| matches!(f, Finding::ScanError(_)))
        .collect();
    assert!(
        scan_errors.is_empty(),
        "expected zero ScanError envelopes; got {} — full stream:\n{}",
        scan_errors.len(),
        sink.as_str()
    );

    let results: Vec<&Finding> = findings
        .iter()
        .filter(|f| matches!(f, Finding::Result(_)))
        .collect();
    assert!(
        results.len() >= 2,
        "expected at least two Result envelopes (one per side of the gap), \
         got {}; full stream:\n{}",
        results.len(),
        sink.as_str()
    );

    // Each Result must carry the inlined gap manifest with all 15
    // `IntraDayGap` minutes. The inlined manifest is the FULL raw
    // queried-window manifest (not the per-sub-range or timeframe-projected
    // view) per the engine's `ctx_gap_manifest` rule — data-quality info
    // must round-trip even when dispatch absorbs sub-bucket holes.
    for f in &results {
        let Finding::Result(r) = f else {
            unreachable!("filtered to Result above")
        };
        let m = r
            .data_slice
            .gap_manifest
            .as_ref()
            .expect("ContinuousOnly inlines a manifest in data_slice.gap_manifest");
        assert_eq!(
            m.gaps.len(),
            15,
            "expected 15 IntraDayGap minutes in inlined manifest, got {} ({:?})",
            m.gaps.len(),
            m.gaps,
        );
        // First and last gap minutes pin the window.
        let first = &m.gaps[0];
        let last = &m.gaps[m.gaps.len() - 1];
        assert_eq!(
            first.start_utc,
            Utc.with_ymd_and_hms(2024, 6, 12, 12, 0, 0).unwrap()
        );
        assert_eq!(
            last.end_utc,
            Utc.with_ymd_and_hms(2024, 6, 12, 12, 15, 0).unwrap()
        );
    }
}

/// Same shape as [`run_one_emits_results_across_intra_minute_15m_gap`] but
/// drives a 1-hour timeframe across a hole exactly covering one Tf1h
/// bucket (`[12:00, 13:00)`). Mirrors step-6 of the v1.0 smoke
/// (cross.cointegration, Tf1h) at the single-leg layer.
#[test]
#[serial_test::serial]
fn run_one_emits_results_across_intra_minute_1h_gap() {
    let day = open_day();
    // Full Tf1h bucket: minutes 720..780 = [12:00, 13:00).
    let cache =
        SyntheticCache::new().with_day_holed("EURUSD", Side::Bid, day, 0xBADC_0FFE, 720..780);
    let cfg = build_cfg(&cache);
    let reader = DukascopyReader::new(cache.cache_root());

    let req = ljung_box_request(day_window(day), Timeframe::Tf1h);

    let mut sink = BufferSink::new();
    let outcome = run_one(
        &req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("run_one ok");

    assert_eq!(outcome, RunOutcome::Ok, "expected RunOutcome::Ok");

    let findings = parse_findings(&sink.0);
    let scan_errors: Vec<&Finding> = findings
        .iter()
        .filter(|f| matches!(f, Finding::ScanError(_)))
        .collect();
    assert!(
        scan_errors.is_empty(),
        "expected zero ScanError envelopes on Tf1h; got {} — full stream:\n{}",
        scan_errors.len(),
        sink.as_str()
    );

    let results: Vec<&Finding> = findings
        .iter()
        .filter(|f| matches!(f, Finding::Result(_)))
        .collect();
    assert!(
        results.len() >= 2,
        "expected at least two Result envelopes (one per side of the gap) on Tf1h, got {}",
        results.len(),
    );
}

/// RAD-2642 — sub-bucket holes are absorbed by the timeframe-aware gap
/// projection. A single 1-minute hole at `12:00` inside a Tf15m window
/// must produce ONE pass-through `Result` envelope (no partition), with
/// the raw 1-minute `IntraDayGap` preserved in the inlined data_slice
/// manifest for data-quality auditing.
///
/// This is the regression that locks the v1.0.2 → v1.0.3 semantics shift
/// in place. Pre-fix this produced 2+ `Result` envelopes (one per side
/// of the hole). Post-fix it produces exactly one.
#[test]
#[serial_test::serial]
fn run_one_absorbs_sub_bucket_hole_at_15m() {
    let day = open_day();
    // 1-minute hole at 12:00 — fully inside [12:00, 12:15) Tf15m bucket.
    let cache =
        SyntheticCache::new().with_day_holed("EURUSD", Side::Bid, day, 0xCAFE_F00D, 720..721);
    let cfg = build_cfg(&cache);
    let reader = DukascopyReader::new(cache.cache_root());

    let req = ljung_box_request(day_window(day), Timeframe::Tf15m);

    let mut sink = BufferSink::new();
    let outcome = run_one(
        &req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("run_one ok");

    assert_eq!(outcome, RunOutcome::Ok, "expected RunOutcome::Ok");

    let findings = parse_findings(&sink.0);
    let scan_errors: Vec<&Finding> = findings
        .iter()
        .filter(|f| matches!(f, Finding::ScanError(_)))
        .collect();
    assert!(
        scan_errors.is_empty(),
        "expected zero ScanError envelopes; got {} — full stream:\n{}",
        scan_errors.len(),
        sink.as_str()
    );

    let results: Vec<&Finding> = findings
        .iter()
        .filter(|f| matches!(f, Finding::Result(_)))
        .collect();
    assert_eq!(
        results.len(),
        1,
        "sub-bucket hole must NOT partition the window; expected one \
         pass-through Result, got {}; full stream:\n{}",
        results.len(),
        sink.as_str()
    );

    // The single Result's inlined manifest still carries the raw 1-min hole
    // — data quality info is preserved even though dispatch ignored it.
    let Finding::Result(r) = results[0] else {
        unreachable!("filtered above")
    };
    let m = r
        .data_slice
        .gap_manifest
        .as_ref()
        .expect("inlined manifest required under ContinuousOnly");
    assert_eq!(m.gaps.len(), 1, "raw manifest preserved, got {:?}", m.gaps);
    let gap = &m.gaps[0];
    assert_eq!(
        gap.start_utc,
        Utc.with_ymd_and_hms(2024, 6, 12, 12, 0, 0).unwrap()
    );
    assert_eq!(
        gap.end_utc,
        Utc.with_ymd_and_hms(2024, 6, 12, 12, 1, 0).unwrap()
    );
}

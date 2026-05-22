//! RAD-2352 regression — gap-continuation alignment fix end-to-end.
//!
//! Drives [`engine::run_one`] against a synthetic Dukascopy cache that contains
//! a single intra-minute hole during open hours, with `--gap-policy
//! continuous_only` and a 15-minute target timeframe. The pre-fix engine
//! handed the aggregator a post-gap `range.start` of `12:01:00` (one minute
//! after the hole), which the aggregator's alignment guard rejected with
//! `range.start ... is not aligned to Tf15m boundary`, short-circuiting the
//! whole post-gap slice into a `Finding::ScanError`.
//!
//! After the fix in `gap_policy::snap_subranges_to_timeframe` the engine
//! snaps the post-gap start UP to `12:15:00` (the next 15m bucket boundary)
//! before handing the range to `BarCache::get_or_build`, and the scan emits
//! one `Finding::Result` envelope per gap-free sub-range with zero
//! `Finding::ScanError` envelopes.
//!
//! Acceptance criteria 1 of RAD-2352 (the unit/integration test that
//! reproduces the bug pre-fix and passes post-fix).

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

/// Run the engine against a 1-day synthetic Dukascopy fixture with a single
/// missing minute and assert: exit OK, at least two `Result` envelopes, zero
/// `ScanError` envelopes, exactly one `IntraDayGap` in the inlined manifest.
///
/// Pre-fix this asserted `assert!(scan_errors.is_empty())` would fail with
/// one `cache: aggregator error: range.start ... is not aligned to Tf15m
/// boundary` line per intra-minute hole.
#[test]
#[serial_test::serial]
fn run_one_emits_results_across_intra_minute_15m_gap() {
    let day = open_day();
    // Hole at minute 720 (12:00 UTC). Post-gap subrange would start at 12:01,
    // which is NOT on a 15m boundary — exercises the alignment guard the
    // partitioner snap now compensates for.
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

    // Each Result must carry the inlined gap manifest with exactly one
    // IntraDayGap span. The manifest is the FULL queried-window manifest
    // (not the per-sub-range one) per the engine's `ctx_gap_manifest` rule.
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
            1,
            "expected one IntraDayGap in inlined manifest, got {} ({:?})",
            m.gaps.len(),
            m.gaps,
        );
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
}

/// Same shape as [`run_one_emits_results_across_intra_minute_15m_gap`] but
/// drives a 1-hour timeframe across a hole whose post-gap start (12:01) is
/// also not aligned to a 1h boundary. Mirrors step-6 of the v1.0 smoke
/// (cross.cointegration, Tf1h) at the single-leg layer — the same snap
/// powers both single- and pair-arity dispatch under RAD-2352.
#[test]
#[serial_test::serial]
fn run_one_emits_results_across_intra_minute_1h_gap() {
    let day = open_day();
    let cache =
        SyntheticCache::new().with_day_holed("EURUSD", Side::Bid, day, 0xBADC_0FFE, 720..721);
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

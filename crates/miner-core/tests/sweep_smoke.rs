//! Plan 05-04 Task 3 — sweep_smoke: end-to-end 2-scan × 2-instrument
//! integration test for `miner_core::sweep::run_sweep`.
//!
//! 2 scans (`stats.autocorr.ljung_box@1`,
//! `stats.autocorr.ljung_box_sq@1`) × 2 instruments
//! (EURUSD:bid, GBPUSD:bid) × 1 timeframe × 1 window × 1 param-grid
//! ⇒ 4 jobs ⇒ 4 `Finding::Result` envelopes.
//!
//! Envelope sequence assertion:
//!   `RunStart` → 4 × `Result` → `SweepSummary` → `RunEnd`
//!
//! `SweepSummary.fdr_by_family.len() == 2` (default `[fdr].family =
//! "scan_id"` produces one entry per `scan_id@version`). Both scans
//! emit `effect.p_value: Some(_)` so both populate BH-FDR families.

#![allow(clippy::doc_lazy_continuation, clippy::doc_markdown)]
#[allow(clippy::too_many_lines)]
mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::NaiveDate;

use miner_core::cache::BarCache;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::findings::Finding;
use miner_core::reader::Side;
use miner_core::sweep::manifest::parse_manifest_str;
use miner_core::sweep::{SweepOptions, run_sweep};
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, synthetic_cache::SyntheticCache};

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "end-to-end smoke for the sweep facade — cache setup, manifest parsing, options wiring, sink construction, and the cluster of post-run assertions are intentionally inline so the path reads top-to-bottom; splitting hides the data flow"
)]
fn sweep_smoke_two_scans_two_instruments() {
    // Synthetic cache: same day for both EURUSD and GBPUSD; deterministic
    // LCG-seeded bars so the sweep run is reproducible.
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).expect("valid date");
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 0x1234_5678)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 0x9ABC_DEF0);

    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());
    let bar_cache = BarCache::new(cache.bar_cache_root());

    // Inline TOML manifest. 2 [[jobs]] blocks, each over 2 instruments
    // (Single-arity), 1 timeframe, 1 window, no params.
    let manifest_toml = r#"
        [sweep]
        seed = 305419896

        [[jobs]]
        scan = "stats.autocorr.ljung_box@1"
        instruments = ["EURUSD:bid", "GBPUSD:bid"]
        timeframes = ["15m"]
        windows = ["2024-06-12:2024-06-13"]
        params = { lags = 5 }

        [[jobs]]
        scan = "stats.autocorr.ljung_box_sq@1"
        instruments = ["EURUSD:bid", "GBPUSD:bid"]
        timeframes = ["15m"]
        windows = ["2024-06-12:2024-06-13"]
        params = { lags = 5 }
    "#;
    let manifest = parse_manifest_str(manifest_toml).expect("manifest parses");

    let mut sink = BufferSink::new();
    let outcome = run_sweep(
        manifest,
        SweepOptions::default(),
        &cfg,
        &reader,
        &bar_cache,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("sweep ok");
    assert_eq!(
        outcome,
        miner_core::engine::RunOutcome::Ok,
        "sweep with healthy fixture must return RunOutcome::Ok"
    );

    let findings = common::parse_findings(&sink.0);

    // Envelope sequence: RunStart, 4 × Result, SweepSummary, RunEnd.
    assert!(
        findings.len() >= 4 + 3,
        "expected at least RunStart + 4 Result + SweepSummary + RunEnd ({} envelopes); got {}: {:?}",
        4 + 3,
        findings.len(),
        findings.iter().map(envelope_kind).collect::<Vec<_>>(),
    );

    assert!(
        matches!(findings[0], Finding::RunStart(_)),
        "first envelope must be RunStart; got {}",
        envelope_kind(&findings[0])
    );

    // Result count: exactly 4 (one per cartesian-job under healthy
    // synthetic data; no GapAborted, no ScanError).
    let result_count = findings
        .iter()
        .filter(|f| matches!(f, Finding::Result(_)))
        .count();
    assert_eq!(
        result_count,
        4,
        "expected 4 Result envelopes (2 scans × 2 instruments × 1 tf × 1 window × 1 params); got {}: {:?}",
        result_count,
        findings.iter().map(envelope_kind).collect::<Vec<_>>(),
    );

    // Exactly one SweepSummary, strictly between last Result and RunEnd.
    let summary_count = findings
        .iter()
        .filter(|f| matches!(f, Finding::SweepSummary(_)))
        .count();
    assert_eq!(
        summary_count, 1,
        "expected exactly one SweepSummary envelope"
    );

    let last = findings.last().expect("non-empty findings");
    assert!(
        matches!(last, Finding::RunEnd(_)),
        "last envelope must be RunEnd; got {}",
        envelope_kind(last)
    );

    // SweepSummary.fdr_by_family.len() == 2 (default [fdr].family =
    // "scan_id" produces one entry per scan_id@version).
    let summary = findings
        .iter()
        .find_map(|f| {
            if let Finding::SweepSummary(s) = f {
                Some(s)
            } else {
                None
            }
        })
        .expect("SweepSummary present");
    assert_eq!(
        summary.fdr_by_family.len(),
        2,
        "expected 2 families ({}); got {} keys: {:?}",
        "default [fdr].family = scan_id",
        summary.fdr_by_family.len(),
        summary.fdr_by_family.keys().collect::<Vec<_>>(),
    );
    // Both entries use BH-FDR.
    for (k, v) in &summary.fdr_by_family {
        assert_eq!(
            v.method, "benjamini_hochberg",
            "family {k}: method must be benjamini_hochberg; got {:?}",
            v.method,
        );
    }
}

fn envelope_kind(f: &Finding) -> &'static str {
    match f {
        Finding::RunStart(_) => "run_start",
        Finding::Result(_) => "result",
        Finding::ScanError(_) => "scan_error",
        Finding::GapAborted(_) => "gap_aborted",
        Finding::RunEnd(_) => "run_end",
        Finding::DryRun(_) => "dry_run",
        Finding::SweepSummary(_) => "sweep_summary",
    }
}

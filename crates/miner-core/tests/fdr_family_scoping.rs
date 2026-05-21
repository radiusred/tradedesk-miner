//! Plan 05-04 Task 3 — fdr_family_scoping integration test.
//!
//! Parametrically drives the same 4-job sweep through each of the
//! four `[fdr].family` values and asserts the per-variant
//! `SweepSummary.fdr_by_family` key count + grouping:
//!
//! - `"scan_id"` (default) → 2 keys (one per `scan_id@version`).
//! - `"scan_family"` → 1 key (`"stats"` — both scans are
//!   `stats.autocorr.*`).
//! - `"all"` → 1 key (`"all"`).
//! - `"none"` → empty `fdr_by_family` (SweepSummary still emits with
//!   `fdr_by_family.len() == 0` — Plan 05-04 SUMMARY decision 3
//!   pins this; the consumer branches on `.is_empty()`).

#![allow(clippy::doc_lazy_continuation, clippy::doc_markdown)]

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

fn run_sweep_with_family(fdr_family: &str) -> Vec<Finding> {
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).expect("valid date");
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 0x1111)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 0x2222);

    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());
    let bar_cache = BarCache::new(cache.bar_cache_root());

    let manifest_toml = format!(
        r#"
        [sweep]
        seed = 57005

        [fdr]
        family = "{fdr_family}"
        alpha = 0.05

        [[jobs]]
        scan = "stats.autocorr.ljung_box@1"
        instruments = ["EURUSD:bid", "GBPUSD:bid"]
        timeframes = ["15m"]
        windows = ["2024-06-12:2024-06-13"]
        params = {{ lags = 5 }}

        [[jobs]]
        scan = "stats.autocorr.ljung_box_sq@1"
        instruments = ["EURUSD:bid", "GBPUSD:bid"]
        timeframes = ["15m"]
        windows = ["2024-06-12:2024-06-13"]
        params = {{ lags = 5 }}
    "#
    );
    let manifest = parse_manifest_str(&manifest_toml).expect("manifest parses");

    let mut sink = BufferSink::new();
    run_sweep(
        manifest,
        SweepOptions::default(),
        &cfg,
        &reader,
        &bar_cache,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("sweep ok");

    common::parse_findings(&sink.0)
}

/// `[fdr].family = "scan_id"` (default) — one family per
/// `scan_id@version`. With 2 scans, `fdr_by_family.len() == 2`.
#[test]
fn fdr_family_scoping_scan_id_per_scan_at_version() {
    let findings = run_sweep_with_family("scan_id");
    let summary = extract_summary(&findings);
    assert_eq!(
        summary.fdr_by_family.len(),
        2,
        "scan_id scope: expected 2 keys; got {} -> {:?}",
        summary.fdr_by_family.len(),
        summary.fdr_by_family.keys().collect::<Vec<_>>()
    );
    assert!(summary.fdr_by_family.contains_key("stats.autocorr.ljung_box@1"));
    assert!(
        summary
            .fdr_by_family
            .contains_key("stats.autocorr.ljung_box_sq@1")
    );
}

/// `[fdr].family = "scan_family"` — one family per `scan_id` first-dot
/// prefix. Both scans live under `stats.*`, so `fdr_by_family.len() == 1`,
/// sole key `"stats"`.
#[test]
fn fdr_family_scoping_scan_family_first_dot_prefix() {
    let findings = run_sweep_with_family("scan_family");
    let summary = extract_summary(&findings);
    assert_eq!(
        summary.fdr_by_family.len(),
        1,
        "scan_family scope: expected 1 key; got {} -> {:?}",
        summary.fdr_by_family.len(),
        summary.fdr_by_family.keys().collect::<Vec<_>>()
    );
    assert!(
        summary.fdr_by_family.contains_key("stats"),
        "scan_family scope: sole key must be 'stats'; got {:?}",
        summary.fdr_by_family.keys().collect::<Vec<_>>()
    );
}

/// `[fdr].family = "all"` — sweep-wide single family; sole key `"all"`.
#[test]
fn fdr_family_scoping_all_single_global_family() {
    let findings = run_sweep_with_family("all");
    let summary = extract_summary(&findings);
    assert_eq!(
        summary.fdr_by_family.len(),
        1,
        "all scope: expected 1 key; got {} -> {:?}",
        summary.fdr_by_family.len(),
        summary.fdr_by_family.keys().collect::<Vec<_>>()
    );
    assert!(
        summary.fdr_by_family.contains_key("all"),
        "all scope: sole key must be 'all'; got {:?}",
        summary.fdr_by_family.keys().collect::<Vec<_>>()
    );
}

/// `[fdr].family = "none"` — BH-FDR skipped entirely;
/// `fdr_by_family.is_empty()`. SweepSummary IS still emitted
/// (decision 3 in SUMMARY.md — consumer branches on .is_empty()).
#[test]
fn fdr_family_scoping_none_emits_empty_fdr_map_but_summary_still_emitted() {
    let findings = run_sweep_with_family("none");
    let summary = extract_summary(&findings);
    assert!(
        summary.fdr_by_family.is_empty(),
        "none scope: fdr_by_family must be empty; got {} keys: {:?}",
        summary.fdr_by_family.len(),
        summary.fdr_by_family.keys().collect::<Vec<_>>()
    );
    // Totals are still populated even when FDR is off — 4 results
    // emitted (2 scans × 2 instruments).
    assert_eq!(summary.totals.results_emitted, 4);
}

fn extract_summary(findings: &[Finding]) -> &miner_core::findings::SweepSummaryFinding {
    findings
        .iter()
        .find_map(|f| {
            if let Finding::SweepSummary(s) = f {
                Some(s)
            } else {
                None
            }
        })
        .expect("SweepSummary envelope present")
}

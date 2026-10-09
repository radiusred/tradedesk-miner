//! Plan 05-04 Task 3 — sweep_summary_emission integration test.
//!
//! Structural assertions on the `Finding::SweepSummary` envelope:
//!
//! 1. Exactly one `SweepSummary` envelope.
//! 2. Its position is strictly AFTER the last `Result` and strictly
//!    BEFORE `RunEnd`.
//! 3. Every `FdrFamilySummary.method == "benjamini_hochberg"`.
//! 4. Every `FdrFamilySummary.alpha == 0.05` (default `[fdr].alpha`).
//! 5. `per_finding` is in stable index order — `finding_index` is
//!    monotonically non-decreasing within each family.

#![allow(
    clippy::doc_lazy_continuation,
    clippy::doc_markdown,
    clippy::cast_possible_wrap,
    clippy::too_many_lines
)]

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
fn sweep_summary_envelope_position_and_shape() {
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

    let manifest_toml = r#"
        [sweep]
        seed = 57005

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
    let manifest = parse_manifest_str(manifest_toml).expect("parse");

    let mut sink = BufferSink::new();
    let _ = run_sweep(
        manifest,
        SweepOptions::default(),
        &cfg,
        &reader,
        &bar_cache,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("ok");

    let findings = common::parse_findings(&sink.0);

    // Structural position assertions: find indices.
    let last_result_pos = findings
        .iter()
        .rposition(|f| matches!(f, Finding::Result(_)))
        .expect("at least one Result envelope");
    let summary_pos = findings
        .iter()
        .position(|f| matches!(f, Finding::SweepSummary(_)))
        .expect("SweepSummary envelope present");
    let run_end_pos = findings
        .iter()
        .position(|f| matches!(f, Finding::RunEnd(_)))
        .expect("RunEnd envelope present");

    assert!(
        summary_pos > last_result_pos,
        "SweepSummary (pos {summary_pos}) must come AFTER the last Result (pos {last_result_pos})"
    );
    assert!(
        summary_pos < run_end_pos,
        "SweepSummary (pos {summary_pos}) must come BEFORE RunEnd (pos {run_end_pos})"
    );

    // Exactly one SweepSummary.
    let summary_count = findings
        .iter()
        .filter(|f| matches!(f, Finding::SweepSummary(_)))
        .count();
    assert_eq!(summary_count, 1, "exactly one SweepSummary envelope");

    // Extract the SweepSummary payload for shape assertions.
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

    // Per-family shape: method == benjamini_hochberg, alpha == 0.05 (default).
    for (family_key, family_summary) in &summary.fdr_by_family {
        assert_eq!(
            family_summary.method, "benjamini_hochberg",
            "family {family_key}: method must be benjamini_hochberg"
        );
        assert!(
            (family_summary.alpha - 0.05).abs() < 1e-12,
            "family {family_key}: alpha must be 0.05 (default); got {}",
            family_summary.alpha
        );
        // per_finding indices must be monotonically non-decreasing
        // (stable index order — finding_index_within_family is
        // zero-indexed and emitted in streaming order).
        let mut last_idx: i64 = -1;
        for entry in &family_summary.per_finding {
            assert!(
                (entry.finding_index as i64) >= last_idx,
                "family {family_key}: finding_index must be monotonically non-decreasing; got {} after {}",
                entry.finding_index,
                last_idx,
            );
            last_idx = entry.finding_index as i64;
            // q_value must be in [0, 1] and finite.
            assert!(
                entry.q_value.is_finite() && (0.0..=1.0).contains(&entry.q_value),
                "q_value must be in [0, 1]; got {}",
                entry.q_value
            );
        }
    }

    // 2 families expected (default [fdr].family = "scan_id" produces
    // one entry per scan_id@version; both Ljung-Box variants emit p-values).
    assert_eq!(
        summary.fdr_by_family.len(),
        2,
        "expected 2 families (scan_id scope); got {} keys: {:?}",
        summary.fdr_by_family.len(),
        summary.fdr_by_family.keys().collect::<Vec<_>>(),
    );
}

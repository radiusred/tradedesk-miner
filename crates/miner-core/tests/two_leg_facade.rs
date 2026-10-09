#![allow(clippy::useless_vec)]

//! Phase 4 Plan 04-12 (CR-01) — two-leg facade regression gate.
//!
//! ## Why this file exists in its current form
//!
//! Plan 04-02 shipped `tests/two_leg_facade.rs` as a SCAFFOLD: the module
//! docstring acknowledged that it did not call `engine::run_one`, and
//! deferred the end-to-end engine integration to "a later plan". Plan
//! 04-07 (CROSS Wave 3) then deferred the engine wiring further with the
//! "deferred to Plan 04-11" note. Plan 04-11 (sign-off) did not pick the
//! wiring back up. Result: every CROSS scan's integration test bypassed
//! the engine by manually constructing `ScanCtx { bars_pair: Some(..), .. }`
//! — kernel-correct, facade-broken. CR-01 (04-REVIEW.md) surfaced exactly
//! the structural hole that scaffold had left open.
//!
//! Plan 04-12 closes the hole. This file is now the engine-level
//! regression gate for CR-01: it constructs a Pair-arity `ScanRequest`,
//! invokes `engine::run_one_with_registry` against a `SyntheticCache` +
//! `DukascopyReader` with BOTH legs populated, and asserts a
//! `Finding::Result` envelope (NOT `Finding::ScanError` with the
//! "expected Pair arity" message) lands in the sink. A future regression
//! that drops the Pair branch from `engine::run_one_with_registry` trips
//! this test by name.
//!
//! ## Why we keep the original primitive-shape tests below
//!
//! The two original tests (`inner_join_aligns_two_leg_close_vectors` +
//! `data_slice_sources_vec_is_reachable_for_two_leg_envelopes`) document
//! that the Pair primitive surface (`inner_join` + the Source struct) is
//! reachable from integration tests. They predate Plan 04-12 and remain
//! valid as primitive-shape pins; the new test sits alongside them as
//! the engine-path regression gate.

#![allow(clippy::too_many_lines)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, NaiveDate, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{RunOutcome, param_hash, run_one_with_registry};
use miner_core::findings::{Finding, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::cross::LeadLagCcfScan;
use miner_core::scan::primitives::time_alignment::{AlignedPair, inner_join};
use miner_core::scan::{Registry, ScanRequest};
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, synthetic_cache::SyntheticCache};

/// Build a synthetic `BarFrame` with the given timestamps and closes. Used by
/// the Pattern-A primitive-shape tests below (NOT the engine-path test —
/// that one drives the production aggregator via `DukascopyReader` + `BarCache`).
fn build_bars(symbol: &str, ts: &[chrono::DateTime<Utc>], closes: &[f64]) -> BarFrame {
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

/// **CR-01 regression gate — engine-path Pair-arity dispatch (Plan 04-12).**
///
/// Constructs a real Pair-arity request (`cross.lead_lag.ccf@1`), populates
/// BOTH legs in a `SyntheticCache`, invokes `engine::run_one_with_registry`
/// through the production `DukascopyReader` + `BarCache` pipeline, and
/// asserts:
///
/// 1. The function returns `RunOutcome::Ok` (NOT `HadScanErrors`).
/// 2. Exactly one `Finding::Result` envelope lands in the sink.
/// 3. `data_slice.sources.len() == 2` with leg-A == EURUSD, leg-B == GBPUSD
///    (D4-03 — Pair-arity Source vector populated in `req.instruments` order).
/// 4. The envelope is NOT a `Finding::ScanError` with the "expected Pair
///    arity" message (CR-01 — without the Plan 04-12 fix the engine would
///    hard-code `bars_pair: None` and surface that exact message).
///
/// Without the Plan 04-12 fix to `engine::run_one_with_registry`, this test
/// trips on assertion #4 (or #1, depending on whether the engine still
/// returns `Ok(HadScanErrors)` for the embedded `ScanError`).
#[test]
fn two_leg_facade_pair_arity_dispatch_emits_result_envelope() {
    // Day with bars for BOTH legs. `with_deterministic_day` writes 1440
    // 1-minute bars under `<cache_root>/EURUSD/<YYYY>/<MM>/<DD>_<bid|ask>.csv.zst`.
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).unwrap();
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 17)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 29);

    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());

    // Register the real LeadLagCcfScan (Plan 04-08 Task 1 / CROSS-04).
    let mut registry = Registry::new();
    registry.register(Box::new(LeadLagCcfScan));

    let start = Utc.with_ymd_and_hms(2024, 6, 12, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 6, 13, 0, 0, 0).unwrap();
    let resolved = serde_json::json!({"max_lag": 5});
    let param_hash = param_hash::param_hash(&resolved).expect("param_hash ok");
    let req = ScanRequest {
        scan_id: "cross.lead_lag.ccf".into(),
        version: 1,
        // Pair-arity request: two legs in declared order — leg-A = EURUSD,
        // leg-B = GBPUSD.
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
        timeframe: Timeframe::Tf15m,
        window: ClosedRangeUtc { start, end },
        sub_range: TimeRange {
            start_utc: start,
            end_utc: end,
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
    };

    let mut sink = BufferSink::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let outcome = run_one_with_registry(&req, &cfg, &reader, &mut sink, cancel, &registry)
        .expect("engine::run_one_with_registry must succeed for a well-formed Pair-arity request");

    // Assertion 1 — RunOutcome::Ok. The previous CR-01 behaviour would have
    // produced HadScanErrors (the engine emitted a Finding::ScanError because
    // it hard-coded bars_pair: None).
    assert_eq!(
        outcome,
        RunOutcome::Ok,
        "Pair-arity dispatch must return RunOutcome::Ok, NOT HadScanErrors (CR-01 fix)"
    );

    let findings = common::parse_findings(&sink.0);

    // Assertion 4 — NO ScanError with the bars_pair=None message.
    for f in &findings {
        if let Finding::ScanError(se) = f {
            assert!(
                !se.message.contains("expected Pair arity"),
                "CR-01 regression: engine emitted ScanError({:?}); the Pair dispatch must reach the kernel via dispatch_pair_arity_body",
                se.message
            );
        }
    }

    // Assertion 2 — exactly one Finding::Result envelope.
    let result_count = findings
        .iter()
        .filter(|f| matches!(f, Finding::Result(_)))
        .count();
    assert_eq!(
        result_count,
        1,
        "expected exactly one Finding::Result envelope; sink contained {} envelopes total",
        findings.len()
    );

    let result = findings
        .iter()
        .find_map(|f| match f {
            Finding::Result(r) => Some(r),
            _ => None,
        })
        .expect("Result envelope present after Pair-arity dispatch");

    // Assertion 3 — D4-03 Pair-arity Source vector populated.
    assert_eq!(
        result.data_slice.sources.len(),
        2,
        "Pair-arity Finding::Result must carry data_slice.sources.len() == 2 (D4-03)"
    );
    assert_eq!(result.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(result.data_slice.sources[1].symbol, "GBPUSD");
    assert_eq!(result.data_slice.sources[0].side, "bid");
    assert_eq!(result.data_slice.sources[1].side, "bid");
    assert_eq!(result.data_slice.sources[0].timeframe, "15m");

    // Sanity: the envelope identifies the real CROSS-04 scan.
    assert_eq!(result.scan_id_at_version, "cross.lead_lag.ccf@1");

    // The lead-lag CCF scan emits `lead_lag_argmax_lag` as its effect metric;
    // pin it so future scan renames trip a clear failure rather than a
    // length-only assertion regression.
    assert_eq!(result.effect.metric, "lead_lag_argmax_lag");
}

/// D4-01 / CROSS-01 primitive-shape pin (predates Plan 04-12): two-leg
/// inner-join is reachable from integration tests and the [`AlignedPair`]
/// type exposes the documented public fields (`timestamps_ms`, `close_a`,
/// `close_b`). Kept because the primitive-shape coverage is orthogonal to
/// the engine-path test above.
#[test]
fn inner_join_aligns_two_leg_close_vectors() {
    let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts_a = [t0, t0 + Duration::minutes(15), t0 + Duration::minutes(30)];
    let ts_b = [t0 + Duration::minutes(15), t0 + Duration::minutes(30)];
    let bars_a = build_bars("EURUSD", &ts_a, &[1.0, 1.1, 1.2]);
    let bars_b = build_bars("GBPUSD", &ts_b, &[2.1, 2.2]);

    let aligned: AlignedPair = inner_join(&bars_a, &bars_b);

    assert_eq!(
        aligned.timestamps_ms.len(),
        2,
        "joint timestamps = intersection of both legs"
    );
    assert_eq!(aligned.close_a.len(), aligned.timestamps_ms.len());
    assert_eq!(aligned.close_b.len(), aligned.timestamps_ms.len());
    assert_eq!(aligned.close_a, vec![1.1, 1.2]);
    assert_eq!(aligned.close_b, vec![2.1, 2.2]);

    let want_ms_0 = (t0 + Duration::minutes(15)).timestamp_millis();
    let want_ms_1 = (t0 + Duration::minutes(30)).timestamp_millis();
    assert_eq!(aligned.timestamps_ms[0], want_ms_0);
    assert_eq!(aligned.timestamps_ms[1], want_ms_1);
}

/// D4-03 primitive-shape pin (predates Plan 04-12): the public `Source`
/// constructor surface is reachable from integration tests so CROSS-scan
/// bodies can build the per-leg Source vector. Now backed by the engine-path
/// test above which proves the `data_slice.sources.len() == 2` invariant
/// holds through real `engine::run_one_with_registry`.
#[test]
fn data_slice_sources_vec_is_reachable_for_two_leg_envelopes() {
    use miner_core::findings::Source;
    let leg_a = Source {
        source_id: "dukascopy".into(),
        symbol: "EURUSD".into(),
        side: "bid".into(),
        timeframe: "15m".into(),
    };
    let leg_b = Source {
        source_id: "dukascopy".into(),
        symbol: "GBPUSD".into(),
        side: "bid".into(),
        timeframe: "15m".into(),
    };
    let sources = vec![leg_a, leg_b];
    assert_eq!(sources.len(), 2, "D4-03 sources Vec len for Pair");
    assert_eq!(sources[0].symbol, "EURUSD");
    assert_eq!(sources[1].symbol, "GBPUSD");
}

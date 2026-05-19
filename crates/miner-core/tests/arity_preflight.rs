//! Phase 4 (Plan 04-02 Task 2) integration test — D4-02 arity preflight.
//!
//! Submits a single-leg `ScanRequest` against a Pair-arity stub scan
//! via `engine::run_one_with_registry`. The arity preflight (added in
//! `engine::preflight::validate_arity`) must reject the mismatched request
//! with `PreflightCode::WrongInstrumentArity` BEFORE the engine emits
//! `RunStart` — so the function returns `Err(MinerError::Preflight(WireError))`
//! with `code == "wrong_instrument_arity"`, stdout stays empty, and the CLI
//! exit-code router (D3-24) yields exit 1.
//!
//! Pattern analog: `crates/miner-core/tests/gap_policy.rs` (calls preflight
//! helpers + builds in-process scans). The Wave-2 stub Pair-arity scan
//! lives inline in this file — Plan 04-07 will register real CROSS scans
//! into `scan::cross::register_cross_scans`.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{TimeZone, Utc};

use miner_core::aggregator::Timeframe;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{param_hash, run_one_with_registry};
use miner_core::error::MinerError;
use miner_core::findings::TimeRange;
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::{
    Registry, Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest,
};
use miner_reader_dukascopy::DukascopyReader;

use common::BufferSink;

/// Stub Pair-arity scan. Registered in a per-test Registry so the engine's
/// arity preflight can be exercised without touching the production
/// bootstrap (Plan 04-07 will register real CROSS scans).
struct StubPair;

impl Scan for StubPair {
    fn id(&self) -> &'static str {
        "stub.cross.pair"
    }
    fn version(&self) -> u32 {
        1
    }
    fn arity(&self) -> ScanArity {
        ScanArity::Pair
    }
    fn param_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "additionalProperties": false})
    }
    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[],
            raw_series_keys: &[],
        }
    }
    fn run(
        &self,
        _ctx: &ScanCtx<'_>,
        _req: &ScanRequest,
        _sink: &mut dyn miner_core::FindingSink,
    ) -> Result<(), ScanError> {
        // Unreachable in this test — preflight rejects before dispatch.
        Ok(())
    }
}

/// D4-02 — single-leg request against a Pair-arity scan rejects at
/// preflight with `wrong_instrument_arity`; stdout stays empty.
#[test]
fn wrong_arity_single_leg_against_pair_scan_rejects_at_preflight() {
    // Build a per-test Registry containing only StubPair.
    let mut registry = Registry::new();
    registry.register(Box::new(StubPair));

    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = MinerConfig {
        cache_root: tmp.path().join("c"),
        bar_cache_root: tmp.path().join("bc"),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(&cfg.cache_root);

    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
    let resolved = serde_json::json!({});
    let param_hash = param_hash::param_hash(&resolved).expect("ok");

    let req = ScanRequest {
        scan_id: "stub.cross.pair".into(),
        version: 1,
        // Single-leg input — but the scan declares Pair; preflight rejects.
        instruments: vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }],
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
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let result = run_one_with_registry(&req, &cfg, &reader, &mut sink, cancel, &registry);

    // The engine returns a typed Preflight error with the
    // `wrong_instrument_arity` code; stdout stays empty per D3-23 / D-06
    // (no RunStart, no Result, no RunEnd).
    match result {
        Err(MinerError::Preflight(w)) => {
            assert_eq!(
                w.code, "wrong_instrument_arity",
                "preflight WireError must carry PreflightCode::WrongInstrumentArity"
            );
            assert_eq!(
                w.context.get("expected_arity"),
                Some(&serde_json::json!(2)),
                "expected_arity must be 2 (Pair)"
            );
            assert_eq!(
                w.context.get("supplied_arity"),
                Some(&serde_json::json!(1)),
                "supplied_arity must be 1 (the singleton in the request)"
            );
        }
        other => panic!("expected MinerError::Preflight(WrongInstrumentArity); got {other:?}"),
    }
    assert!(
        sink.0.is_empty(),
        "stdout must stay empty on preflight rejection (D-06)"
    );
}

/// D4-02 — Pair scan + two-leg request preflights successfully (i.e. the
/// engine proceeds past the arity check; the test only asserts that the
/// engine no longer rejects at preflight). The bar fetch then fails for
/// the synthetic-cache-less reader, which surfaces as a Finding::ScanError
/// after RunStart — that proves the arity preflight passed.
#[test]
fn correct_arity_pair_scan_passes_arity_preflight() {
    let mut registry = Registry::new();
    registry.register(Box::new(StubPair));

    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = MinerConfig {
        cache_root: tmp.path().join("c"),
        bar_cache_root: tmp.path().join("bc"),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(&cfg.cache_root);

    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
    let resolved = serde_json::json!({});
    let param_hash = param_hash::param_hash(&resolved).expect("ok");

    let req = ScanRequest {
        scan_id: "stub.cross.pair".into(),
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
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let result = run_one_with_registry(&req, &cfg, &reader, &mut sink, cancel, &registry);

    // Arity preflight passes — the engine progresses past the preflight
    // step. The exact post-preflight outcome depends on the engine's Pair
    // branch (Plan 04-07 will land it); the assertion here is only that
    // we do NOT see a Preflight(WrongInstrumentArity) error.
    match result {
        Err(MinerError::Preflight(ref w)) if w.code == "wrong_instrument_arity" => {
            panic!("Arity preflight must accept Pair scan + two-leg request; got {w:?}");
        }
        _ => {
            // Anything else (Ok, or non-arity error) is fine — the test
            // pins ONLY the arity-preflight gate. Plan 04-07's CROSS
            // dispatch will tighten the assertion to a full
            // RunStart/Result/RunEnd shape.
        }
    }
}

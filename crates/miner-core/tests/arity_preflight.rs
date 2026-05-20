//! Phase 4 integration tests — D4-02 arity preflight + Plan 04-12 (CR-01)
//! post-preflight `Finding::Result` assertion.
//!
//! Two tests:
//!
//! 1. `wrong_arity_single_leg_against_pair_scan_rejects_at_preflight` —
//!    arity preflight rejects a single-leg request against a Pair scan
//!    with `PreflightCode::WrongInstrumentArity` BEFORE the engine emits
//!    `RunStart`. Stdout stays empty (D-06).
//! 2. `correct_arity_pair_scan_passes_arity_preflight` — Plan 04-12 (CR-01
//!    sibling regression gate) tightens this from "any non-arity outcome
//!    is fine" to "a `Finding::Result` envelope MUST be produced". Without
//!    the Plan 04-12 fix the engine would emit `Finding::ScanError` with
//!    `"expected Pair arity (ctx.bars_pair is None)"` — this test now trips
//!    on that regression by demanding a real Result post-preflight.
//!
//! Pattern analog: `crates/miner-core/tests/gap_policy.rs` (calls preflight
//! helpers + builds in-process scans).

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{NaiveDate, TimeZone, Utc};

use miner_core::aggregator::Timeframe;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{RunOutcome, param_hash, run_one_with_registry};
use miner_core::error::MinerError;
use miner_core::findings::{DataSlice, Finding, Source, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::{
    Registry, Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest,
};
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, synthetic_cache::SyntheticCache};

/// Stub Pair-arity scan. Registered in a per-test Registry so the engine's
/// arity preflight + Pair dispatch path can be exercised without depending
/// on a real CROSS scan's parameter surface.
///
/// Plan 04-12: the scan body now emits exactly one `Finding::Result` so
/// the post-preflight assertion can require a real Result envelope (not
/// just "any non-arity outcome"). The body also panics if
/// `ctx.bars_pair.is_none()` — that's the CR-01 regression negative pin.
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
            raw_series_keys: &["timestamps_ms"],
        }
    }
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn miner_core::FindingSink,
    ) -> Result<(), ScanError> {
        // CR-01 negative pin: Pair-arity scans MUST receive a populated
        // bars_pair. If the engine regresses to bars_pair: None, this
        // assertion fires the test as a panic.
        assert!(
            ctx.bars_pair.is_some(),
            "StubPair (CR-01 pin): engine must pass bars_pair: Some((a, b)) for Pair arity"
        );
        let sources: Vec<Source> = req
            .instruments
            .iter()
            .map(|spec| Source {
                source_id: ctx.bars.source_id.clone(),
                symbol: spec.symbol.clone(),
                side: spec.side.as_str().to_string(),
                timeframe: req.timeframe.as_str().to_string(),
            })
            .collect();
        let mut series = std::collections::BTreeMap::new();
        series.insert(
            "timestamps_ms".to_string(),
            miner_core::scan::primitives::raw_array::f64_slice_to_raw_array(&[]),
        );
        let raw = miner_core::findings::Raw { series };
        let result = Finding::Result(miner_core::findings::ResultFinding {
            schema_version: 1,
            scan_id_at_version: format!("{}@{}", req.scan_id, req.version),
            param_hash: req.param_hash.as_str().to_string(),
            code_revision: ctx.code_revision.to_string(),
            data_slice: DataSlice {
                range: req.sub_range.clone(),
                gap_manifest_ref: None,
                gap_manifest: None,
                sources,
            },
            dsr: None,
            fdr_q: None,
            run_id: ctx.run_id,
            produced_at_utc: Utc::now(),
            params: req.resolved_params.clone(),
            effect: miner_core::findings::Effect {
                metric: "stub_pair_metric".to_string(),
                value: 0.0,
                p_value: None,
                n: None,
                ci95: None,
                effect_size: None,
                extra: std::collections::BTreeMap::new(),
            },
            raw: Some(raw),
            repro: None,
        });
        sink.write_envelope(&result)?;
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

/// D4-02 + Plan 04-12 (CR-01 sibling regression gate) — Pair scan + two-leg
/// request preflights successfully AND reaches the kernel post-preflight.
///
/// Plan 04-02 shipped this test with a loose post-preflight assertion ("any
/// non-arity outcome is fine") because the engine's Pair-branch dispatch
/// was deferred to Plan 04-07 → 04-11 → 04-12. That looseness is exactly
/// what swallowed CR-01: the engine emitted a `Finding::ScanError` with
/// `"expected Pair arity (ctx.bars_pair is None)"` and this test accepted
/// it as "not an arity error, therefore fine".
///
/// Plan 04-12 tightens the assertion to require a `Finding::Result`
/// envelope — proof the dispatch actually reached the kernel via
/// `dispatch_pair_arity_body`. A `SyntheticCache` populates both legs so the
/// engine can load bars and run the scan body end-to-end. The `StubPair`
/// body additionally asserts `ctx.bars_pair.is_some()` as a negative pin.
#[test]
fn correct_arity_pair_scan_passes_arity_preflight() {
    // Populate both legs in the synthetic cache so the engine can load
    // bars through the production DukascopyReader + BarCache pipeline.
    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 11)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 22);

    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());

    let mut registry = Registry::new();
    registry.register(Box::new(StubPair));

    let start = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap();
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
    let outcome = run_one_with_registry(&req, &cfg, &reader, &mut sink, cancel, &registry)
        .expect("Pair-arity request must NOT trip preflight (D4-02) or any other engine Err arm");

    // Plan 04-12: assert the dispatch reached the kernel — the Pair body
    // must produce a Finding::Result envelope (NOT a Finding::ScanError
    // with the "expected Pair arity" message).
    assert_eq!(
        outcome,
        RunOutcome::Ok,
        "Pair-arity dispatch must return RunOutcome::Ok, NOT HadScanErrors (CR-01 sibling)"
    );

    let findings = common::parse_findings(&sink.0);

    // CR-01 negative pin: NO ScanError envelope with the bars_pair=None
    // message. If the engine regresses to hard-coded single-leg dispatch
    // this assertion fires.
    for f in &findings {
        if let Finding::ScanError(se) = f {
            assert!(
                !se.message.contains("expected Pair arity"),
                "CR-01 regression: engine emitted ScanError({:?}); Pair dispatch must reach the kernel",
                se.message
            );
        }
    }

    // Positive pin: at least one Finding::Result envelope reached the sink.
    let has_result = findings.iter().any(|f| matches!(f, Finding::Result(_)));
    assert!(
        has_result,
        "Pair-arity dispatch must produce at least one Finding::Result envelope post-preflight (Plan 04-12 tightening). Got envelopes: {:#?}",
        findings
            .iter()
            .map(|f| match f {
                Finding::RunStart(_) => "run_start",
                Finding::Result(_) => "result",
                Finding::ScanError(_) => "scan_error",
                Finding::GapAborted(_) => "gap_aborted",
                Finding::RunEnd(_) => "run_end",
                Finding::DryRun(_) => "dry_run",
                Finding::SweepSummary(_) => "sweep_summary",
            })
            .collect::<Vec<_>>()
    );
}

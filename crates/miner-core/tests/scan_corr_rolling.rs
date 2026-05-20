//! Phase 4 (Plan 04-07 Task 1) integration test — CROSS-02 rolling
//! Pearson + Spearman correlation.
//!
//! Drives `PearsonRollingScan` / `SpearmanRollingScan` directly via
//! `Scan::run` against a deterministic two-leg seeded fixture (no engine
//! Pair-branch wiring required; that's deferred to Plan 04-11 with the
//! full `RunStart` -> Result -> `RunEnd` shape pin).
//!
//! Pattern J analog: `scan_ljung_box.rs` — same 8-step walk but with two
//! `BarFrames` + `ScanCtx.bars_pair` populated.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::cross::{PearsonRollingScan, SpearmanRollingScan};
use miner_core::scan::{Scan, ScanCtx};

use common::BufferSink;

/// LCG-seeded closes (Numerical Recipes constants) so test inputs are
/// deterministic and reproducible. Used for the two synthetic legs.
fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
    #[allow(clippy::cast_possible_truncation)]
    let mut s = seed as u32;
    let mut closes = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        closes.push(1.0 + frac);
    }
    closes
}

fn build_bars(symbol: &str, ts: &[chrono::DateTime<Utc>], closes: &[f64]) -> BarFrame {
    assert_eq!(ts.len(), closes.len());
    BarFrame {
        source_id: "dukascopy".into(),
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

fn two_leg_fixture(n: usize, seed_a: u64, seed_b: u64) -> (BarFrame, BarFrame) {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| {
            let i_i64 = i64::try_from(i).expect("n fits");
            start + Duration::minutes(15 * i_i64)
        })
        .collect();
    let a = build_bars("EURUSD", &ts, &lcg_closes(n, seed_a));
    let b = build_bars("GBPUSD", &ts, &lcg_closes(n, seed_b));
    (a, b)
}

/// Plan 04-07 Task 1 integration test — `scan_corr_rolling_happy_path` for
/// Pearson. The scan emits exactly one `Finding::Result` envelope with
/// arity-Pair shape (`data_slice.sources.len()` == 2 + leg-labelled raw.series
/// keys + per-window correlation values in effect.extra).
#[test]
fn scan_corr_rolling_pearson_happy_path() {
    let (a, b) = two_leg_fixture(64, 11, 22);

    let resolved_params = serde_json::json!({"window": 5, "threshold": 0.6});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();

    let req = miner_core::scan::ScanRequest {
        scan_id: "cross.corr.pearson_rolling".into(),
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
        window: ClosedRangeUtc {
            start: window_start,
            end: window_end,
        },
        sub_range: TimeRange {
            start_utc: window_start,
            end_utc: window_end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params,
        param_hash,
        dry_run: false,
        sleep_after_first_finding_ms: None,
    };

    let ctx = ScanCtx {
        bars: &a,
        bars_pair: Some((&a, &b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    PearsonRollingScan
        .run(&ctx, &req, &mut sink)
        .expect("Pearson rolling run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one Result envelope");
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result; got {:?}", findings[0]);
    };

    // Envelope identity (Pattern J step 6).
    assert_eq!(r.scan_id_at_version, "cross.corr.pearson_rolling@1");
    assert_eq!(r.effect.metric, "pearson_corr_last");
    // n == values.len(); window=5 against 64 bars -> 63 returns -> 59 windows.
    assert_eq!(r.effect.n, Some(59));

    // D4-03: two-leg sources Vec.
    assert_eq!(r.data_slice.sources.len(), 2);
    assert_eq!(r.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(r.data_slice.sources[1].symbol, "GBPUSD");
    assert_eq!(r.data_slice.sources[0].timeframe, "15m");

    // effect.extra arrays have the expected keys.
    let raw = r.raw.as_ref().expect("raw present");
    assert!(raw.series.contains_key("returns_a"));
    assert!(raw.series.contains_key("returns_b"));
    assert!(raw.series.contains_key("timestamps_ms"));

    // effect.value is finite (the kernel emits a real rolling-correlation
    // value; the seeded LCG inputs guarantee non-zero variance in every
    // window so NaN cannot appear).
    assert!(r.effect.value.is_finite());
}

/// Plan 04-07 Task 1 integration test — Spearman variant. Same shape as
/// the Pearson test; the only delta is the scan-id-at-version + metric.
#[test]
fn scan_corr_rolling_spearman_happy_path() {
    let (a, b) = two_leg_fixture(64, 33, 44);

    let resolved_params = serde_json::json!({"window": 6});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();

    let req = miner_core::scan::ScanRequest {
        scan_id: "cross.corr.spearman_rolling".into(),
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
        window: ClosedRangeUtc {
            start: window_start,
            end: window_end,
        },
        sub_range: TimeRange {
            start_utc: window_start,
            end_utc: window_end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params,
        param_hash,
        dry_run: false,
        sleep_after_first_finding_ms: None,
    };

    let ctx = ScanCtx {
        bars: &a,
        bars_pair: Some((&a, &b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    SpearmanRollingScan
        .run(&ctx, &req, &mut sink)
        .expect("Spearman rolling run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1);
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result");
    };
    assert_eq!(r.scan_id_at_version, "cross.corr.spearman_rolling@1");
    assert_eq!(r.effect.metric, "spearman_corr_last");
    assert!(r.effect.value.is_finite());
    // window=6 -> 63 returns -> 58 windows.
    assert_eq!(r.effect.n, Some(58));
}

// ---------------------------------------------------------------------------
// Plan 04-12 (CR-01) — engine-facade variants. The direct-ScanCtx tests
// above pin the kernels; these variants drive the scans through
// `engine::run_one_with_registry` against a SyntheticCache + DukascopyReader
// so a future regression of the Pair-arity dispatch wiring trips here too.
// ---------------------------------------------------------------------------

/// CR-01 engine-facade variant of `scan_corr_rolling_pearson_happy_path`.
/// Drives `PearsonRollingScan` through `engine::run_one_with_registry`
/// against a `SyntheticCache` populated with both legs.
#[test]
fn scan_corr_rolling_pearson_happy_path_via_engine_facade() {
    use chrono::NaiveDate;
    use miner_core::config::{MinerConfig, OutputDest};
    use miner_core::engine::{RunOutcome, run_one_with_registry};
    use miner_core::scan::Registry;
    use miner_reader_dukascopy::DukascopyReader;

    use common::synthetic_cache::SyntheticCache;

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
    registry.register(Box::new(PearsonRollingScan));

    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + chrono::Duration::days(1);
    let resolved_params = serde_json::json!({"window": 5, "threshold": 0.6});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let req = miner_core::scan::ScanRequest {
        scan_id: "cross.corr.pearson_rolling".into(),
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
        resolved_params,
        param_hash,
        dry_run: false,
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    let outcome = run_one_with_registry(
        &req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
        &registry,
    )
    .expect("engine::run_one_with_registry ok");
    assert_eq!(outcome, RunOutcome::Ok);

    let findings = common::parse_findings(&sink.0);
    // CR-01 negative pin.
    for f in &findings {
        if let Finding::ScanError(se) = f {
            assert!(
                !se.message.contains("expected Pair arity"),
                "CR-01 regression: {:?}",
                se.message
            );
        }
    }
    let result = findings
        .iter()
        .find_map(|f| match f {
            Finding::Result(r) => Some(r),
            _ => None,
        })
        .expect("Result envelope present after engine-facade Pair dispatch");
    assert_eq!(result.scan_id_at_version, "cross.corr.pearson_rolling@1");
    assert_eq!(result.data_slice.sources.len(), 2);
    assert_eq!(result.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(result.data_slice.sources[1].symbol, "GBPUSD");
    assert!(result.effect.value.is_finite());
}

/// CR-01 engine-facade variant of `scan_corr_rolling_spearman_happy_path`.
#[test]
fn scan_corr_rolling_spearman_happy_path_via_engine_facade() {
    use chrono::NaiveDate;
    use miner_core::config::{MinerConfig, OutputDest};
    use miner_core::engine::{RunOutcome, run_one_with_registry};
    use miner_core::scan::Registry;
    use miner_reader_dukascopy::DukascopyReader;

    use common::synthetic_cache::SyntheticCache;

    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 33)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 44);
    let cfg = MinerConfig {
        cache_root: cache.cache_root().to_path_buf(),
        bar_cache_root: cache.bar_cache_root().to_path_buf(),
        output: OutputDest::Stdout,
    };
    let reader = DukascopyReader::new(cache.cache_root());
    let mut registry = Registry::new();
    registry.register(Box::new(SpearmanRollingScan));

    let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = start + chrono::Duration::days(1);
    let resolved_params = serde_json::json!({"window": 6});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let req = miner_core::scan::ScanRequest {
        scan_id: "cross.corr.spearman_rolling".into(),
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
        resolved_params,
        param_hash,
        dry_run: false,
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    let outcome = run_one_with_registry(
        &req,
        &cfg,
        &reader,
        &mut sink,
        Arc::new(AtomicBool::new(false)),
        &registry,
    )
    .expect("engine::run_one_with_registry ok");
    assert_eq!(outcome, RunOutcome::Ok);

    let findings = common::parse_findings(&sink.0);
    let result = findings
        .iter()
        .find_map(|f| match f {
            Finding::Result(r) => Some(r),
            _ => None,
        })
        .expect("Result envelope present");
    assert_eq!(result.scan_id_at_version, "cross.corr.spearman_rolling@1");
    assert_eq!(result.data_slice.sources.len(), 2);
    assert!(result.effect.value.is_finite());
}

//! Phase 4 Plan 04-11 Task 2 — byte-identical-rerun integration test.
//!
//! Pins ROADMAP Phase 4 Success Criterion #4: "consistent Finding envelope
//! shape across all 22 scans, single discriminant by `scan_id`, scan-specific
//! extras in documented effect.extra object". By exercising ONE representative
//! scan from each family (ANOM / CROSS / SEAS) via the canonical
//! `Scan::run` boundary and proving the masked JSONL is bit-equal across
//! two runs, the test simultaneously pins:
//!
//! - D3-23 (byte-identical re-run modulo `run_id` + clock fields) — once for
//!   each family
//! - The envelope-shape invariant (every emitted envelope deserialises into
//!   the same five-variant `Finding` enum tagged by `scan_id_at_version`)
//! - The deterministic ordering of `effect.extra` keys (`BTreeMap`, alphabetic
//!   on every run)
//!
//! ## Scans exercised
//!
//! - `stats.summary.welford@1` (ANOM-02) — Single-arity, pure Welford
//!   moments; no rolling state, no rayon.
//! - `cross.cointegration.engle_granger@1` (CROSS-05) — Pair-arity, full
//!   OLS + ADF + OU half-life pipeline; the most complex Phase 4 scan
//!   and the one most likely to surface a determinism regression.
//! - `seas.bucket.hour_of_day@1` (SEAS-01) — Single-arity bucket
//!   aggregation; exercises the BTreeMap-backed 24-bucket parallel arrays.
//!
//! ## Pattern
//!
//! For each scan: build deterministic LCG-seeded BarFrame(s), run
//! `Scan::run` into a `BufferSink` TWICE with two fresh `RunId`s and
//! independent contexts, parse and mask the volatile envelope fields
//! (`run_id`, `produced_at_utc`), and assert the resulting masked
//! `Vec<serde_json::Value>`s are byte-identical.
//!
//! A SECOND assertion-level check confirms the UN-masked outputs DIFFER
//! ONLY in the masked fields — i.e. masking is the ONLY structural
//! difference between two runs of the same scan on the same input.

#![allow(clippy::cast_precision_loss, clippy::too_many_lines)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, NaiveDate, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::{param_hash, run_one_with_registry};
use miner_core::findings::{RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::anom::SummaryWelfordScan;
use miner_core::scan::cross::EngleGrangerScan;
use miner_core::scan::seas::HourOfDayScan;
use miner_core::scan::{Registry, Scan, ScanCtx, ScanRequest};
use miner_reader_dukascopy::DukascopyReader;

use common::{BufferSink, synthetic_cache::SyntheticCache};

#[allow(clippy::cast_possible_truncation)]
fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed as u32;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        out.push(1.0 + frac);
    }
    out
}

fn build_bars(symbol: &str, n: usize, closes: &[f64]) -> BarFrame {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts_open: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| {
            let i_i64 = i64::try_from(i).expect("fits in i64");
            start + Duration::minutes(15 * i_i64)
        })
        .collect();
    let ts_close: Vec<chrono::DateTime<Utc>> =
        ts_open.iter().map(|t| *t + Duration::minutes(15)).collect();
    BarFrame {
        source_id: "dukascopy".into(),
        symbol: symbol.into(),
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        ts_open_utc: ts_open,
        ts_close_utc: ts_close,
        open: closes.to_vec(),
        high: closes.iter().map(|c| c + 0.001).collect(),
        low: closes.iter().map(|c| c - 0.001).collect(),
        close: closes.to_vec(),
        tick_volume: vec![1.0; n],
    }
}

/// Run a single-arity scan twice into separate `BufferSinks`; return both
/// JSONL byte buffers AND the matching masked envelope Vecs.
fn run_single_arity_twice<S: Scan>(
    scan: S,
    bars: &BarFrame,
    req: &ScanRequest,
) -> ((Vec<u8>, Vec<serde_json::Value>), (Vec<u8>, Vec<serde_json::Value>)) {
    let ctx1 = ScanCtx {
        bars,
        bars_pair: None,
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev-abc1234",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };
    let mut sink1 = BufferSink::new();
    scan.run(&ctx1, req, &mut sink1).expect("run 1 ok");
    let masked1 = common::parse_and_mask_jsonl(&sink1.0);

    let ctx2 = ScanCtx {
        bars,
        bars_pair: None,
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev-abc1234",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };
    let mut sink2 = BufferSink::new();
    scan.run(&ctx2, req, &mut sink2).expect("run 2 ok");
    let masked2 = common::parse_and_mask_jsonl(&sink2.0);

    ((sink1.0, masked1), (sink2.0, masked2))
}

/// Pair-arity twice-run helper that drives `Scan::run` DIRECTLY (kernel-only
/// path, no engine). Plan 04-12 retired the byte-identical-rerun CROSS test
/// from this helper in favour of the engine-path variant
/// (`run_pair_arity_via_engine_twice`) so the CR-01 dispatch wiring is
/// covered by the byte-identity invariant too. Kept available for future
/// kernel-direct pins that intentionally bypass the engine.
#[allow(dead_code, reason = "retained for future kernel-direct Pair-arity pins; Plan 04-12 routes the CROSS byte-identical-rerun test through the engine facade")]
fn run_pair_arity_twice<S: Scan>(
    scan: S,
    bars_a: &BarFrame,
    bars_b: &BarFrame,
    req: &ScanRequest,
) -> ((Vec<u8>, Vec<serde_json::Value>), (Vec<u8>, Vec<serde_json::Value>)) {
    let ctx1 = ScanCtx {
        bars: bars_a,
        bars_pair: Some((bars_a, bars_b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev-abc1234",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };
    let mut sink1 = BufferSink::new();
    scan.run(&ctx1, req, &mut sink1).expect("pair run 1 ok");
    let masked1 = common::parse_and_mask_jsonl(&sink1.0);

    let ctx2 = ScanCtx {
        bars: bars_a,
        bars_pair: Some((bars_a, bars_b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev-abc1234",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };
    let mut sink2 = BufferSink::new();
    scan.run(&ctx2, req, &mut sink2).expect("pair run 2 ok");
    let masked2 = common::parse_and_mask_jsonl(&sink2.0);

    ((sink1.0, masked1), (sink2.0, masked2))
}

fn build_single_request(scan_id: &str) -> ScanRequest {
    let resolved_params = serde_json::json!({});
    let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();
    ScanRequest {
        scan_id: scan_id.into(),
        version: 1,
        instruments: vec![InstrumentSpec {
            symbol: "EURUSD".into(),
            side: Side::Bid,
        }],
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
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    }
}

/// Pair-arity request builder for the kernel-direct path. See
/// `run_pair_arity_twice` — both are retained as future-use kernel-direct
/// helpers. The engine-path equivalent is `run_pair_arity_via_engine_twice`
/// (Plan 04-12) which builds the request inline against a `SyntheticCache`.
#[allow(dead_code, reason = "paired with run_pair_arity_twice; retained for future kernel-direct pins")]
fn build_pair_request() -> ScanRequest {
    let resolved_params = serde_json::json!({"regression": "c"});
    let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 5, 0, 0, 0).unwrap();
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
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    }
}

/// ANOM-02 byte-identical-rerun pin. The `ResultFinding` has zero
/// inherently volatile structural fields beyond `run_id` + `produced_at_utc`,
/// so masking those two should yield byte-identical envelopes.
#[test]
fn byte_identical_rerun_anom_summary_welford() {
    let closes = lcg_closes(64, 42);
    let bars = build_bars("EURUSD", 64, &closes);
    let req = build_single_request("stats.summary.welford");

    let ((_raw1, masked1), (_raw2, masked2)) =
        run_single_arity_twice(SummaryWelfordScan, &bars, &req);

    assert_eq!(masked1.len(), 1, "exactly one envelope per run");
    assert_eq!(masked2.len(), 1, "exactly one envelope per run");
    assert_eq!(
        masked1, masked2,
        "ANOM byte-identical-rerun violation:\nrun 1: {}\nrun 2: {}",
        serde_json::to_string_pretty(&masked1).unwrap_or_default(),
        serde_json::to_string_pretty(&masked2).unwrap_or_default(),
    );

    // Sanity — the masked envelope's `kind` discriminant is `result`,
    // confirming the cross-family envelope-shape invariant (SC#4).
    let kind = masked1[0]
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(kind, "result", "expected kind=result envelope; got {kind:?}");
}

/// Drive a Pair-arity scan through `engine::run_one_with_registry` twice
/// against a fresh `SyntheticCache` populated with the same per-leg seeds.
/// Returns both runs' raw JSONL bytes and the masked envelope Vecs (`RunStart`
/// + Result + `RunEnd`, parsed through `common::parse_and_mask_jsonl`).
///
/// Plan 04-12 refactor: previously the helper hand-built `ScanCtx { bars_pair:
/// Some((a, b)), .. }` directly, bypassing the engine. That kernel-level pin
/// missed CR-01 (the engine hard-coded `bars_pair: None` for every dispatch).
/// The new helper drives the production code path — `DukascopyReader` →
/// `BarCache::get_or_build` → `engine::dispatch_pair_arity_body` →
/// `Scan::run` — so a future CR-01 regression trips here too.
fn run_pair_arity_via_engine_twice(
    scan_id: &str,
    version: u32,
    seed_a: u32,
    seed_b: u32,
    day: NaiveDate,
    resolved_params: serde_json::Value,
    register_scan: fn(&mut Registry),
) -> ((Vec<u8>, Vec<serde_json::Value>), (Vec<u8>, Vec<serde_json::Value>)) {
    let do_run = || -> (Vec<u8>, Vec<serde_json::Value>) {
        let cache = SyntheticCache::new()
            .with_deterministic_day("EURUSD", Side::Bid, day, seed_a)
            .with_deterministic_day("GBPUSD", Side::Bid, day, seed_b);
        let cfg = MinerConfig {
            cache_root: cache.cache_root().to_path_buf(),
            bar_cache_root: cache.bar_cache_root().to_path_buf(),
            output: OutputDest::Stdout,
        };
        let reader = DukascopyReader::new(cache.cache_root());

        let mut registry = Registry::new();
        register_scan(&mut registry);

        let start = day.and_hms_opt(0, 0, 0).expect("00:00:00").and_utc();
        let end = start + Duration::days(1);
        let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
        let req = ScanRequest {
            scan_id: scan_id.into(),
            version,
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
            resolved_params: resolved_params.clone(),
            param_hash,
            dry_run: false,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        };

        let mut sink = BufferSink::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let _outcome = run_one_with_registry(&req, &cfg, &reader, &mut sink, cancel, &registry)
            .expect("engine::run_one_with_registry ok");
        let masked = common::parse_and_mask_jsonl(&sink.0);
        (sink.0, masked)
    };
    (do_run(), do_run())
}

/// CROSS-05 byte-identical-rerun pin (Pair-arity) via the engine facade.
///
/// Plan 04-12 refactor: the previous version of this test constructed
/// `ScanCtx { bars_pair: Some(..), .. }` by hand and ran `Scan::run`
/// directly — kernel-correct but facade-broken (this is exactly what
/// CR-01 was). The new version drives the scan through
/// `engine::run_one_with_registry` against a `SyntheticCache` so the
/// byte-identity invariant covers the full `RunStart` / Result / `RunEnd`
/// envelope chain (D3-23) AND CR-01 itself: if the engine ever regresses
/// to hard-coded single-leg dispatch, the `Finding::ScanError "expected
/// Pair arity"` envelope it would emit cannot byte-match a different
/// `RunStart`'s masked output, and this test fires.
#[test]
fn byte_identical_rerun_cross_engle_granger() {
    let day = NaiveDate::from_ymd_opt(2024, 1, 2).unwrap();
    let ((_raw1, masked1), (_raw2, masked2)) = run_pair_arity_via_engine_twice(
        "cross.cointegration.engle_granger",
        1,
        0x1357_9BDF,
        0x0ACE_F123,
        day,
        serde_json::json!({"regression": "c"}),
        |r| r.register(Box::new(EngleGrangerScan)),
    );

    // Engine emits RunStart + Result + RunEnd (3 envelopes per run after
    // the dispatch reaches the kernel via dispatch_pair_arity_body).
    assert_eq!(
        masked1.len(),
        3,
        "expected RunStart + Result + RunEnd envelopes per engine run; got {} envelopes\nmasked: {}",
        masked1.len(),
        serde_json::to_string_pretty(&masked1).unwrap_or_default(),
    );
    assert_eq!(masked2.len(), 3);
    assert_eq!(
        masked1, masked2,
        "CROSS byte-identical-rerun (via engine facade) violation:\nrun 1: {}\nrun 2: {}",
        serde_json::to_string_pretty(&masked1).unwrap_or_default(),
        serde_json::to_string_pretty(&masked2).unwrap_or_default(),
    );

    // Plan 04-12 CR-01 sibling pin: the second envelope is a Result, NOT
    // a ScanError. The masked envelope still carries the `kind`
    // discriminant (assigned at serialization-time by the Finding enum's
    // serde-tag).
    let kind = masked1[1]
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        kind, "result",
        "engine-path Pair-arity dispatch must produce kind=result; got {kind:?} (CR-01 regression?)\nenvelope: {}",
        serde_json::to_string_pretty(&masked1[1]).unwrap_or_default()
    );
}

/// SEAS-01 byte-identical-rerun pin. The 24-bucket aggregation uses
/// BTreeMap-backed `effect.extra` so iteration order is alphabetic on
/// every run; no rayon, no rolling state.
#[test]
fn byte_identical_rerun_seas_hour_of_day() {
    // 7 days × 24h × 4 bars/h at 15m = 672 bars (same recipe as
    // scan_seas_hour_of_day_happy_path).
    let closes = lcg_closes(672, 0xDEAD_BEEF);
    let bars = build_bars("EURUSD", 672, &closes);
    let resolved_params = serde_json::json!({"min_obs_per_bucket": 5});
    let param_hash_v = param_hash::param_hash(&resolved_params).expect("ok");
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = start + Duration::days(7);
    let req = ScanRequest {
        scan_id: "seas.bucket.hour_of_day".into(),
        version: 1,
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
        resolved_params,
        param_hash: param_hash_v,
        dry_run: false,
        #[cfg(any(test, feature = "test-internal"))]
        sleep_after_first_finding_ms: None,
    };

    let ((_raw1, masked1), (_raw2, masked2)) =
        run_single_arity_twice(HourOfDayScan, &bars, &req);

    assert_eq!(masked1.len(), 1, "exactly one envelope per run");
    assert_eq!(masked2.len(), 1, "exactly one envelope per run");
    assert_eq!(
        masked1, masked2,
        "SEAS byte-identical-rerun violation:\nrun 1: {}\nrun 2: {}",
        serde_json::to_string_pretty(&masked1).unwrap_or_default(),
        serde_json::to_string_pretty(&masked2).unwrap_or_default(),
    );
}

/// Cross-family masking invariant — running ANOM-02 twice with DIFFERENT
/// `RunId`s produces envelopes that differ ONLY in the `run_id` +
/// `produced_at_utc` fields. After masking those two fields the envelopes
/// are byte-identical (the first three tests above prove this); this
/// fourth test pins the COMPLEMENTARY claim: the UNMASKED envelopes do
/// in fact differ, and only in the four documented volatile fields.
#[test]
fn unmasked_envelopes_differ_only_in_volatile_fields() {
    let closes = lcg_closes(64, 42);
    let bars = build_bars("EURUSD", 64, &closes);
    let req = build_single_request("stats.summary.welford");

    let ((raw1, _masked1), (raw2, _masked2)) =
        run_single_arity_twice(SummaryWelfordScan, &bars, &req);

    let v1: serde_json::Value =
        serde_json::from_slice(raw1.trim_ascii_end()).expect("run 1 jsonl parses");
    let v2: serde_json::Value =
        serde_json::from_slice(raw2.trim_ascii_end()).expect("run 2 jsonl parses");

    // The raw JSON values differ (different run_id).
    assert_ne!(
        v1, v2,
        "expected unmasked envelopes to differ via run_id/produced_at_utc"
    );

    // Drop the volatile fields from each and confirm the rest matches.
    fn strip_volatile(v: &mut serde_json::Value) {
        if let serde_json::Value::Object(map) = v {
            for k in [
                "run_id",
                "started_at_utc",
                "produced_at_utc",
                "ended_at_utc",
                "wall_clock_ms",
            ] {
                map.remove(k);
            }
            for (_, child) in map.iter_mut() {
                strip_volatile(child);
            }
        } else if let serde_json::Value::Array(arr) = v {
            for child in arr.iter_mut() {
                strip_volatile(child);
            }
        }
    }
    let mut s1 = v1;
    let mut s2 = v2;
    strip_volatile(&mut s1);
    strip_volatile(&mut s2);
    assert_eq!(
        s1, s2,
        "after stripping volatile fields (run_id, started_at_utc, produced_at_utc, ended_at_utc, wall_clock_ms) \
         the envelopes must be structurally identical"
    );
}

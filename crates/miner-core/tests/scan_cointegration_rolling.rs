//! RAD-3626 integration test — CROSS-06 rolling Engle-Granger cointegration
//! scan with breakdown detection.
//!
//! Drives `CointegrationRollingScan` directly via `Scan::run` against two-leg
//! deterministic fixtures. Pattern analog: `scan_ols_rolling.rs`.
//!
//! Coverage:
//! - (a) a synthetic cointegrated pair with an injected structural break
//!   mid-series flips `breakdown=true` ONLY after the break window;
//! - (b) a persistently cointegrated pair never trips the flag;
//! - (c) determinism — two `Scan::run` passes over identical inputs produce
//!   byte-identical masked envelopes (`scan_facade_determinism` style);
//! - (4) an `insta` snapshot of the finding schema (consumers parse it).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::float_cmp
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RawArray, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::cross::CointegrationRollingScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const WINDOW: usize = 60;
const STEP: usize = 10;

fn make_ts(n: usize) -> Vec<chrono::DateTime<Utc>> {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    (0..n)
        .map(|i| start + Duration::minutes(15 * i64::try_from(i).expect("n fits")))
        .collect()
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

/// Deterministic LCG random-walk increments (mean-zero, scaled).
fn walk_increments(n: usize, seed: u64, scale: f64) -> Vec<f64> {
    let mut s = seed as u32;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        out.push((f64::from(s) / f64::from(u32::MAX) - 0.5) * scale);
    }
    out
}

/// Stationary AR(1) residual series (phi = 0.3) — strongly mean-reverting.
fn ar1_noise(n: usize, seed: u64, scale: f64) -> Vec<f64> {
    let mut s = seed as u32;
    let mut prev = 0.0_f64;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let eta = (f64::from(s) / f64::from(u32::MAX) - 0.5) * scale;
        let v = 0.3_f64 * prev + eta;
        out.push(v);
        prev = v;
    }
    out
}

/// `b` = random walk; `a` = `b` + stationary AR(1) noise everywhere (a
/// persistently cointegrated pair).
fn cointegrated_pair(n: usize, seed_b: u64, seed_e: u64) -> (Vec<f64>, Vec<f64>) {
    let inc = walk_increments(n, seed_b, 0.02);
    let mut b = Vec::with_capacity(n);
    let mut acc = 1.0_f64;
    for d in inc {
        acc += d;
        b.push(acc);
    }
    let e = ar1_noise(n, seed_e, 0.01);
    let a: Vec<f64> = b.iter().zip(e.iter()).map(|(bi, ei)| bi + ei).collect();
    (a, b)
}

/// Cointegrated up to `break_idx`, then `a` decouples into its OWN independent
/// random walk (the structural break destroys cointegration).
fn structural_break_pair(
    n: usize,
    break_idx: usize,
    seed_b: u64,
    seed_e: u64,
    seed_break: u64,
) -> (Vec<f64>, Vec<f64>) {
    let (mut a, b) = cointegrated_pair(n, seed_b, seed_e);
    // Replace a[break_idx..] with an independent random walk continuing from
    // a[break_idx - 1].
    let inc = walk_increments(n, seed_break, 0.02);
    let mut acc = a[break_idx.saturating_sub(1)];
    for (i, d) in inc.iter().enumerate().take(n).skip(break_idx) {
        acc += d;
        a[i] = acc;
    }
    (a, b)
}

fn request(params: serde_json::Value) -> ScanRequest {
    let resolved = params;
    let param_hash = param_hash::param_hash(&resolved).expect("hash ok");
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
    ScanRequest {
        scan_id: "cross.cointegration.rolling".into(),
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
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
        sleep_after_first_finding_ms: None,
    }
}

fn ctx<'a>(a: &'a BarFrame, b: &'a BarFrame, cancel: Arc<AtomicBool>) -> ScanCtx<'a> {
    ScanCtx {
        bars: a,
        bars_pair: Some((a, b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev-rad3626",
        cancel,
        sleep_after_first_finding_ms: None,
    }
}

fn decode_f64(arr: &RawArray) -> Vec<f64> {
    let bytes = &arr.data.0;
    assert_eq!(bytes.len() % 8, 0);
    bytes
        .chunks_exact(8)
        .map(|c| {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(c);
            f64::from_le_bytes(buf)
        })
        .collect()
}

fn run_scan(a: &BarFrame, b: &BarFrame, params: serde_json::Value) -> Vec<Finding> {
    let req = request(params);
    let c = ctx(a, b, Arc::new(AtomicBool::new(false)));
    let mut sink = BufferSink::new();
    CointegrationRollingScan
        .run(&c, &req, &mut sink)
        .expect("scan run ok");
    common::parse_findings(&sink.0)
}

// ---------------------------------------------------------------------------
// (a) structural break flips breakdown only AFTER the break
// ---------------------------------------------------------------------------

#[test]
fn structural_break_trips_breakdown_only_after_break() {
    let n = 400;
    let break_idx = 200;
    let (a_closes, b_closes) =
        structural_break_pair(n, break_idx, 0x1357_9BDF, 0x0ACE_F123, 0xBEEF);
    let ts = make_ts(n);
    let a = build_bars("EURUSD", &ts, &a_closes);
    let b = build_bars("GBPUSD", &ts, &b_closes);

    let findings = run_scan(&a, &b, serde_json::json!({"window": WINDOW, "step": STEP}));
    assert_eq!(findings.len(), 1);
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result");
    };

    let flags = decode_f64(&r.effect.extra["breakdown_flags"]);
    let reasons = decode_f64(&r.effect.extra["breakdown_reason_codes"]);
    let n_windows = flags.len();
    assert_eq!(n_windows, (n - WINDOW) / STEP + 1);

    // Window i covers aligned indices [i*STEP, i*STEP + WINDOW). Since the
    // fixture is a full gap-free series, indices map directly.
    for (i, &flag) in flags.iter().enumerate() {
        let end_idx = i * STEP + WINDOW - 1;
        if end_idx < break_idx {
            // Fully pre-break window — must NOT flag breakdown.
            assert_eq!(
                flag,
                0.0,
                "fully pre-break window {i} (end_idx={end_idx}) must not trip breakdown; \
                 adf_p={:?}",
                decode_f64(&r.effect.extra["adf_p_values"])[i]
            );
        }
    }

    // At least one fully post-break window (start_idx >= break_idx) must trip.
    let post_break_tripped = flags.iter().enumerate().any(|(i, &flag)| {
        let start_idx = i * STEP;
        start_idx >= break_idx && flag == 1.0
    });
    assert!(
        post_break_tripped,
        "expected at least one post-break window to trip breakdown; flags={flags:?} reasons={reasons:?}"
    );

    // Headline effect.value == breakdown count > 0.
    assert!(r.effect.value > 0.0, "breakdown count must be > 0");
    let tripped = flags.iter().filter(|f| **f == 1.0).count() as f64;
    assert_eq!(
        r.effect.value, tripped,
        "effect.value must equal flag count"
    );
    // Every tripped window has a non-zero reason code.
    for (i, &flag) in flags.iter().enumerate() {
        if flag == 1.0 {
            assert!(
                reasons[i] > 0.0,
                "tripped window {i} must carry a reason code"
            );
        } else {
            assert_eq!(reasons[i], 0.0, "untripped window {i} must have reason 0");
        }
    }
}

// ---------------------------------------------------------------------------
// (b) persistently cointegrated pair never trips
// ---------------------------------------------------------------------------

#[test]
fn persistently_cointegrated_never_trips_breakdown() {
    let n = 400;
    let (a_closes, b_closes) = cointegrated_pair(n, 0x2468_ACE0, 0x1111_2222);
    let ts = make_ts(n);
    let a = build_bars("EURUSD", &ts, &a_closes);
    let b = build_bars("GBPUSD", &ts, &b_closes);

    let findings = run_scan(&a, &b, serde_json::json!({"window": WINDOW, "step": STEP}));
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result");
    };
    let flags = decode_f64(&r.effect.extra["breakdown_flags"]);
    assert!(!flags.is_empty());
    let count = flags.iter().filter(|f| **f == 1.0).count();
    assert_eq!(
        count,
        0,
        "persistently cointegrated pair must never trip breakdown; flags={flags:?} \
         adf_p={:?}",
        decode_f64(&r.effect.extra["adf_p_values"])
    );
    assert_eq!(r.effect.value, 0.0);
}

// ---------------------------------------------------------------------------
// (c) determinism — twice-run masked byte-equality
// ---------------------------------------------------------------------------

#[test]
fn twice_run_byte_identical_when_volatile_fields_masked() {
    let n = 240;
    let (a_closes, b_closes) = cointegrated_pair(n, 0xDEAD_BEEF, 0xFEED_FACE);
    let ts = make_ts(n);
    let a = build_bars("EURUSD", &ts, &a_closes);
    let b = build_bars("GBPUSD", &ts, &b_closes);
    let params = serde_json::json!({"window": WINDOW, "step": STEP});

    let f1 = run_scan(&a, &b, params.clone());
    let f2 = run_scan(&a, &b, params);

    let mut v1 = serde_json::to_value(&f1).expect("v1");
    let mut v2 = serde_json::to_value(&f2).expect("v2");
    common::mask_volatile_fields(&mut v1);
    common::mask_volatile_fields(&mut v2);
    assert_eq!(
        v1,
        v2,
        "masked envelopes from two runs differ:\n{}\n{}",
        serde_json::to_string_pretty(&v1).unwrap_or_default(),
        serde_json::to_string_pretty(&v2).unwrap_or_default(),
    );
}

// ---------------------------------------------------------------------------
// (4) insta snapshot of the finding SCHEMA
//
// Consumers parse the finding, so the SHAPE (keys, nesting, dtypes, array
// lengths) must be stable. We snapshot a schema view that redacts the
// computed floats (`value`), the params blake3 hash (`param_hash`), the
// volatile envelope fields (`run_id` / `produced_at_utc`) and the raw f64
// array bytes (`data`) — keeping every key, `dtype`, and `shape`. This pins
// the contract without coupling the snapshot to platform-specific float bits.
// ---------------------------------------------------------------------------

/// Replace value-bearing leaves with stable placeholders, preserving the
/// schema (keys / dtype / shape / array lengths).
fn redact_schema(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            // A RawArray is exactly {data, dtype, shape}: redact the bytes to a
            // length-tagged placeholder, keep dtype + shape.
            if map.contains_key("data") && map.contains_key("dtype") && map.contains_key("shape") {
                let len: u64 = map.get("shape").and_then(|s| s.as_array()).map_or(0, |a| {
                    a.iter().filter_map(serde_json::Value::as_u64).product()
                });
                map.insert(
                    "data".to_string(),
                    serde_json::Value::String(format!("<f64 x {len}>")),
                );
                return;
            }
            for (k, child) in map.iter_mut() {
                if (k == "value" && child.is_number()) || (k == "param_hash" && child.is_string()) {
                    *child = serde_json::Value::String(format!("<{k}>"));
                } else {
                    redact_schema(child);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for child in arr.iter_mut() {
                redact_schema(child);
            }
        }
        _ => {}
    }
}

#[test]
fn cointegration_rolling_schema_snapshot() {
    // Small deterministic fixture so the snapshot stays compact: n=120,
    // window=60, step=20 -> 4 windows.
    let n = 120;
    let (a_closes, b_closes) = cointegrated_pair(n, 0x00C0_FFEE, 0x00BA_DA55);
    let ts = make_ts(n);
    let a = build_bars("EURUSD", &ts, &a_closes);
    let b = build_bars("GBPUSD", &ts, &b_closes);

    let findings = run_scan(&a, &b, serde_json::json!({"window": 60, "step": 20}));
    assert_eq!(findings.len(), 1);
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result");
    };
    assert_eq!(r.scan_id_at_version, "cross.cointegration.rolling@1");
    assert_eq!(r.effect.n, Some(4));

    let mut schema = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut schema);
    redact_schema(&mut schema);
    insta::assert_json_snapshot!("cointegration_rolling_schema", schema);
}

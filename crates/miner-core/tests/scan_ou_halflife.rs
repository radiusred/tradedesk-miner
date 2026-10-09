//! RAD-3627 — `stats.meanrev.ou_halflife@1` single-leg OU half-life scan
//! integration test.
//!
//! Pattern analog: `crates/miner-core/tests/scan_adf.rs` (sibling single-leg
//! ANOM-family integration test). Pins the scan SCHEMA via an `insta`
//! snapshot and the half-life recovery / sentinel behaviour via numeric
//! acceptance asserts (acceptance criteria 2 + 4).

#![allow(clippy::cast_precision_loss, clippy::too_many_lines, clippy::float_cmp)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RawArray, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::anom::OuHalfLifeScan;
use miner_core::scan::{Scan, ScanArity, ScanCtx, ScanRequest};

use common::BufferSink;

/// Exact AR(1) decay-to-mean series `y_t = μ + φ^t·(y_0 - μ)`. The points are
/// perfectly collinear in `(y_{t-1}, Δy)` space so OLS recovers `ρ = φ - 1`
/// (hence `half_life = -ln2/ln φ`) to floating-point tolerance.
fn ar1_decay_closes(phi: f64, n: usize) -> Vec<f64> {
    let mu = 1.5_f64;
    let y0 = 2.0_f64;
    (0..n)
        .map(|t| {
            let tt = i32::try_from(t).expect("fits");
            mu + phi.powi(tt) * (y0 - mu)
        })
        .collect()
}

fn build_bar_frame_from_closes(close: &[f64]) -> BarFrame {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let n = close.len();
    let ts_open: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| {
            let i_i64 = i64::try_from(i).expect("fits in i64");
            start + Duration::minutes(15 * i_i64)
        })
        .collect();
    let ts_close: Vec<chrono::DateTime<Utc>> =
        ts_open.iter().map(|t| *t + Duration::minutes(15)).collect();
    let opens: Vec<f64> = close.to_vec();
    let highs: Vec<f64> = close.iter().map(|c| c + 0.001).collect();
    let lows: Vec<f64> = close.iter().map(|c| c - 0.001).collect();
    let vols = vec![1.0; n];
    BarFrame {
        source_id: "dukascopy".into(),
        symbol: "EURUSD".into(),
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        ts_open_utc: ts_open,
        ts_close_utc: ts_close,
        open: opens,
        high: highs,
        low: lows,
        close: close.to_vec(),
        tick_volume: vols,
    }
}

fn request(params: serde_json::Value) -> ScanRequest {
    let param_hash = param_hash::param_hash(&params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 4, 0, 0, 0).unwrap();
    ScanRequest {
        scan_id: "stats.meanrev.ou_halflife".into(),
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
        resolved_params: params,
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

fn ctx(bars: &BarFrame) -> ScanCtx<'_> {
    ScanCtx {
        bars,
        bars_pair: None,
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev-abc1234",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    }
}

fn decode_f64(arr: &RawArray) -> f64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&arr.data.0[0..8]);
    f64::from_le_bytes(buf)
}

fn run_one(params: serde_json::Value, closes: &[f64]) -> miner_core::findings::ResultFinding {
    let bars = build_bar_frame_from_closes(closes);
    let mut sink = BufferSink::new();
    OuHalfLifeScan
        .run(&ctx(&bars), &request(params), &mut sink)
        .expect("scan ok");
    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope");
    let Finding::Result(r) = findings.into_iter().next().unwrap() else {
        panic!("expected Result");
    };
    r
}

/// Acceptance #4 — `insta` snapshot of the scan SCHEMA (surface contract).
/// Schema-only (no computed floats) so it pins the dispatch surface
/// deterministically. The numeric behaviour is pinned by the asserts below.
#[test]
fn scan_ou_halflife_schema() {
    let scan = OuHalfLifeScan;
    assert_eq!(scan.arity(), ScanArity::Single);
    let shape = scan.finding_fields();
    let schema = serde_json::json!({
        "scan_id": scan.id(),
        "version": scan.version(),
        "arity": "Single",
        "effect_metric": "ou_lambda",
        "effect_size_kind": "ou_lambda",
        "effect_value": "lambda (OU mean-reversion rate; half_life=ln2/lambda lives in extra, INFINITY-safe)",
        "effect_extra_keys": shape.effect_extra_keys,
        "raw_series_keys": shape.raw_series_keys,
        "params": {
            "on": {"type": "string", "default": "level", "enum": ["level", "returns"]},
            "min_n": {"type": "integer", "default": 30, "minimum": 3}
        }
    });
    insta::assert_json_snapshot!("ou_halflife_schema", schema);
}

/// Acceptance #2 (part 1) — synthetic AR(1) with φ = 0.5: recovered half-life
/// matches `ln2/(-ln φ) = 1.0` within tolerance, and λ = ln2.
#[test]
fn scan_ou_halflife_recovers_known_phi() {
    let r = run_one(serde_json::json!({}), &ar1_decay_closes(0.5, 40));
    assert_eq!(r.scan_id_at_version, "stats.meanrev.ou_halflife@1");
    assert_eq!(r.effect.metric, "ou_lambda");

    let half_life = decode_f64(&r.effect.extra["half_life"]);
    let expected = std::f64::consts::LN_2 / (-0.5_f64.ln()); // = 1.0
    assert!(
        (half_life - expected).abs() < 1e-6,
        "half_life = {half_life} expected {expected}"
    );
    let phi = decode_f64(&r.effect.extra["ar1_coeff"]);
    assert!((phi - 0.5).abs() < 1e-9, "φ = {phi} expected 0.5");
    assert!(
        (r.effect.value - std::f64::consts::LN_2).abs() < 1e-6,
        "λ = {} expected ln2",
        r.effect.value
    );
}

/// Acceptance #2 (part 2) — an explosive (φ > 1) / non-mean-reverting series
/// yields the INFINITY half-life sentinel and λ = 0.
#[test]
fn scan_ou_halflife_non_mean_reverting_is_infinity() {
    let r = run_one(serde_json::json!({}), &ar1_decay_closes(1.5, 30));
    let half_life = decode_f64(&r.effect.extra["half_life"]);
    assert!(
        half_life.is_infinite(),
        "half_life = {half_life} expected INFINITY sentinel"
    );
    assert_eq!(r.effect.value, 0.0, "λ sentinel = 0 for non-mean-reverting");
}

/// The `on=returns` basis dispatches and echoes the basis label.
#[test]
fn scan_ou_halflife_returns_basis() {
    let r = run_one(
        serde_json::json!({"on": "returns"}),
        &ar1_decay_closes(0.5, 40),
    );
    let on_bytes = &r.effect.extra["on"].data.0;
    assert_eq!(std::str::from_utf8(on_bytes).unwrap(), "returns");
}

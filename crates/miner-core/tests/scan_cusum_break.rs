//! RAD-3841 — `stats.regime.cusum_break@1` CUSUM structural-break /
//! regime-detection scan integration test.
//!
//! Pattern analog: `crates/miner-core/tests/scan_ou_halflife.rs` (sibling
//! single-leg ANOM-family integration test). Pins the scan SCHEMA via an
//! `insta` snapshot (acceptance #4) and the changepoint-recovery / stationary
//! behaviour via numeric acceptance asserts (acceptance #2).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
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
use miner_core::scan::anom::CusumBreakScan;
use miner_core::scan::{Scan, ScanArity, ScanCtx, ScanRequest};

use common::BufferSink;

/// Deterministic LCG noise in `[-0.5, 0.5)·scale` (mean ~0).
fn noise(n: usize, seed: u32, scale: f64) -> Vec<f64> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (f64::from(s) / f64::from(u32::MAX) - 0.5) * scale
        })
        .collect()
}

/// Build a close series whose log returns are exactly `returns`
/// (`c_0 = 1.0`, `c_t = c_{t-1}·exp(r_t)`).
fn closes_from_returns(returns: &[f64]) -> Vec<f64> {
    let mut c = 1.0_f64;
    let mut out = Vec::with_capacity(returns.len() + 1);
    out.push(c);
    for r in returns {
        c *= r.exp();
        out.push(c);
    }
    out
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
    let window_end = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
    ScanRequest {
        scan_id: "stats.regime.cusum_break".into(),
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

fn run_breaks(
    params: serde_json::Value,
    closes: &[f64],
) -> Vec<miner_core::findings::ResultFinding> {
    let bars = build_bar_frame_from_closes(closes);
    let mut sink = BufferSink::new();
    CusumBreakScan
        .run(&ctx(&bars), &request(params), &mut sink)
        .expect("scan ok");
    common::parse_findings(&sink.0)
        .into_iter()
        .map(|f| match f {
            Finding::Result(r) => r,
            other => panic!("expected Result; got {other:?}"),
        })
        .collect()
}

/// Acceptance #4 — `insta` snapshot of the scan SCHEMA (surface contract).
/// Schema-only (no computed floats) so it pins the dispatch surface
/// deterministically across CLI/MCP/HTTP. Numeric behaviour is pinned by the
/// asserts below.
#[test]
fn scan_cusum_break_schema() {
    let scan = CusumBreakScan;
    assert_eq!(scan.arity(), ScanArity::Single);
    let shape = scan.finding_fields();
    let schema = serde_json::json!({
        "scan_id": scan.id(),
        "version": scan.version(),
        "arity": "Single",
        "effect_metric": "cusum_break",
        "effect_size_kind": "mean_shift | vol_shift (per detected break)",
        "effect_value": "normalized CUSUM statistic at the detected break",
        "effect_extra_keys": shape.effect_extra_keys,
        "raw_series_keys": shape.raw_series_keys,
        "params": {
            "target": {"type": "string", "default": "both", "enum": ["mean", "vol", "both"]},
            "threshold": {"type": "number", "default": 1.358, "exclusiveMinimum": 0},
            "min_segment": {"type": "integer", "default": 30, "minimum": 2}
        }
    });
    insta::assert_json_snapshot!("cusum_break_schema", schema);
}

/// Acceptance #2 (part 1) — a stationary single-regime series flags no breaks.
#[test]
fn scan_cusum_break_stationary_flags_none() {
    let closes = closes_from_returns(&noise(400, 0x1234_5678, 0.01));
    // Conservative threshold (≈0.001 % level): a stationary series is tested
    // once per target (segmentation only recurses after a break), so a single
    // fixed-seed noise draw cannot trip a false positive.
    let findings = run_breaks(serde_json::json!({"threshold": 2.5}), &closes);
    assert!(
        findings.is_empty(),
        "stationary series must flag no breaks; got {}",
        findings.len()
    );
}

/// Acceptance #2 (part 2a) — an injected MEAN shift is flagged near the true
/// changepoint with a `mean_shift` effect size.
#[test]
fn scan_cusum_break_mean_shift_near_changepoint() {
    let cp = 200usize;
    let mut rets = noise(400, 0xABCD_1234, 0.01);
    for v in rets.iter_mut().skip(cp) {
        *v += 0.05;
    }
    let closes = closes_from_returns(&rets);
    let findings = run_breaks(serde_json::json!({"target": "mean"}), &closes);
    assert!(!findings.is_empty(), "must detect the injected mean shift");

    let r = &findings[0];
    assert_eq!(r.scan_id_at_version, "stats.regime.cusum_break@1");
    assert_eq!(r.effect.metric, "cusum_break");
    assert!(r.effect.p_value.is_none());
    assert_eq!(r.effect.effect_size.as_ref().unwrap().kind, "mean_shift");
    // every break is a mean break.
    for f in &findings {
        let target = &f.effect.extra["target"].data.0;
        assert_eq!(std::str::from_utf8(target).unwrap(), "mean");
    }
    // close-space break index lands near cp+1.
    let nearest = findings
        .iter()
        .map(|f| (decode_f64(&f.effect.extra["break_index"]) as usize).abs_diff(cp + 1))
        .min()
        .expect("non-empty");
    assert!(
        nearest <= 16,
        "nearest break {nearest} bars from changepoint"
    );

    // raw + extra shape.
    let raw = r.raw.as_ref().expect("raw");
    let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
    assert_eq!(raw_keys, vec!["closes", "timestamps_ms"]);
}

/// Acceptance #2 (part 2b) — an injected VOLATILITY shift is flagged near the
/// true changepoint with a `vol_shift` effect size.
#[test]
fn scan_cusum_break_vol_shift_near_changepoint() {
    let cp = 200usize;
    let mut rets = noise(400, 0x0F0F_0F0F, 0.01);
    for v in rets.iter_mut().skip(cp) {
        *v *= 8.0;
    }
    let closes = closes_from_returns(&rets);
    let findings = run_breaks(serde_json::json!({"target": "vol"}), &closes);
    assert!(!findings.is_empty(), "must detect the injected vol shift");
    for f in &findings {
        let target = &f.effect.extra["target"].data.0;
        assert_eq!(std::str::from_utf8(target).unwrap(), "vol");
        assert_eq!(f.effect.effect_size.as_ref().unwrap().kind, "vol_shift");
    }
    let nearest = findings
        .iter()
        .map(|f| (decode_f64(&f.effect.extra["break_index"]) as usize).abs_diff(cp + 1))
        .min()
        .expect("non-empty");
    assert!(
        nearest <= 20,
        "nearest break {nearest} bars from changepoint"
    );
}

/// An invalid `target` is rejected with a kernel error (surface validation).
#[test]
fn scan_cusum_break_invalid_target_rejected() {
    let bars = build_bar_frame_from_closes(&closes_from_returns(&noise(100, 1, 0.01)));
    let mut sink = BufferSink::new();
    let err = CusumBreakScan
        .run(
            &ctx(&bars),
            &request(serde_json::json!({"target": "garbage"})),
            &mut sink,
        )
        .expect_err("must reject invalid target");
    assert!(matches!(err, miner_core::scan::ScanError::Kernel(_)));
}

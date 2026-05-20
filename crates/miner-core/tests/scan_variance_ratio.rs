//! Phase 4 Plan 04-05 — ANOM-07 `stats.variance_ratio.lo_mackinlay@1`
//! happy-path integration test.
//!
//! Pattern analog: `crates/miner-core/tests/scan_kpss.rs` (sibling Plan
//! 04-05 stationarity-test integration). Pins envelope shape via `insta`
//! snapshot. Reference: `arch.unitroot.VarianceRatio(returns, lags=k,
//! robust=True)`. Golden parity reserved for Plan 04-11 within 1e-8 per
//! RESEARCH §Section 2.

#![allow(clippy::cast_precision_loss, clippy::too_many_lines)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::anom::VarianceRatioScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

/// Random-walk close series: close[i] = close[i-1] * exp(eps).
/// log_returns are essentially the eps sequence (IID), so VR(k) ≈ 1.0
/// under the random-walk null hypothesis.
#[allow(clippy::cast_possible_truncation)]
fn random_walk_closes(n: usize, seed: u64) -> Vec<f64> {
    let mut s = seed as u32;
    let mut out = Vec::with_capacity(n);
    let mut price = 1.0_f64;
    out.push(price);
    for _ in 1..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let eps = (f64::from(s) / f64::from(u32::MAX) - 0.5) * 0.01;
        price *= eps.exp();
        out.push(price);
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

/// Plan 04-05 Task 3 — happy-path envelope snapshot.
#[test]
fn scan_variance_ratio_happy_path() {
    let closes = random_walk_closes(500, 42);
    let bars = build_bar_frame_from_closes(&closes);

    let resolved_params = serde_json::json!({"k_values": [2, 4, 8, 16], "robust": true});
    let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();
    let req = ScanRequest {
        scan_id: "stats.variance_ratio.lo_mackinlay".into(),
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
    };
    let ctx = ScanCtx {
        bars: &bars,
        bars_pair: None,
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev-abc1234",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    VarianceRatioScan
        .run(&ctx, &req, &mut sink)
        .expect("scan ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result");
    };
    assert_eq!(r.scan_id_at_version, "stats.variance_ratio.lo_mackinlay@1");
    assert_eq!(r.effect.metric, "variance_ratio_max_k");
    assert!(r.effect.p_value.is_none(), "VR uses parallel arrays, no headline p");

    let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
    assert_eq!(
        extra_keys,
        vec!["k_values", "p_values", "vr_values", "z_stats"]
    );
    // All four parallel arrays of length 4.
    for key in ["k_values", "vr_values", "z_stats", "p_values"] {
        assert_eq!(r.effect.extra[key].shape, vec![4], "{key} length mismatch");
    }

    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("variance_ratio_happy_path", masked);
}

//! Phase 4 Plan 04-05 — ANOM-05 `stats.stationarity.adf@1` happy-path
//! integration test.
//!
//! Pattern analog: `crates/miner-core/tests/scan_outliers.rs` (sibling
//! ANOM-family integration test). Pins envelope shape via `insta` snapshot.
//! Hand-derived ADF test statistic tolerance: 1e-10; `MacKinnon` p approx 1e-8
//! per RESEARCH §Section 2.

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
use miner_core::scan::anom::AdfScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

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

/// Plan 04-05 Task 1 — happy-path envelope snapshot on a deterministic LCG
/// fixture. Plan 04-11 will pin the actual ADF stat/p-value against
/// `statsmodels.tsa.stattools.adfuller` within 1e-10 (stat) / 1e-8 (p).
#[test]
fn scan_adf_happy_path() {
    let closes = lcg_closes(80, 42);
    let bars = build_bar_frame_from_closes(&closes);

    let resolved_params = serde_json::json!({"max_lag": 4, "regression": "c", "autolag": "AIC"});
    let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 4, 0, 0, 0).unwrap();
    let req = ScanRequest {
        scan_id: "stats.stationarity.adf".into(),
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
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
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
    AdfScan.run(&ctx, &req, &mut sink).expect("scan ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result");
    };
    assert_eq!(r.scan_id_at_version, "stats.stationarity.adf@1");
    assert_eq!(r.effect.metric, "adf_statistic");
    assert!(r.effect.p_value.is_some(), "ADF emits p_value");

    let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
    assert_eq!(
        extra_keys,
        vec![
            "crit_values",
            "lag_selected",
            "nobs",
            "p_value",
            "regression",
        ]
    );
    assert_eq!(r.effect.extra["crit_values"].shape, vec![3]);

    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("adf_happy_path", masked);
}

//! RAD-3839 — ANOM-12 `stats.momentum.tsmom@1` happy-path integration test.
//!
//! Pattern analog: `crates/miner-core/tests/scan_variance_ratio.rs` (sibling
//! multi-`k` ANOM scan). Asserts the full envelope shape inline, then pins a
//! float-free `insta` *schema* snapshot (scan-id, metric, effect-size kind,
//! parallel-array names + shapes, raw-series names + shapes, params). The
//! schema snapshot is deterministic regardless of the underlying float math
//! (the continuation coefficients / p-values are exercised for sign and
//! significance by the unit tests in the scan module).

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
use miner_core::scan::anom::TsmomScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

/// Random-walk close series: close[i] = close[i-1] * exp(eps), eps IID — so
/// `log_returns` are white noise and the continuation coefficient is ≈ 0.
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

#[test]
fn scan_tsmom_happy_path() {
    let closes = random_walk_closes(600, 42);
    let bars = build_bar_frame_from_closes(&closes);

    let resolved_params = serde_json::json!({"k_values": [1, 5, 10, 20], "scaling": true});
    let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();
    let req = ScanRequest {
        scan_id: "stats.momentum.tsmom".into(),
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
    TsmomScan.run(&ctx, &req, &mut sink).expect("scan ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result");
    };
    assert_eq!(r.scan_id_at_version, "stats.momentum.tsmom@1");
    assert_eq!(r.effect.metric, "tsmom_continuation");
    assert!(
        r.effect.p_value.is_some(),
        "TSMOM reports a headline continuation p at the selected hold horizon"
    );
    assert_eq!(r.effect.n, Some(599), "n = closes - 1 log returns");

    let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
    assert_eq!(
        extra_keys,
        vec![
            "continuation_coefs",
            "hit_rates",
            "k_values",
            "p_values",
            "selected_hold_bars",
            "t_stats",
            "tsmom_means",
            "turnover_per_bar",
        ]
    );
    for key in [
        "continuation_coefs",
        "hit_rates",
        "k_values",
        "p_values",
        "t_stats",
        "tsmom_means",
        "turnover_per_bar",
    ] {
        assert_eq!(r.effect.extra[key].shape, vec![4], "{key} length mismatch");
    }
    assert_eq!(
        r.effect.extra["selected_hold_bars"].shape,
        vec![1],
        "selected_hold_bars is a single-element array"
    );

    let es = r.effect.effect_size.as_ref().expect("effect_size present");
    assert_eq!(es.kind, "hit_rate");
    assert!(es.value >= 0.0 && es.value <= 1.0, "hit-rate in [0,1]");

    let raw = r.raw.as_ref().expect("raw present");
    let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
    assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);

    // Float-free schema snapshot — pins the catalogue-facing shape without
    // coupling to the exact continuation/p-value floats (those are covered by
    // the scan-module unit tests for sign + significance).
    let mut extra_shapes = serde_json::Map::new();
    for (k, arr) in &r.effect.extra {
        extra_shapes.insert(k.clone(), serde_json::json!(arr.shape));
    }
    let mut raw_shapes = serde_json::Map::new();
    for (k, arr) in &raw.series {
        raw_shapes.insert(k.clone(), serde_json::json!(arr.shape));
    }
    let schema = serde_json::json!({
        "scan_id_at_version": r.scan_id_at_version,
        "effect_metric": r.effect.metric,
        "effect_size_kind": es.kind,
        "headline_p_value_present": r.effect.p_value.is_some(),
        "n": r.effect.n,
        "effect_extra": serde_json::Value::Object(extra_shapes),
        "raw_series": serde_json::Value::Object(raw_shapes),
        "params": r.params,
    });
    insta::assert_json_snapshot!("tsmom_schema", schema);
}

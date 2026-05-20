//! Phase 4 Plan 04-03 — ANOM-03 `stats.vol.rolling@1` happy-path integration
//! test.
//!
//! Pattern analog: `crates/miner-core/tests/scan_returns_profile.rs` (sibling
//! ANOM-01 integration test). Pins envelope shape via `insta` snapshot;
//! emits EXACTLY ONE Finding::Result with vector arrays in `effect.extra`
//! (Pattern D / Pitfall 1 — never N envelopes per window). Golden cross-check
//! against `pandas.Series.rolling(W).std(ddof=1)` lands in Plan 04-11.

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
use miner_core::scan::anom::VolRollingScan;
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

/// Plan 04-03 Task 3 — happy-path envelope snapshot.
#[test]
fn scan_vol_rolling_happy_path() {
    let closes = lcg_closes(64, 42);
    let bars = build_bar_frame_from_closes(&closes);

    let resolved_params = serde_json::json!({"window": 10});
    let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 4, 0, 0, 0).unwrap();
    let req = ScanRequest {
        scan_id: "stats.vol.rolling".into(),
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
    VolRollingScan
        .run(&ctx, &req, &mut sink)
        .expect("scan ok");

    let findings = common::parse_findings(&sink.0);
    // Pattern D / Pitfall 1 — exactly ONE envelope, vectors in extra.
    assert_eq!(findings.len(), 1, "exactly one envelope (Pitfall 1)");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result");
    };
    assert_eq!(r.scan_id_at_version, "stats.vol.rolling@1");
    assert_eq!(r.effect.metric, "vol_rolling_last");
    // 64 closes -> 63 log returns -> 63 - 10 + 1 = 54 rolling windows.
    assert_eq!(r.effect.n, Some(54));
    let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
    assert_eq!(
        extra_keys,
        vec!["values", "vol_of_vol", "window_length", "window_starts_ms"]
    );

    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("vol_rolling_happy_path", masked);
}

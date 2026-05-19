//! Plan 04-09 Task 1 integration test — SEAS-01 hour-of-day envelope snapshot.
//!
//! Builds a deterministic 7-day × 24h × 4 bars/h BarFrame at 15m timeframe
//! (672 bars total), runs `HourOfDayScan::run`, parses the resulting envelope,
//! masks volatile fields (`run_id`, `produced_at_utc`), and pins the shape via
//! an insta snapshot. The 24-bucket extra-array lengths and the
//! `seas.bucket.hour_of_day@1` `scan_id_at_version` are asserted directly.
//!
//! Pattern analog: `scan_ljung_box.rs` (Pattern J).

#![allow(clippy::cast_precision_loss)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::seas::HourOfDayScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

#[test]
fn scan_seas_hour_of_day_happy_path() {
    // 7 days × 24 hours × 4 bars/hour at 15m = 672 bars.
    let bars = build_synthetic_15m_bars(672, 0xDEAD_BEEF);

    let resolved_params = serde_json::json!({"min_obs_per_bucket": 5});
    let param_hash = param_hash::param_hash(&resolved_params).expect("param_hash ok");
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
        param_hash,
        dry_run: false,
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
    HourOfDayScan
        .run(&ctx, &req, &mut sink)
        .expect("HourOfDayScan::run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result, got {:?}", findings[0]);
    };

    // Pin the wire-form shape directly (insta snapshot also covers this).
    assert_eq!(r.scan_id_at_version, "seas.bucket.hour_of_day@1");
    assert_eq!(r.effect.metric, "hour_of_day_max_abs_t_stat");
    // 672 closes -> 671 returns.
    assert_eq!(r.effect.n, Some(671));
    // 24-bucket parallel arrays in effect.extra.
    for key in ["buckets", "means", "stds", "counts", "t_stats", "iqrs"] {
        let arr = r
            .effect
            .extra
            .get(key)
            .unwrap_or_else(|| panic!("effect.extra[{key}] present"));
        assert_eq!(arr.shape, vec![24], "{key} must be length 24");
    }
    // Single-arity sources len 1.
    assert_eq!(r.data_slice.sources.len(), 1);
    assert_eq!(r.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(r.data_slice.sources[0].side, "bid");
    assert_eq!(r.data_slice.sources[0].timeframe, "15m");

    // Insta snapshot of the masked envelope shape.
    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("scan_seas_hour_of_day_happy_path", masked);
}

/// Build a 15m-timeframe BarFrame of `n` bars starting at 2024-01-01 00:00 UTC
/// with deterministic LCG-seeded closes (Phase 3's `lcg_closes` convention).
#[allow(clippy::cast_possible_truncation)]
fn build_synthetic_15m_bars(n: usize, seed: u64) -> BarFrame {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let mut s = seed as u32;
    let mut closes = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        closes.push(1.0 + frac);
    }
    let ts_open: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| start + Duration::minutes(15 * i as i64))
        .collect();
    let ts_close: Vec<chrono::DateTime<Utc>> =
        ts_open.iter().map(|t| *t + Duration::minutes(15)).collect();
    let opens = closes.clone();
    let highs: Vec<f64> = closes.iter().map(|c| c + 0.001).collect();
    let lows: Vec<f64> = closes.iter().map(|c| c - 0.001).collect();
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
        close: closes,
        tick_volume: vols,
    }
}

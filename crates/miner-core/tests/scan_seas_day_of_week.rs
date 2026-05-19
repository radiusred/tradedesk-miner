//! Plan 04-09 Task 2 integration test — SEAS-02 day-of-week envelope snapshot.
//!
//! Builds a deterministic 28-day BarFrame at 15m timeframe (2688 bars =
//! 4 weeks × 7 days × 24h × 4 bars/h), runs `DayOfWeekScan::run`, parses the
//! resulting envelope, masks volatile fields, and pins the shape via an insta
//! snapshot.
//!
//! Pattern analog: `scan_seas_hour_of_day.rs` (Pattern J).

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
use miner_core::scan::seas::DayOfWeekScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

#[test]
fn scan_seas_day_of_week_happy_path() {
    // 28 days × 24 hours × 4 bars/hour at 15m = 2688 bars.
    let bars = build_synthetic_15m_bars(2688, 0xC0FF_EE42);

    let resolved_params = serde_json::json!({"min_obs_per_bucket": 5});
    let param_hash = param_hash::param_hash(&resolved_params).expect("param_hash ok");
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = start + Duration::days(28);
    let req = ScanRequest {
        scan_id: "seas.bucket.day_of_week".into(),
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
    DayOfWeekScan
        .run(&ctx, &req, &mut sink)
        .expect("DayOfWeekScan::run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result, got {:?}", findings[0]);
    };

    assert_eq!(r.scan_id_at_version, "seas.bucket.day_of_week@1");
    assert_eq!(r.effect.metric, "day_of_week_max_abs_t_stat");
    // 2688 closes -> 2687 returns.
    assert_eq!(r.effect.n, Some(2687));
    for key in ["buckets", "means", "stds", "counts", "t_stats", "iqrs"] {
        let arr = r
            .effect
            .extra
            .get(key)
            .unwrap_or_else(|| panic!("effect.extra[{key}] present"));
        assert_eq!(arr.shape, vec![7], "{key} must be length 7");
    }
    assert_eq!(r.data_slice.sources.len(), 1);

    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("scan_seas_day_of_week_happy_path", masked);
}

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

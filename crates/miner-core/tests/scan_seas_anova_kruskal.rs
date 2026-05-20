//! Plan 04-10 Task 2 integration test — SEAS-05 `anova_kruskal` envelope snapshot.
//!
//! Builds a deterministic 7-day × 24h × 4 bars/h = 672-bar `BarFrame` at the
//! 15m timeframe, runs `AnovaKruskalScan::run` with
//! `buckets_via=hour_of_day`, parses the resulting envelope, masks volatile
//! fields, and pins the shape via an insta snapshot.
//!
//! Pattern analog: `scan_seas_hour_of_day.rs` (Pattern J).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::seas::AnovaKruskalScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

#[test]
fn scan_seas_anova_kruskal_happy_path() {
    // 7 days × 24h × 4 bars/h = 672 bars at 15m.
    let bars = build_synthetic_15m_bars(672, 0xABCD_1234);

    let resolved_params = serde_json::json!({"buckets_via": "hour_of_day", "min_obs_per_group": 5});
    let param_hash = param_hash::param_hash(&resolved_params).expect("param_hash ok");
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = start + Duration::days(7);
    let req = ScanRequest {
        scan_id: "seas.test.anova_kruskal".into(),
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
    AnovaKruskalScan
        .run(&ctx, &req, &mut sink)
        .expect("AnovaKruskalScan::run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result, got {:?}", findings[0]);
    };

    assert_eq!(r.scan_id_at_version, "seas.test.anova_kruskal@1");
    assert_eq!(r.effect.metric, "anova_f_statistic");
    assert!(r.effect.p_value.is_some());
    for key in [
        "anova_p_value",
        "group_count",
        "kw_p_value",
        "kw_stat",
        "total_n",
    ] {
        assert!(
            r.effect.extra.contains_key(key),
            "effect.extra[{key}] missing"
        );
    }
    assert_eq!(r.data_slice.sources.len(), 1);

    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("scan_seas_anova_kruskal_happy_path", masked);
}

/// Build a 15m-timeframe `BarFrame` of `n` bars starting at 2024-01-01 00:00
/// UTC with deterministic LCG-seeded closes.
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

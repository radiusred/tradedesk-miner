//! Phase 4 (Plan 04-07 Task 2) integration test — CROSS-03 rolling OLS
//! regression scan.
//!
//! Drives `OlsRollingScan` directly via `Scan::run` against a two-leg
//! deterministic seeded fixture. Pattern J analog: `scan_ljung_box.rs`.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::cross::OlsRollingScan;
use miner_core::scan::{Scan, ScanCtx};

use common::BufferSink;

fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
    #[allow(clippy::cast_possible_truncation)]
    let mut s = seed as u32;
    let mut closes = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        closes.push(1.0 + frac);
    }
    closes
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

#[test]
fn scan_ols_rolling_happy_path() {
    let n = 64;
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let ts: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| {
            let i_i64 = i64::try_from(i).expect("n fits");
            start + Duration::minutes(15 * i_i64)
        })
        .collect();
    let closes_a = lcg_closes(n, 17);
    let closes_b = lcg_closes(n, 29);
    let a = build_bars("EURUSD", &ts, &closes_a);
    let b = build_bars("GBPUSD", &ts, &closes_b);

    let resolved_params = serde_json::json!({"window": 6});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap();

    let req = miner_core::scan::ScanRequest {
        scan_id: "cross.ols.rolling".into(),
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
        sleep_after_first_finding_ms: None,
    };

    let ctx = ScanCtx {
        bars: &a,
        bars_pair: Some((&a, &b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };

    let mut sink = BufferSink::new();
    OlsRollingScan
        .run(&ctx, &req, &mut sink)
        .expect("OLS rolling run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1);
    let Finding::Result(r) = &findings[0] else {
        panic!("expected Result");
    };
    assert_eq!(r.scan_id_at_version, "cross.ols.rolling@1");
    assert_eq!(r.effect.metric, "ols_rolling_beta_last");
    // window=6 against n=64 -> 63 returns -> 58 windows.
    assert_eq!(r.effect.n, Some(58));
    // D4-03: two-leg sources Vec.
    assert_eq!(r.data_slice.sources.len(), 2);
    assert_eq!(r.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(r.data_slice.sources[1].symbol, "GBPUSD");
    // raw.series keys (D-03 canonical timestamps_ms + leg-labelled returns).
    let raw = r.raw.as_ref().expect("raw present");
    assert!(raw.series.contains_key("returns_a"));
    assert!(raw.series.contains_key("returns_b"));
    assert!(raw.series.contains_key("timestamps_ms"));
    // effect.extra includes the four per-window stats vectors.
    for key in ["betas", "alphas", "r2s", "residual_stds", "window_starts_ms", "window_length"] {
        assert!(
            r.effect.extra.contains_key(key),
            "effect.extra missing {key}"
        );
    }
    // effect.value is finite.
    assert!(r.effect.value.is_finite());
}

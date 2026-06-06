//! RAD-3840 integration test — `seas.gap.overnight@1` envelope snapshot.
//!
//! Builds a deterministic 6-session × 2-bar (15m) `BarFrame` with engineered
//! overnight gaps (each session's second bar closes at 100.0 so every gap is
//! measured against 100.0), runs `OvernightGapScan::run`, asserts the recovered
//! gap-size distribution / direction×bucket fill-probability / caveat flags
//! (AC-2), then pins the masked envelope shape via an insta snapshot (AC-4).
//!
//! A sibling unit test in `src/scan/seas/gap/mod.rs` covers the gapless /
//! sparse-flag path (AC-3); the kernel unit tests cover detection / fill /
//! bucketing edge cases.
//!
//! Pattern analog: `scan_seas_eom_som.rs` (Pattern J).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Duration, TimeZone, Utc};

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{Blake3Hex, ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::seas::OvernightGapScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

fn blake3_hex_zero() -> Blake3Hex {
    Blake3Hex::from_hex_bytes(&[b'0'; 64])
}

/// Decode a base64 LE-f64 `effect.extra` / `raw.series` array back to `Vec<f64>`.
fn decode(arr: &miner_core::findings::RawArray) -> Vec<f64> {
    let bytes = &arr.data.0;
    assert_eq!(bytes.len() % 8, 0, "byte length not a multiple of 8");
    bytes
        .chunks_exact(8)
        .map(|c| {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(c);
            f64::from_le_bytes(buf)
        })
        .collect()
}

#[test]
fn scan_seas_gap_overnight_happy_path() {
    let bars = engineered_gap_frame();

    let resolved_params = serde_json::json!({
        "min_obs_per_bucket": 1,
        "fill_lookahead_bars": 1,
        "sparse_gap_min_count": 3
    });
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 8, 0, 0, 0).unwrap();
    let req = ScanRequest {
        scan_id: "seas.gap.overnight".into(),
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
        param_hash: blake3_hex_zero(),
        dry_run: false,
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
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
    OvernightGapScan
        .run(&ctx, &req, &mut sink)
        .expect("OvernightGapScan::run ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result, got {:?}", findings[0]);
    };

    // Wire-form facts.
    assert_eq!(r.scan_id_at_version, "seas.gap.overnight@1");
    assert_eq!(r.effect.metric, "overnight_gap_fill_rate");
    // 5 engineered gaps, 4 fill -> overall fill rate 0.8.
    assert_eq!(r.effect.n, Some(5));
    assert!((r.effect.value - 0.8).abs() < 1e-12);

    // AC-2: distribution + direction×bucket fill-probability match construction.
    assert_eq!(decode(&r.effect.extra["gap_count"]), vec![5.0]);
    assert_eq!(
        decode(&r.effect.extra["up_counts"]),
        vec![0.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(
        decode(&r.effect.extra["down_counts"]),
        vec![0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(
        decode(&r.effect.extra["up_fill_counts"]),
        vec![0.0, 1.0, 1.0, 0.0]
    );
    let up_prob = decode(&r.effect.extra["up_fill_prob"]);
    assert!(up_prob[0].is_nan(), "empty bucket -> NaN");
    assert!((up_prob[1] - 1.0).abs() < 1e-12);
    assert!((up_prob[2] - 1.0).abs() < 1e-12);
    assert!((up_prob[3] - 0.0).abs() < 1e-12, "1 gap, 0 fills -> 0.0");

    // Caveat flags: median bars-to-fill 1 (< 12 floor) -> hold_floor caveat;
    // 5 gaps >= sparse threshold 3 -> NOT sparse.
    assert_eq!(decode(&r.effect.extra["median_bars_to_fill"]), vec![1.0]);
    assert_eq!(decode(&r.effect.extra["hold_floor_caveat"]), vec![1.0]);
    assert_eq!(decode(&r.effect.extra["sparse_gaps"]), vec![0.0]);

    // Size-bucket labels decode to the default 4-bucket scheme.
    let labels_bytes = &r.effect.extra["bucket_labels"].data.0;
    let labels: Vec<String> = serde_json::from_slice(labels_bytes).expect("labels JSON");
    assert_eq!(
        labels,
        vec!["<0.0005", "0.0005..0.001", "0.001..0.002", ">=0.002"]
    );

    // Raw series carry one entry per gap event.
    let raw = r.raw.as_ref().expect("raw present");
    assert_eq!(
        decode(&raw.series["gap_directions"]),
        vec![1.0, 1.0, -1.0, -1.0, 1.0]
    );
    assert_eq!(
        decode(&raw.series["gap_filled"]),
        vec![1.0, 0.0, 1.0, 1.0, 1.0]
    );

    // Single-arity source metadata.
    assert_eq!(r.data_slice.sources.len(), 1);
    assert_eq!(r.data_slice.sources[0].symbol, "EURUSD");
    assert_eq!(r.data_slice.sources[0].side, "bid");
    assert_eq!(r.data_slice.sources[0].timeframe, "15m");

    // AC-4: insta snapshot of the masked envelope shape.
    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("scan_seas_gap_overnight_happy_path", masked);
}

/// 6 sessions × 2 bars (15m). Sessions are one day apart (a >23-min boundary at
/// the default 1.5×15m threshold); each session's second bar closes at 100.0 so
/// every gap is measured against 100.0. With `fill_lookahead_bars = 1` each gap
/// fills (or not) on its own post-gap bar.
fn engineered_gap_frame() -> BarFrame {
    let mut ts: Vec<DateTime<Utc>> = Vec::new();
    for day in 1..=6 {
        let d = Utc.with_ymd_and_hms(2024, 1, day, 0, 0, 0).unwrap();
        ts.push(d);
        ts.push(d + Duration::minutes(15));
    }
    // (open, high, low, close) per bar. Even indices are session-open bars.
    let rows: [(f64, f64, f64, f64); 12] = [
        (100.0, 100.05, 99.95, 100.0),    // s0.A
        (100.0, 100.05, 99.95, 100.0),    // s0.B
        (100.15, 100.20, 99.99, 100.15),  // s1.A up 0.0015 -> fills
        (100.0, 100.05, 99.95, 100.0),    // s1.B
        (100.30, 100.40, 100.20, 100.30), // s2.A up 0.003 -> NOT filled
        (100.0, 100.05, 99.95, 100.0),    // s2.B
        (99.85, 100.01, 99.80, 99.85),    // s3.A down 0.0015 -> fills
        (100.0, 100.05, 99.95, 100.0),    // s3.B
        (99.70, 100.02, 99.60, 99.70),    // s4.A down 0.003 -> fills
        (100.0, 100.05, 99.95, 100.0),    // s4.B
        (100.07, 100.10, 99.95, 100.07),  // s5.A up 0.0007 -> fills
        (100.0, 100.05, 99.95, 100.0),    // s5.B
    ];
    BarFrame {
        source_id: "dukascopy".into(),
        symbol: "EURUSD".into(),
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        ts_open_utc: ts.clone(),
        ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
        open: rows.iter().map(|r| r.0).collect(),
        high: rows.iter().map(|r| r.1).collect(),
        low: rows.iter().map(|r| r.2).collect(),
        close: rows.iter().map(|r| r.3).collect(),
        tick_volume: vec![1.0; 12],
    }
}

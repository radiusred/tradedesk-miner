//! Phase 4 Plan 04-03 — ANOM-01 `stats.returns.profile@1` happy-path
//! integration test.
//!
//! Pattern analog: `crates/miner-core/tests/scan_ljung_box.rs` (Phase 3
//! gold-standard) — 8-step Pattern J walk. ANOM-01 has no statsmodels
//! golden (the returns kernel is hand-derivable per CLAUDE.md
//! `windows(2).map(|w| (w[1]/w[0]).ln())`); this test pins the envelope
//! shape via an `insta` snapshot of the masked finding.

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
use miner_core::scan::anom::ReturnsProfileScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

use common::BufferSink;

/// LCG-seeded close vector — Numerical Recipes constants. Deterministic
/// across platforms; no `rand` dependency.
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

/// Plan 04-03 Task 1 Pattern J — happy-path envelope snapshot.
#[test]
fn scan_returns_profile_happy_path() {
    // Step 1 — synthesize a deterministic close series.
    let closes = lcg_closes(64, 42);
    assert_eq!(closes.len(), 64);

    // Step 2 — build a `BarFrame` over those closes.
    let bars = build_bar_frame_from_closes(&closes);

    // Step 3 — construct the ScanRequest + ScanCtx (D4-01 instruments Vec).
    let resolved_params = serde_json::json!({"variant": "log"});
    let param_hash = param_hash::param_hash(&resolved_params).expect("param_hash ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 4, 0, 0, 0).unwrap();
    let req = ScanRequest {
        scan_id: "stats.returns.profile".into(),
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

    // Step 4 — dispatch the scan.
    let mut sink = BufferSink::new();
    ReturnsProfileScan
        .run(&ctx, &req, &mut sink)
        .expect("scan ok");

    // Step 5 — parse the captured envelope.
    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1, "exactly one envelope emitted");
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Finding::Result; got {:?}", findings[0]);
    };

    // Step 6 — assert wire-form invariants the snapshot would otherwise
    // ratify silently (defensive — catches a snapshot regression that
    // accidentally accepts a wrong shape).
    assert_eq!(r.scan_id_at_version, "stats.returns.profile@1");
    assert_eq!(r.effect.metric, "returns_log_mean");
    // 64 closes -> 63 log returns.
    assert_eq!(r.effect.n, Some(63));
    let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
    assert_eq!(
        extra_keys,
        vec!["mean", "n", "returns_vector", "std", "variant_label"]
    );

    // Step 7 — variant_label discriminator == 0.0 (log).
    let bytes = &r.effect.extra["variant_label"].data.0;
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[0..8]);
    assert_eq!(f64::from_le_bytes(buf), 0.0_f64);

    // Step 8 — insta snapshot of the masked envelope shape. The masked
    // snapshot pins structural changes; volatile fields (`run_id`,
    // `produced_at_utc`) are replaced with constants by
    // `common::mask_volatile_fields`.
    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("returns_profile_happy_path", masked);
}

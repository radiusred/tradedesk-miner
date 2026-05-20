//! Phase 4 Plan 04-03 — ANOM-02 `stats.summary.welford@1` happy-path
//! integration test.
//!
//! Pattern analog: `crates/miner-core/tests/scan_returns_profile.rs` (sibling
//! ANOM-01 integration test). The Welford pass + IQR kernel is hand-derived
//! against scipy.stats references (offline) and unit-tested in the kernel
//! module to within 1e-12; this integration test pins the envelope shape
//! via `insta` snapshot. Golden cross-check against `scipy.stats.describe`
//! lands in Plan 04-11 once the regen scripts are authored.

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
use miner_core::scan::anom::SummaryWelfordScan;
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

/// Plan 04-03 Task 2 Pattern J — happy-path envelope snapshot.
#[test]
fn scan_summary_welford_happy_path() {
    let closes = lcg_closes(64, 42);
    let bars = build_bar_frame_from_closes(&closes);

    let resolved_params = serde_json::json!({"series": "log_returns"});
    let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 4, 0, 0, 0).unwrap();
    let req = ScanRequest {
        scan_id: "stats.summary.welford".into(),
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
    SummaryWelfordScan
        .run(&ctx, &req, &mut sink)
        .expect("scan ok");

    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1);
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result; got {:?}", findings[0]);
    };
    assert_eq!(r.scan_id_at_version, "stats.summary.welford@1");
    assert_eq!(r.effect.metric, "summary_welford_mean");
    assert_eq!(r.effect.n, Some(63));
    let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
    assert_eq!(
        extra_keys,
        vec!["excess_kurtosis", "iqr", "max", "min", "n", "skew", "std"]
    );

    let mut masked = serde_json::to_value(&findings[0]).expect("envelope -> Value");
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("summary_welford_happy_path", masked);
}

/// Phase 4 Plan 04-11 Task 1 golden cross-check — ANOM-02.
///
/// Loads `crates/miner-core/tests/goldens/stats.summary.welford.jsonl`,
/// builds a `BarFrame` from the SAME LCG-seeded close array
/// (`lcg_closes(64, 42)`) the Rust scan consumes, runs
/// `SummaryWelfordScan::run`, and asserts the emitted `effect.*` fields
/// (mean / std / skew / `excess_kurtosis` / iqr / min / max / n) match
/// the golden's `expected.*` block within RESEARCH §Section 2 tolerance
/// 1e-10 (per REFERENCE-VERSIONS.md row 1 — "1e-10 — chi2.cdf / norm.cdf
/// paths"; ANOM-02 is pure Welford so it should match even tighter,
/// but 1e-10 is the documented headline).
///
/// Pattern J Step 1 — provenance gate. The test refuses to run unless
/// `provenance.scipy_version == "1.14.1"`. The stub golden checked in
/// at Plan 04-11 has `provenance.scipy_version == "STUB"`; this test
/// is `#[ignore]`d until a developer runs the regen recipe documented
/// in `goldens/REFERENCE-VERSIONS.md`.
#[test]
#[ignore = "Phase 4 Plan 04-11: golden is a STUB until pinned Python 3.11 \
            + scipy==1.14.1 venv regenerates stats.summary.welford.jsonl \
            (see crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md)"]
fn summary_welford_matches_scipy_describe_golden() {
    const GOLDEN_JSON: &str = include_str!("goldens/stats.summary.welford.jsonl");
    const TOL: f64 = 1e-10;

    let golden: serde_json::Value = serde_json::from_str(GOLDEN_JSON.trim())
        .expect("stats.summary.welford.jsonl must be valid JSON");

    // Pattern J Step 1 — provenance gate.
    let prov = golden["provenance"]["scipy_version"].as_str();
    assert_eq!(
        prov,
        Some("1.14.1"),
        "stats.summary.welford.jsonl provenance.scipy_version must be \"1.14.1\"; got {prov:?}. \
         Regenerate via `python crates/miner-core/tests/goldens/generate_summary_welford.py > crates/miner-core/tests/goldens/stats.summary.welford.jsonl` \
         in a pinned venv per crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md.",
    );

    // Build BarFrame from the LCG-seeded close array (same recipe Rust
    // and Python both consume).
    let closes = lcg_closes(64, 42);
    let bars = build_bar_frame_from_closes(&closes);
    let resolved_params = serde_json::json!({"series": "log_returns"});
    let param_hash = param_hash::param_hash(&resolved_params).expect("ok");
    let window_start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let window_end = Utc.with_ymd_and_hms(2024, 1, 4, 0, 0, 0).unwrap();
    let req = ScanRequest {
        scan_id: "stats.summary.welford".into(),
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
    SummaryWelfordScan
        .run(&ctx, &req, &mut sink)
        .expect("scan ok");
    let findings = common::parse_findings(&sink.0);
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Result");
    };

    // Decode the per-scalar effect.extra entries (length-1 RawArrays) +
    // the headline effect.value (= mean).
    let mean_actual = r.effect.value;
    let std_actual = decode_scalar(&r.effect.extra, "std");
    let skew_actual = decode_scalar(&r.effect.extra, "skew");
    let kurt_actual = decode_scalar(&r.effect.extra, "excess_kurtosis");
    let iqr_actual = decode_scalar(&r.effect.extra, "iqr");
    let min_actual = decode_scalar(&r.effect.extra, "min");
    let max_actual = decode_scalar(&r.effect.extra, "max");
    let n_actual = r.effect.n.expect("n present");

    let exp = &golden["expected"];
    let assert_within = |label: &str, actual: f64, expected: f64| {
        let diff = (actual - expected).abs();
        assert!(
            diff <= TOL,
            "{label}: |{actual} - {expected}| = {diff:.3e} exceeds tolerance {TOL:.3e}",
        );
    };
    assert_within("mean", mean_actual, exp["mean"].as_f64().expect("mean f64"));
    assert_within("std", std_actual, exp["std"].as_f64().expect("std f64"));
    assert_within("skew", skew_actual, exp["skew"].as_f64().expect("skew f64"));
    assert_within(
        "excess_kurtosis",
        kurt_actual,
        exp["excess_kurtosis"].as_f64().expect("excess_kurtosis f64"),
    );
    assert_within("iqr", iqr_actual, exp["iqr"].as_f64().expect("iqr f64"));
    assert_within("min", min_actual, exp["min"].as_f64().expect("min f64"));
    assert_within("max", max_actual, exp["max"].as_f64().expect("max f64"));
    assert_eq!(
        n_actual,
        exp["n"].as_u64().expect("n u64"),
        "n mismatch"
    );
}

/// Decode a length-1 `effect.extra[key]` `RawArray` (LE-f64 bytes) into
/// a single `f64` scalar.
fn decode_scalar(
    extra: &std::collections::BTreeMap<String, miner_core::findings::RawArray>,
    key: &str,
) -> f64 {
    let arr = extra
        .get(key)
        .unwrap_or_else(|| panic!("effect.extra[{key}] present"));
    let bytes = &arr.data.0;
    assert_eq!(bytes.len(), 8, "{key}: expected length-1 f64 array (8 bytes)");
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[..8]);
    f64::from_le_bytes(buf)
}

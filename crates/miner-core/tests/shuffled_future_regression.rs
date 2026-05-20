//! Phase 3 integration test — look-ahead-safety proptest (D3-09 / SC-6b).
//!
//! Statistics up to time T MUST be byte-identical when bars at index >T are
//! permuted. The shuffle is a deterministic permutation seeded by the
//! proptest `seed` so failures are reproducible.
//!
//! ## Scope (Warning 10 — exact doc-comment phrasing required)
//!
//! This is the full D3-09 enforcement for Ljung-Box (a single-shot,
//! non-rolling scan). Phase 4 will ADD additional cancellation_tests-style
//! proptests for each new rolling/causal scan it introduces — it does NOT
//! extend this proptest.
//!
//! Phase 4 Plan 04-03 — ANOM-03 `stats.vol.rolling@1` extension added the
//! `vol_rolling_shuffled_future_invariant` proptest below per Pattern M.
//! Plans 04-07 (Pearson/Spearman/OLS rolling) defer their proptest
//! extensions to Plan 04-08 (Wave 4) to avoid same-wave write-conflicts on
//! this file.

#![allow(dead_code, unused_imports, unexpected_cfgs)]

use chrono::{Duration, TimeZone, Utc};
use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, Side};
use miner_core::scan::anom::VolRollingScan;
use miner_core::scan::ljung_box::LjungBoxScan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};

mod common;
use common::BufferSink;

/// N bars in the synthetic series; T is the cut-point at which we re-compute
/// Ljung-Box. The post-T tail is shuffled; the proptest asserts the pre-T
/// stats are byte-identical.
const N: usize = 256;
const T: usize = 128;
const LAGS: usize = 5;

/// LCG seeded by `seed`; returns `n` deterministic f64 closes in `[1.0, 2.0)`.
fn lcg_closes(n: usize, seed: u32) -> Vec<f64> {
    let mut s = seed;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frac = f64::from(s) / f64::from(u32::MAX);
        out.push(1.0 + frac);
    }
    out
}

/// Apply a deterministic Fisher-Yates shuffle to `slice` in place using `seed`.
fn shuffle_in_place(slice: &mut [f64], seed: u32) {
    let mut s = seed.wrapping_add(0xABCD_1234);
    for i in (1..slice.len()).rev() {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let j = (s as usize) % (i + 1);
        slice.swap(i, j);
    }
}

/// Build a `BarFrame` from a pre-computed close array; OHLC derived trivially.
fn bar_frame_from_closes(closes: &[f64]) -> BarFrame {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let n = closes.len();
    let ts_open: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| start + Duration::minutes(15 * i64::try_from(i).unwrap()))
        .collect();
    let ts_close: Vec<chrono::DateTime<Utc>> =
        ts_open.iter().map(|t| *t + Duration::minutes(15)).collect();
    let opens: Vec<f64> = closes.to_vec();
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
        close: closes.to_vec(),
        tick_volume: vols,
    }
}

/// Run `LjungBoxScan::run` against a `BarFrame` slice (closes[..=T]) and
/// return the Result envelope's `effect.extra["q_stats"]` as a `Vec<f64>`.
fn run_and_extract_q_stats(closes_slice: &[f64]) -> Vec<f64> {
    let bars = bar_frame_from_closes(closes_slice);
    let resolved_params = serde_json::json!({"lags": LAGS});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = start + Duration::minutes(15 * i64::try_from(closes_slice.len()).unwrap());
    let req = ScanRequest {
        scan_id: "stats.autocorr.ljung_box".into(),
        version: 1,
        // Phase 4 (D4-01): single-leg instruments Vec.
        instruments: vec![miner_core::reader::InstrumentSpec {
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
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };
    let mut sink = BufferSink::new();
    LjungBoxScan.run(&ctx, &req, &mut sink).expect("scan ok");
    let findings = common::parse_findings(&sink.0);
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Finding::Result");
    };
    let arr = r.effect.extra.get("q_stats").expect("q_stats present");
    let mut out = Vec::with_capacity(arr.data.0.len() / 8);
    for chunk in arr.data.0.chunks_exact(8) {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(chunk);
        out.push(f64::from_le_bytes(buf));
    }
    out
}

/// Run `VolRollingScan` against `closes_slice` and return the
/// `effect.extra["values"]` rolling-vol vector as `Vec<f64>`. Mirrors
/// `run_and_extract_q_stats` for the LjungBox proptest above.
fn run_and_extract_vol_values(closes_slice: &[f64], window: usize) -> Vec<f64> {
    let bars = bar_frame_from_closes(closes_slice);
    let resolved_params = serde_json::json!({"window": window});
    let param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = start + Duration::minutes(15 * i64::try_from(closes_slice.len()).unwrap());
    let req = ScanRequest {
        scan_id: "stats.vol.rolling".into(),
        version: 1,
        instruments: vec![miner_core::reader::InstrumentSpec {
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
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };
    let mut sink = BufferSink::new();
    VolRollingScan.run(&ctx, &req, &mut sink).expect("scan ok");
    let findings = common::parse_findings(&sink.0);
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Finding::Result");
    };
    let arr = r.effect.extra.get("values").expect("values present");
    let mut out = Vec::with_capacity(arr.data.0.len() / 8);
    for chunk in arr.data.0.chunks_exact(8) {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(chunk);
        out.push(f64::from_le_bytes(buf));
    }
    out
}

proptest! {
    /// D3-09 — Ljung-Box stats up to time T MUST be byte-identical when bars
    /// at index >T are shuffled.
    ///
    /// This is the full D3-09 enforcement for Ljung-Box (a single-shot, non-rolling
    /// scan). Phase 4 will ADD additional cancellation_tests-style proptests for each
    /// new rolling/causal scan it introduces — it does NOT extend this proptest.
    #[test]
    fn look_ahead_safe_under_post_t_shuffle(seed in 0u64..1_000) {
        #[allow(clippy::cast_possible_truncation)]
        let seed_u32 = seed as u32;
        let closes = lcg_closes(N, seed_u32);

        // Pre-T stats from the unshuffled series, sliced to T+1 bars (=> T
        // returns, of which LAGS are used).
        let pre_t = &closes[..=T];
        let q_pre = run_and_extract_q_stats(pre_t);

        // Shuffle the post-T tail in place; pre-T bytes unchanged.
        let mut shuffled = closes.clone();
        shuffle_in_place(&mut shuffled[T + 1..], seed_u32);
        let post_shuffle_pre_t = &shuffled[..=T];
        let q_post = run_and_extract_q_stats(post_shuffle_pre_t);

        // Byte-identical Q-stats: the kernel reads only the supplied slice
        // (D3-09 structural invariant for single-shot scans).
        prop_assert_eq!(
            q_pre.clone(),
            q_post.clone(),
            "pre-T Q-stats differ after post-T shuffle (seed={}); before: {:?}, after: {:?}",
            seed,
            q_pre,
            q_post,
        );
    }

    /// Phase 4 Plan 04-03 Pattern M — ANOM-03 stats.vol.rolling@1 rolling
    /// vol values up to time T MUST be byte-identical when bars at index
    /// >T are shuffled. The kernel iterates only over the supplied slice;
    /// look-ahead-safety is a structural property (T-04-03-02 mitigation).
    ///
    /// We run the scan on the prefix [0..=T] and on the post-shuffle full
    /// array, then compare the values-vector prefix that ends at the same
    /// rolling-window index (n_windows_pre is bounded by the prefix's
    /// returns count).
    #[test]
    fn vol_rolling_shuffled_future_invariant(seed in 0u64..1_000) {
        #[allow(clippy::cast_possible_truncation)]
        let seed_u32 = seed as u32;
        let closes = lcg_closes(N, seed_u32);
        let window = 8usize;

        // Pre-T values from the unshuffled prefix.
        let pre_t = &closes[..=T];
        let values_pre = run_and_extract_vol_values(pre_t, window);

        // Shuffle the post-T tail in place; pre-T bytes unchanged.
        let mut shuffled = closes.clone();
        shuffle_in_place(&mut shuffled[T + 1..], seed_u32);
        let values_post_full = run_and_extract_vol_values(&shuffled, window);

        // The prefix-bound rolling windows are exactly `values_pre.len()`
        // many (the prefix returns count - window + 1). Compare the
        // first `values_pre.len()` elements of the full-array vector
        // byte-identically.
        let prefix_len = values_pre.len();
        prop_assert!(
            values_post_full.len() >= prefix_len,
            "full-array values vector ({}) must be >= prefix vector ({})",
            values_post_full.len(),
            prefix_len,
        );
        for i in 0..prefix_len {
            let a = values_pre[i];
            let b = values_post_full[i];
            prop_assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "pre-T vol[{}] differs after post-T shuffle (seed={}, window={}): pre={}, post={}",
                i,
                seed,
                window,
                a,
                b,
            );
        }
    }
}

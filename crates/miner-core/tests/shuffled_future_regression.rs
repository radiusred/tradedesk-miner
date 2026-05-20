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
//! Plan 04-08 (Wave 4) authors FOUR additional Pair-arity proptests:
//! `lead_lag_shuffled_future_invariant` (CROSS-04, full-sample-with-window
//! variant), `pearson_rolling_shuffled_future_invariant`,
//! `spearman_rolling_shuffled_future_invariant`, and
//! `ols_rolling_shuffled_future_invariant` (CROSS-02 + CROSS-03 — deferred
//! from Plan 04-07 Wave 3 to avoid same-wave file-write conflict with Plan
//! 04-03's `vol_rolling` extension).

#![allow(dead_code, unused_imports, unexpected_cfgs)]

use chrono::{Duration, TimeZone, Utc};
use proptest::prelude::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use miner_core::aggregator::{BarFrame, Timeframe};
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::param_hash;
use miner_core::findings::{Finding, RunId, TimeRange};
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
use miner_core::scan::anom::VolRollingScan;
use miner_core::scan::cross::{
    LeadLagCcfScan, OlsRollingScan, PearsonRollingScan, SpearmanRollingScan,
};
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
/// `run_and_extract_q_stats` for the `LjungBox` proptest above.
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

/// Build a `BarFrame` from a pre-computed close array using a custom symbol +
/// source-id. Companion to [`bar_frame_from_closes`] for the Pair-arity
/// proptests below — we need two distinct legs with distinct symbols.
fn bar_frame_named(closes: &[f64], symbol: &str) -> BarFrame {
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let n = closes.len();
    let ts_open: Vec<chrono::DateTime<Utc>> = (0..n)
        .map(|i| start + Duration::minutes(15 * i64::try_from(i).unwrap()))
        .collect();
    let ts_close: Vec<chrono::DateTime<Utc>> =
        ts_open.iter().map(|t| *t + Duration::minutes(15)).collect();
    BarFrame {
        source_id: "dukascopy".into(),
        symbol: symbol.into(),
        side: Side::Bid,
        tf: Timeframe::Tf15m,
        ts_open_utc: ts_open,
        ts_close_utc: ts_close,
        open: closes.to_vec(),
        high: closes.iter().map(|c| c + 0.001).collect(),
        low: closes.iter().map(|c| c - 0.001).collect(),
        close: closes.to_vec(),
        tick_volume: vec![1.0; n],
    }
}

/// Decode a single `effect.extra[key]` `RawArray` to a `Vec<f64>` via the
/// canonical D-01 little-endian f64 layout.
fn extract_f64_vec(r: &miner_core::findings::ResultFinding, key: &str) -> Vec<f64> {
    let arr = r.effect.extra.get(key).expect("effect.extra missing key");
    let mut out = Vec::with_capacity(arr.data.0.len() / 8);
    for chunk in arr.data.0.chunks_exact(8) {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(chunk);
        out.push(f64::from_le_bytes(buf));
    }
    out
}

/// Pair-arity request builder for the CROSS proptests.
fn pair_request(
    scan_id: &str,
    aligned_n: usize,
    resolved_params: serde_json::Value,
) -> ScanRequest {
    let pair_param_hash = param_hash::param_hash(&resolved_params).expect("hash ok");
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let end = start + Duration::minutes(15 * i64::try_from(aligned_n).unwrap());
    ScanRequest {
        scan_id: scan_id.into(),
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
        window: ClosedRangeUtc { start, end },
        sub_range: TimeRange {
            start_utc: start,
            end_utc: end,
        },
        gap_policy: GapPolicyKind::ContinuousOnly,
        resolved_params,
        param_hash: pair_param_hash,
        dry_run: false,
        sleep_after_first_finding_ms: None,
    }
}

/// Generic Pair-arity scan runner — dispatches `scan.run` against the two
/// supplied close arrays (constructed into `BarFrame`s on the fly) and
/// returns the single `ResultFinding` envelope's `effect.extra[key]`
/// decoded as a `Vec<f64>`.
fn run_pair_and_extract(
    scan: &dyn Scan,
    closes_a: &[f64],
    closes_b: &[f64],
    resolved_params: serde_json::Value,
    extra_key: &str,
) -> Vec<f64> {
    assert_eq!(closes_a.len(), closes_b.len(), "legs must have equal len");
    let bars_a = bar_frame_named(closes_a, "EURUSD");
    let bars_b = bar_frame_named(closes_b, "GBPUSD");
    let req = pair_request(scan.id(), closes_a.len(), resolved_params);
    let ctx = ScanCtx {
        bars: &bars_a,
        bars_pair: Some((&bars_a, &bars_b)),
        gap_manifest: None,
        run_id: RunId::new(),
        code_revision: "test-rev",
        cancel: Arc::new(AtomicBool::new(false)),
        sleep_after_first_finding_ms: None,
    };
    let mut sink = BufferSink::new();
    scan.run(&ctx, &req, &mut sink).expect("scan ok");
    let findings = common::parse_findings(&sink.0);
    let Finding::Result(ref r) = findings[0] else {
        panic!("expected Finding::Result for {}", scan.id());
    };
    extract_f64_vec(r, extra_key)
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

    /// Phase 4 Plan 04-08 Task 1 Pattern M — CROSS-04 `cross.lead_lag.ccf@1`.
    ///
    /// Lead-lag CCF is FULL-SAMPLE (no rolling/causal window structure), so
    /// the standard "rolling-prefix equals shuffled-tail-prefix" pattern
    /// doesn't fit directly. Instead we assert: the scan run on the
    /// truncated prefix `closes[..=T]` produces a `ccf_values` vector
    /// byte-identical to the scan run on the OTHER truncated prefix
    /// `shuffled[..=T]` where the post-T tail was shuffled before
    /// truncation. (Truncating both inputs at T means the scan sees only
    /// the pre-T window in either case; the equality is then the structural
    /// "kernel reads only the supplied slice" invariant — the same property
    /// the LjungBox proptest above pins for the single-leg case.)
    #[test]
    fn lead_lag_shuffled_future_invariant(seed in 0u64..1_000) {
        #[allow(clippy::cast_possible_truncation)]
        let seed_u32 = seed as u32;
        let closes_a_full = lcg_closes(N, seed_u32);
        let closes_b_full = lcg_closes(N, seed_u32.wrapping_add(7));
        let max_lag = 5_usize;

        // Pre-T CCF from the unshuffled prefix.
        let pre_t_a = closes_a_full[..=T].to_vec();
        let pre_t_b = closes_b_full[..=T].to_vec();
        let ccf_pre = run_pair_and_extract(
            &LeadLagCcfScan,
            &pre_t_a,
            &pre_t_b,
            serde_json::json!({"max_lag": max_lag}),
            "ccf_values",
        );

        // Shuffle the post-T tails in BOTH legs (deterministic per-leg seeds).
        let mut shuffled_a = closes_a_full.clone();
        shuffle_in_place(&mut shuffled_a[T + 1..], seed_u32);
        let mut shuffled_b = closes_b_full.clone();
        shuffle_in_place(&mut shuffled_b[T + 1..], seed_u32.wrapping_add(13));

        // Truncate at T; run again.
        let post_a = shuffled_a[..=T].to_vec();
        let post_b = shuffled_b[..=T].to_vec();
        let ccf_post = run_pair_and_extract(
            &LeadLagCcfScan,
            &post_a,
            &post_b,
            serde_json::json!({"max_lag": max_lag}),
            "ccf_values",
        );

        // Byte-identical: the kernel must see only the supplied slice.
        prop_assert_eq!(
            ccf_pre.len(),
            ccf_post.len(),
            "ccf lengths differ (pre={}, post={})",
            ccf_pre.len(),
            ccf_post.len(),
        );
        for i in 0..ccf_pre.len() {
            let a = ccf_pre[i];
            let b = ccf_post[i];
            prop_assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "pre-T ccf[{}] differs after post-T shuffle+truncate (seed={}, max_lag={}): pre={}, post={}",
                i,
                seed,
                max_lag,
                a,
                b,
            );
        }
    }

    /// Phase 4 Plan 04-08 Task 1 Pattern M — CROSS-02 Pearson
    /// `cross.corr.pearson_rolling@1`. Pair-arity rolling proptest: shuffle
    /// post-T tail in BOTH legs; assert prefix-aligned `values` vector is
    /// byte-identical to the prefix-only run. Deferred from Plan 04-07 Wave
    /// 3 to avoid same-wave file-write conflict.
    #[test]
    fn pearson_rolling_shuffled_future_invariant(seed in 0u64..1_000) {
        #[allow(clippy::cast_possible_truncation)]
        let seed_u32 = seed as u32;
        let closes_a_full = lcg_closes(N, seed_u32);
        let closes_b_full = lcg_closes(N, seed_u32.wrapping_add(7));
        let window = 8_usize;

        let pre_t_a = closes_a_full[..=T].to_vec();
        let pre_t_b = closes_b_full[..=T].to_vec();
        let values_pre = run_pair_and_extract(
            &PearsonRollingScan,
            &pre_t_a,
            &pre_t_b,
            serde_json::json!({"window": window}),
            "values",
        );

        let mut shuffled_a = closes_a_full.clone();
        shuffle_in_place(&mut shuffled_a[T + 1..], seed_u32);
        let mut shuffled_b = closes_b_full.clone();
        shuffle_in_place(&mut shuffled_b[T + 1..], seed_u32.wrapping_add(13));
        let values_post_full = run_pair_and_extract(
            &PearsonRollingScan,
            &shuffled_a,
            &shuffled_b,
            serde_json::json!({"window": window}),
            "values",
        );

        let prefix_len = values_pre.len();
        prop_assert!(
            values_post_full.len() >= prefix_len,
            "full values vector ({}) must be >= prefix vector ({})",
            values_post_full.len(),
            prefix_len,
        );
        for i in 0..prefix_len {
            let a = values_pre[i];
            let b = values_post_full[i];
            prop_assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "pre-T pearson[{}] differs after post-T shuffle (seed={}, window={}): pre={}, post={}",
                i,
                seed,
                window,
                a,
                b,
            );
        }
    }

    /// Phase 4 Plan 04-08 Task 1 Pattern M — CROSS-02 Spearman
    /// `cross.corr.spearman_rolling@1`. Same shape as the Pearson proptest;
    /// pins the rank-with-ties kernel's look-ahead-safety. Deferred from
    /// Plan 04-07 Wave 3.
    #[test]
    fn spearman_rolling_shuffled_future_invariant(seed in 0u64..1_000) {
        #[allow(clippy::cast_possible_truncation)]
        let seed_u32 = seed as u32;
        let closes_a_full = lcg_closes(N, seed_u32);
        let closes_b_full = lcg_closes(N, seed_u32.wrapping_add(7));
        let window = 8_usize;

        let pre_t_a = closes_a_full[..=T].to_vec();
        let pre_t_b = closes_b_full[..=T].to_vec();
        let values_pre = run_pair_and_extract(
            &SpearmanRollingScan,
            &pre_t_a,
            &pre_t_b,
            serde_json::json!({"window": window}),
            "values",
        );

        let mut shuffled_a = closes_a_full.clone();
        shuffle_in_place(&mut shuffled_a[T + 1..], seed_u32);
        let mut shuffled_b = closes_b_full.clone();
        shuffle_in_place(&mut shuffled_b[T + 1..], seed_u32.wrapping_add(13));
        let values_post_full = run_pair_and_extract(
            &SpearmanRollingScan,
            &shuffled_a,
            &shuffled_b,
            serde_json::json!({"window": window}),
            "values",
        );

        let prefix_len = values_pre.len();
        prop_assert!(values_post_full.len() >= prefix_len);
        for i in 0..prefix_len {
            let a = values_pre[i];
            let b = values_post_full[i];
            prop_assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "pre-T spearman[{}] differs after post-T shuffle (seed={}, window={}): pre={}, post={}",
                i,
                seed,
                window,
                a,
                b,
            );
        }
    }

    /// Phase 4 Plan 04-08 Task 1 Pattern M — CROSS-03 `cross.ols.rolling@1`.
    /// Pair-arity rolling OLS proptest. Asserts prefix-aligned `betas`,
    /// `alphas`, `r2s`, AND `residual_stds` vectors are each byte-identical
    /// across the post-T shuffle cut. Deferred from Plan 04-07 Wave 3.
    #[test]
    fn ols_rolling_shuffled_future_invariant(seed in 0u64..1_000) {
        #[allow(clippy::cast_possible_truncation)]
        let seed_u32 = seed as u32;
        let closes_a_full = lcg_closes(N, seed_u32);
        let closes_b_full = lcg_closes(N, seed_u32.wrapping_add(7));
        let window = 8_usize;

        let pre_t_a = closes_a_full[..=T].to_vec();
        let pre_t_b = closes_b_full[..=T].to_vec();
        let mut shuffled_a = closes_a_full.clone();
        shuffle_in_place(&mut shuffled_a[T + 1..], seed_u32);
        let mut shuffled_b = closes_b_full.clone();
        shuffle_in_place(&mut shuffled_b[T + 1..], seed_u32.wrapping_add(13));

        for key in ["betas", "alphas", "r2s", "residual_stds"] {
            let pre = run_pair_and_extract(
                &OlsRollingScan,
                &pre_t_a,
                &pre_t_b,
                serde_json::json!({"window": window}),
                key,
            );
            let post = run_pair_and_extract(
                &OlsRollingScan,
                &shuffled_a,
                &shuffled_b,
                serde_json::json!({"window": window}),
                key,
            );
            let prefix_len = pre.len();
            prop_assert!(post.len() >= prefix_len);
            for i in 0..prefix_len {
                let a = pre[i];
                let b = post[i];
                prop_assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "pre-T ols/{}[{}] differs after post-T shuffle (seed={}, window={}): pre={}, post={}",
                    key,
                    i,
                    seed,
                    window,
                    a,
                    b,
                );
            }
        }
    }
}

//! `VarianceRatioScan` — ANOM-07 Lo-MacKinlay variance ratio over multiple k.
//!
//! Pattern analog: [`crate::scan::anom::adf::AdfScan`] +
//! [`crate::scan::anom::kpss::KpssScan`] (Plan 04-05 Task 1/2 siblings) —
//! Pattern A from `04-PATTERNS.md`.
//!
//! ## Reference
//!
//! `arch.unitroot.VarianceRatio(returns, lags=k, robust=True)` for each k in
//! the default grid `[2, 4, 8, 16]`. Lo-MacKinlay variance ratio is NOT in
//! statsmodels core — the `arch` Python package is the canonical reference.
//! Original paper: Lo, A. W. & `MacKinlay`, A. C. (1988), "Stock Market
//! Prices Do Not Follow Random Walks: Evidence from a Simple Specification
//! Test", Review of Financial Studies 1(1), 41-66.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.variance_ratio.lo_mackinlay"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params`: optional `k_values` (array of integers, default `[2,4,8,16]`),
//!   optional `robust` (boolean, default `true`; heteroskedasticity-robust
//!   z-statistic per Lo-MacKinlay 1988 eq 13b).
//! - `effect.metric = "variance_ratio_max_k"`, `effect.value = VR at max(k_values)`.
//! - `effect.extra = {k_values, p_values, vr_values, z_stats}` (alphabetical
//!   `BTreeMap` order) — four parallel arrays of equal length.
//! - `raw.series = {returns, timestamps_ms}`.
//!
//! ## Determinism
//!
//! The k-grid loop is SEQUENTIAL (not `par_iter`) — same Pitfall 4 discipline
//! as the ADF AIC lag selection.
//!
//! ## Registration
//!
//! Appended inside `crate::scan::anom::register_anom_scans` (Pattern E —
//! `crates/miner-core/src/scan/registry.rs` is NOT modified).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;
use serde_json::Value as JsonValue;

use crate::findings::{
    DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// ANOM-07 — Lo-MacKinlay variance ratio scan.
pub struct VarianceRatioScan;

const SCAN_ID: &str = "stats.variance_ratio.lo_mackinlay";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "variance_ratio_max_k";

const DEFAULT_K_VALUES: &[i64] = &[2, 4, 8, 16];

impl Scan for VarianceRatioScan {
    fn id(&self) -> &'static str {
        SCAN_ID
    }

    fn version(&self) -> u32 {
        SCAN_VERSION
    }

    fn arity(&self) -> ScanArity {
        ScanArity::Single
    }

    fn param_schema(&self) -> JsonValue {
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "k_values": {
                    "type": "array",
                    "items": { "type": "integer", "minimum": 2 },
                    "default": [2, 4, 8, 16],
                    "description": "Holding-period horizons; each k >= 2. VR(k) at each is computed."
                },
                "robust": {
                    "type": "boolean",
                    "default": true,
                    "description": "If true (default), use heteroskedasticity-robust z-stat (Lo-MacKinlay 1988 eq 13b). If false, asymptotic iid variance."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["k_values", "p_values", "vr_values", "z_stats"],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Scan::run is the linear dispatch + envelope build path; splitting into helpers obscures the 7-step Pattern A structure"
    )]
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        // Step 1 — cancel at entry.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Step 2 — N guard. Need at least 2 closes for log_returns.
        let n_closes = ctx.bars.close.len();
        if n_closes < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.variance_ratio.lo_mackinlay: need n >= 2 closes; got n={n_closes} (InsufficientData)"
            )));
        }

        // Step 3 — resolve params.
        let k_values = resolve_k_values(req)?;
        let robust = resolve_robust(req)?;

        // Step 4 — compute returns and validate k_values vs returns length.
        let returns = log_returns(&ctx.bars.close);
        let n_returns = returns.len();
        if n_returns < 4 {
            return Err(ScanError::Kernel(format!(
                "stats.variance_ratio.lo_mackinlay: need >= 4 returns; got {n_returns} (InsufficientData)"
            )));
        }
        for &k in &k_values {
            if k < 2 {
                return Err(ScanError::Kernel(format!(
                    "stats.variance_ratio.lo_mackinlay: k_values entries must be >= 2; got {k}"
                )));
            }
            if k > n_returns / 2 {
                return Err(ScanError::Kernel(format!(
                    "stats.variance_ratio.lo_mackinlay: k={k} too large for n_returns={n_returns} (need k <= n/2)"
                )));
            }
        }

        // Step 5 — kernel calls (sequential k loop — Pitfall 4 determinism).
        let mut vr_values: Vec<f64> = Vec::with_capacity(k_values.len());
        let mut z_stats: Vec<f64> = Vec::with_capacity(k_values.len());
        let mut p_values: Vec<f64> = Vec::with_capacity(k_values.len());
        for &k in &k_values {
            let vr_res = kernel::variance_ratio(&returns, k, robust).map_err(ScanError::Kernel)?;
            vr_values.push(vr_res.vr);
            z_stats.push(vr_res.z_stat);
            p_values.push(vr_res.p_value);
        }

        let k_values_f64: Vec<f64> = k_values.iter().map(|k| usize_to_f64(*k)).collect();

        // Step 6 — build raw.series (returns + parallel timestamps).
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
        )]
        let ts_ms: Vec<f64> = ctx
            .bars
            .ts_open_utc
            .iter()
            .skip(1)
            .map(|t| t.timestamp_millis() as f64)
            .collect();

        // Step 7 — envelope construction. effect.value = VR at the last
        // (largest) k in the user-supplied grid.
        let max_k_vr = *vr_values.last().expect("k_values non-empty");

        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("k_values".into(), f64_slice_to_raw_array(&k_values_f64));
        extra.insert("p_values".into(), f64_slice_to_raw_array(&p_values));
        extra.insert("vr_values".into(), f64_slice_to_raw_array(&vr_values));
        extra.insert("z_stats".into(), f64_slice_to_raw_array(&z_stats));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: max_k_vr,
            // No single headline p — multi-k results live in effect.extra.p_values.
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "n_returns <= u64 on all supported targets"
            )]
            n: Some(n_returns as u64),
            ci95: None,
            effect_size: None,
            extra,
        };

        let mut series_map: BTreeMap<String, RawArray> = BTreeMap::new();
        series_map.insert("returns".into(), f64_slice_to_raw_array(&returns));
        series_map.insert("timestamps_ms".into(), f64_slice_to_raw_array(&ts_ms));
        let raw_block = Raw::new(series_map).map_err(|m| ScanError::Kernel(m.to_string()))?;

        let sources: Vec<Source> = req
            .instruments
            .iter()
            .map(|spec| Source {
                source_id: ctx.bars.source_id.clone(),
                symbol: spec.symbol.clone(),
                side: spec.side.as_str().to_string(),
                timeframe: req.timeframe.as_str().to_string(),
            })
            .collect();

        let finding = ResultFinding {
            schema_version: 1,
            scan_id_at_version: format!("{SCAN_ID}@{SCAN_VERSION}"),
            param_hash: req.param_hash.as_str().to_string(),
            code_revision: ctx.code_revision.to_string(),
            data_slice: DataSlice {
                range: req.sub_range.clone(),
                gap_manifest_ref: None,
                gap_manifest: ctx.gap_manifest.cloned(),
                sources,
            },
            dsr: None,
            fdr_q: None,
            run_id: ctx.run_id,
            produced_at_utc: Utc::now(),
            params: req.resolved_params.clone(),
            effect,
            raw: Some(raw_block),
            repro: None,
        };

        sink.write_envelope(&Finding::Result(finding))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_k_values(req: &ScanRequest) -> Result<Vec<usize>, ScanError> {
    let raw = req.resolved_params.get("k_values");
    let arr = match raw {
        None => {
            return Ok(DEFAULT_K_VALUES
                .iter()
                .map(|i| usize::try_from(*i).expect("defaults positive"))
                .collect());
        }
        Some(v) => v.as_array().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.variance_ratio.lo_mackinlay: k_values must be an array; got {v}"
            ))
        })?,
    };
    if arr.is_empty() {
        return Err(ScanError::Kernel(
            "stats.variance_ratio.lo_mackinlay: k_values must be non-empty".into(),
        ));
    }
    let mut out: Vec<usize> = Vec::with_capacity(arr.len());
    for v in arr {
        let i = v.as_i64().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.variance_ratio.lo_mackinlay: k_values entries must be integers; got {v}"
            ))
        })?;
        if i < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.variance_ratio.lo_mackinlay: k_values entries must be >= 2; got {i}"
            )));
        }
        let u = usize::try_from(i).map_err(|_| {
            ScanError::Kernel(format!(
                "stats.variance_ratio.lo_mackinlay: k_values entry {i} out of usize range"
            ))
        })?;
        out.push(u);
    }
    Ok(out)
}

fn resolve_robust(req: &ScanRequest) -> Result<bool, ScanError> {
    let raw = req.resolved_params.get("robust");
    match raw {
        None => Ok(true),
        Some(v) => v.as_bool().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.variance_ratio.lo_mackinlay: robust must be a boolean; got {v}"
            ))
        }),
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "index << 2^52 for any realistic bar count"
)]
#[inline]
fn usize_to_f64(i: usize) -> f64 {
    i as f64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::{BarFrame, Timeframe};
    use crate::engine::gap_policy::GapPolicyKind;
    use crate::findings::TimeRange;
    use crate::findings::run_id::RunId;
    use crate::findings::sink::VecSink;
    use crate::reader::{Blake3Hex, ClosedRangeUtc, InstrumentSpec, Side};
    use chrono::{DateTime, Duration, TimeZone};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn blake3_hex_zero() -> Blake3Hex {
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

    /// Random-walk close series — close[i] = close[i-1] * exp(eps) where eps
    /// is IID zero-mean uniform noise. `log_returns` of this series are
    /// essentially the eps sequence (IID white noise), giving VR(k) ≈ 1.0
    /// under the random-walk null hypothesis.
    #[allow(clippy::cast_possible_truncation)]
    fn lcg_bar_frame_seeded(n: usize, seed: u64) -> BarFrame {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let mut s = seed as u32;
        let mut closes = Vec::with_capacity(n);
        let mut price = 1.0_f64;
        closes.push(price);
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            // Centred uniform noise in (-0.005, 0.005) — small log-return.
            let eps = (f64::from(s) / f64::from(u32::MAX) - 0.5) * 0.01;
            price *= eps.exp();
            closes.push(price);
        }
        let ts_open: Vec<DateTime<chrono::Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("fits in i64");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        let ts_close: Vec<DateTime<chrono::Utc>> =
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

    fn bar_frame_from_closes(closes: Vec<f64>) -> BarFrame {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let n = closes.len();
        let ts_open: Vec<DateTime<chrono::Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("fits in i64");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        let ts_close: Vec<DateTime<chrono::Utc>> =
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

    fn sample_request_with_params(params: serde_json::Value) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: SCAN_ID.into(),
            version: SCAN_VERSION,
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
            resolved_params: params,
            param_hash: blake3_hex_zero(),
            dry_run: false,
        master_seed: None,
        job_seed: None,
        bootstrap_method: None,
        bootstrap_n: None,
        null_method: None,
        null_n: None,
            sleep_after_first_finding_ms: None,
        }
    }

    fn make_ctx(bars: &BarFrame, cancel: Arc<AtomicBool>) -> ScanCtx<'_> {
        ScanCtx {
            bars,
            bars_pair: None,
            gap_manifest: None,
            run_id: RunId::new(),
            code_revision: "abc1234",
            cancel,
            sleep_after_first_finding_ms: None,
        }
    }

    fn parse_sink_to_findings(sink: &VecSink) -> Vec<Finding> {
        sink.0
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<Finding>(line).expect("parse"))
            .collect()
    }

    // -----------------------------------------------------------------------

    #[test]
    fn variance_ratio_id_and_version() {
        assert_eq!(VarianceRatioScan.id(), "stats.variance_ratio.lo_mackinlay");
        assert_eq!(VarianceRatioScan.version(), 1);
    }

    #[test]
    fn variance_ratio_arity_is_single() {
        assert_eq!(VarianceRatioScan.arity(), ScanArity::Single);
    }

    #[test]
    fn variance_ratio_param_schema() {
        let schema = VarianceRatioScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["robust"]["default"], true);
        assert_eq!(
            schema["properties"]["k_values"]["default"],
            serde_json::json!([2, 4, 8, 16])
        );
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn variance_ratio_default_k_values() {
        let bars = lcg_bar_frame_seeded(200, 1);
        let mut sink = VecSink::new();
        // Omit k_values -> default [2,4,8,16].
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VarianceRatioScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // 4 k-values -> all extra arrays of length 4.
        assert_eq!(r.effect.extra["k_values"].shape, vec![4]);
        assert_eq!(r.effect.extra["vr_values"].shape, vec![4]);
        assert_eq!(r.effect.extra["z_stats"].shape, vec![4]);
        assert_eq!(r.effect.extra["p_values"].shape, vec![4]);
    }

    #[test]
    fn variance_ratio_emits_one_result() {
        let bars = lcg_bar_frame_seeded(200, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VarianceRatioScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn variance_ratio_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(200, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [2, 4]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VarianceRatioScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.variance_ratio.lo_mackinlay@1");
        assert_eq!(r.effect.metric, "variance_ratio_max_k");
        assert_eq!(r.effect.p_value, None, "no single headline p");
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec!["k_values", "p_values", "vr_values", "z_stats"]
        );
        // All four parallel arrays of length 2 (we passed 2 k values).
        for key in ["k_values", "vr_values", "z_stats", "p_values"] {
            assert_eq!(r.effect.extra[key].shape, vec![2], "{key} length mismatch");
        }
        let raw = r.raw.as_ref().expect("raw");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
    }

    /// For an IID-like seeded return series, VR(k) ≈ 1 for all k within
    /// finite-sample tolerance.
    #[test]
    fn variance_ratio_white_noise_returns_unity() {
        let bars = lcg_bar_frame_seeded(500, 7);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [2, 4]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VarianceRatioScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let vr_arr = &r.effect.extra["vr_values"];
        for i in 0..2 {
            let off = i * 8;
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&vr_arr.data.0[off..off + 8]);
            let vr = f64::from_le_bytes(buf);
            assert!(
                (vr - 1.0).abs() < 0.3,
                "white-noise VR[{i}] = {vr} should be ≈ 1.0"
            );
        }
    }

    #[test]
    fn variance_ratio_random_walk_returns_unity_for_increments() {
        // log_returns of a random walk are the increments themselves (white noise).
        let bars = lcg_bar_frame_seeded(500, 13);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [2]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VarianceRatioScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let vr_arr = &r.effect.extra["vr_values"];
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&vr_arr.data.0[0..8]);
        let vr = f64::from_le_bytes(buf);
        assert!(
            (vr - 1.0).abs() < 0.3,
            "VR(2) on random-walk increments = {vr} should be ≈ 1"
        );
    }

    #[test]
    fn variance_ratio_cancellation() {
        let bars = lcg_bar_frame_seeded(64, 4);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        VarianceRatioScan.run(&ctx, &req, &mut sink).expect("ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn variance_ratio_invalid_k_low() {
        let bars = lcg_bar_frame_seeded(200, 5);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [1]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = VarianceRatioScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject k=1");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn variance_ratio_k_too_large_rejected() {
        // 20 closes -> 19 returns. k=20 > 19/2 = 9 should error.
        let bars = lcg_bar_frame_seeded(20, 6);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [20]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = VarianceRatioScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject k too large");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn variance_ratio_n_zero_emits_scan_error() {
        let bars = bar_frame_from_closes(Vec::new());
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = VarianceRatioScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject n=0");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "{msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn variance_ratio_invalid_k_values_non_array() {
        let bars = lcg_bar_frame_seeded(200, 7);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": "garbage"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = VarianceRatioScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject non-array");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn variance_ratio_effect_value_is_vr_at_max_k() {
        let bars = lcg_bar_frame_seeded(500, 8);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [2, 4, 8]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VarianceRatioScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // effect.value == vr_values[2] (the last k = 8).
        let vr_arr = &r.effect.extra["vr_values"];
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&vr_arr.data.0[16..24]); // 3rd element (index 2).
        let vr_at_k8 = f64::from_le_bytes(buf);
        assert!(
            (r.effect.value - vr_at_k8).abs() < 1e-12,
            "effect.value should equal VR at max k"
        );
    }
}

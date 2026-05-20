//! `JarqueBeraScan` — ANOM-09 Jarque-Bera normality test.
//!
//! Pattern analog: [`crate::scan::anom::outliers::OutliersZAndMadScan`] —
//! Pattern A from `04-PATTERNS.md`. Both surfaces ship the
//! `series ∈ {"log_returns","close"}` enum so the agent can choose which
//! statistic to test for normality.
//!
//! ## Reference
//!
//! `scipy.stats.jarque_bera(x)` — emits `(jb_stat, p_value)`. The kernel
//! reuses `welford_pass` from `anom::summary::kernel` to keep the moments
//! byte-identical with ANOM-02 (`stats.summary.welford@1`).
//!
//! ## D4-02 surface
//!
//! - `id = "stats.normality.jarque_bera"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params`: optional `series` enum `["log_returns", "close"]`, default
//!   `"log_returns"`.
//! - `effect.metric = "jarque_bera_statistic"`,
//!   `effect.value = JB stat`, `effect.p_value = 1 - ChiSquared(2).cdf(JB)`.
//! - `effect.extra = {excess_kurtosis, n, p_value, skew}` (alphabetical
//!   `BTreeMap` order).
//! - `raw.series = {returns, timestamps_ms}` under default; `{closes,
//!   timestamps_ms}` under `series=close`.
//!
//! ## Constant-input rejection
//!
//! If the resolved input series has zero variance (`std == 0`), the kernel
//! returns `Err` and the scan body surfaces this as `ScanError::Kernel`
//! per T-04-06-02 (mirrors ANOM-10 outliers constant-input handling).
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

/// ANOM-09 — Jarque-Bera normality test scan.
pub struct JarqueBeraScan;

const SCAN_ID: &str = "stats.normality.jarque_bera";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "jarque_bera_statistic";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JbSeries {
    LogReturns,
    Close,
}

impl JbSeries {
    fn as_str(self) -> &'static str {
        match self {
            JbSeries::LogReturns => "log_returns",
            JbSeries::Close => "close",
        }
    }
}

impl Scan for JarqueBeraScan {
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
                "series": {
                    "type": "string",
                    "enum": ["log_returns", "close"],
                    "default": "log_returns",
                    "description": "Input series for the JB test."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["excess_kurtosis", "n", "p_value", "skew"],
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

        // Step 2 — resolve params.
        let series = resolve_series(req)?;

        // Step 3 — N=0 / N=1 guard on raw closes.
        let n_closes = ctx.bars.close.len();
        if n_closes == 0 {
            return Err(ScanError::Kernel(
                "stats.normality.jarque_bera: empty close series (InsufficientData)".to_string(),
            ));
        }
        if n_closes < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.normality.jarque_bera: need n >= 2 closes; got n={n_closes} (InsufficientData)"
            )));
        }

        // Step 4 — compute target series + parallel timestamps.
        let (values, series_label, ts_ms): (Vec<f64>, &'static str, Vec<f64>) = match series {
            JbSeries::LogReturns => {
                let returns = log_returns(&ctx.bars.close);
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
                )]
                let ts: Vec<f64> = ctx
                    .bars
                    .ts_open_utc
                    .iter()
                    .skip(1) // returns aligned with bars[1..].
                    .map(|t| t.timestamp_millis() as f64)
                    .collect();
                (returns, "returns", ts)
            }
            JbSeries::Close => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
                )]
                let ts: Vec<f64> = ctx
                    .bars
                    .ts_open_utc
                    .iter()
                    .map(|t| t.timestamp_millis() as f64)
                    .collect();
                (ctx.bars.close.clone(), "closes", ts)
            }
        };

        let n = values.len();
        if n < 4 {
            return Err(ScanError::Kernel(format!(
                "stats.normality.jarque_bera: series={} produced n={n} (need >= 4 for bias-corrected kurtosis; InsufficientData)",
                series.as_str()
            )));
        }

        // Step 5 — kernel call. Constant-input -> Err -> ScanError::Kernel
        // (T-04-06-02).
        let result = kernel::jarque_bera(&values).map_err(ScanError::Kernel)?;

        // Step 6 — envelope construction.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "excess_kurtosis".into(),
            f64_slice_to_raw_array(&[result.excess_kurtosis]),
        );
        extra.insert(
            "n".into(),
            f64_slice_to_raw_array(&[index_to_f64(result.n)]),
        );
        extra.insert("p_value".into(), f64_slice_to_raw_array(&[result.p_value]));
        extra.insert("skew".into(), f64_slice_to_raw_array(&[result.skew]));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: result.statistic,
            p_value: Some(result.p_value),
            #[allow(
                clippy::cast_possible_truncation,
                reason = "n <= u64 on all supported targets"
            )]
            n: Some(n as u64),
            ci95: None,
            extra,
        };

        let mut series_map: BTreeMap<String, RawArray> = BTreeMap::new();
        series_map.insert(series_label.into(), f64_slice_to_raw_array(&values));
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
        };

        sink.write_envelope(&Finding::Result(finding))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_series(req: &ScanRequest) -> Result<JbSeries, ScanError> {
    let raw = req.resolved_params.get("series");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.normality.jarque_bera: series must be log_returns|close; got {v}"
            ))
        })?,
        None => "log_returns",
    };
    match label {
        "log_returns" => Ok(JbSeries::LogReturns),
        "close" => Ok(JbSeries::Close),
        other => Err(ScanError::Kernel(format!(
            "stats.normality.jarque_bera: series must be log_returns|close; got {other:?}"
        ))),
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "index << 2^52 for any realistic bar count"
)]
#[inline]
fn index_to_f64(i: usize) -> f64 {
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

    #[allow(clippy::cast_possible_truncation)]
    fn lcg_bar_frame_seeded(n: usize, seed: u64) -> BarFrame {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let mut s = seed as u32;
        let mut closes = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            closes.push(1.0 + frac);
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
    fn jarque_bera_id_and_version() {
        assert_eq!(JarqueBeraScan.id(), "stats.normality.jarque_bera");
        assert_eq!(JarqueBeraScan.version(), 1);
    }

    #[test]
    fn jarque_bera_arity_is_single() {
        assert_eq!(JarqueBeraScan.arity(), ScanArity::Single);
    }

    #[test]
    fn jarque_bera_param_schema() {
        let schema = JarqueBeraScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["series"]["default"], "log_returns");
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn jarque_bera_emits_one_result() {
        let bars = lcg_bar_frame_seeded(80, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        JarqueBeraScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn jarque_bera_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(80, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "log_returns"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        JarqueBeraScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.normality.jarque_bera@1");
        assert_eq!(r.effect.metric, "jarque_bera_statistic");
        assert!(r.effect.p_value.is_some());
        // 80 closes -> 79 log_returns.
        assert_eq!(r.effect.n, Some(79));
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(extra_keys, vec!["excess_kurtosis", "n", "p_value", "skew"]);
        let raw = r.raw.as_ref().expect("raw");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    #[test]
    fn jarque_bera_cancellation() {
        let bars = lcg_bar_frame_seeded(80, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        JarqueBeraScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn jarque_bera_n_zero_emits_scan_error() {
        let bars = bar_frame_from_closes(Vec::new());
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = JarqueBeraScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject n=0");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn jarque_bera_n_one_emits_scan_error() {
        let bars = bar_frame_from_closes(vec![1.5_f64]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = JarqueBeraScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject n=1");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    /// T-04-06-02 — constant series -> std == 0 -> `ScanError::Kernel`.
    #[test]
    fn jarque_bera_constant_input_emits_scan_error() {
        let bars = bar_frame_from_closes(vec![1.5_f64; 20]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "close"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = JarqueBeraScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject constant input");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn jarque_bera_invalid_series_rejected() {
        let bars = lcg_bar_frame_seeded(40, 4);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "garbage"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = JarqueBeraScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject invalid series");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    /// For a roughly-Gaussian input (sum of 12 uniforms ≈ N(6,1) via CLT),
    /// the JB statistic should be small.
    #[test]
    fn jarque_bera_approximate_gaussian_does_not_reject() {
        let n = 200;
        let mut closes = Vec::with_capacity(n);
        let mut s: u32 = 123;
        let mut prev = 1.0_f64;
        closes.push(prev);
        for _ in 1..n {
            let mut total = 0.0_f64;
            for _ in 0..12 {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                total += f64::from(s) / f64::from(u32::MAX);
            }
            // approx-Gaussian increment.
            let g = total - 6.0;
            prev *= 1.0 + 0.001 * g;
            closes.push(prev);
        }
        let bars = bar_frame_from_closes(closes);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "log_returns"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        JarqueBeraScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // chi²(2) 5% critical = 5.991. Approx-Gaussian input should sit
        // well below this; generous bound for LCG stability.
        assert!(
            r.effect.value < 15.0,
            "JB = {} should be small for approx-Gaussian log_returns",
            r.effect.value
        );
    }
}

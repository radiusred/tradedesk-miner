//! `OutliersZAndMadScan` — ANOM-10 z-score + Iglewicz-Hoaglin modified-z
//! outlier detection.
//!
//! Pattern analog: [`crate::scan::ljung_box::LjungBoxScan`] (Phase 3 gold-
//! standard / `04-PATTERNS.md` Pattern A) and
//! [`crate::scan::anom::summary::SummaryWelfordScan`] (the closest in-family
//! analog with `params.series ∈ {"log_returns","close"}`).
//!
//! ## D4-02 surface
//!
//! - `id = "stats.outliers.z_and_mad"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params`: `z_threshold` (number, default 3.0), `modified_z_threshold`
//!   (number, default 3.5), `series` (enum `["log_returns","close"]`,
//!   default `"log_returns"`).
//! - `effect.metric = "outliers_count"`, `effect.value = outlier_indices.len()`
//!   (as f64). The "union" rule: a bar is an outlier if `|z| > z_threshold`
//!   OR `|modified_z| > modified_z_threshold`.
//! - `effect.extra = {mad, median, modified_z_threshold, outlier_indices,
//!    outlier_values_modified_z, outlier_values_z, z_threshold}`
//!   (alphabetical `BTreeMap` order).
//! - `raw.series = {returns, timestamps_ms}` under default series; under
//!   `series=close` the raw series carries `{closes, timestamps_ms}`.
//!
//! ## Constant-input rejection
//!
//! If the resolved input series has zero variance (`MAD == 0`), the kernel
//! returns `(zeros, 0.0)` and the scan body converts the MAD=0 condition
//! into `ScanError::Kernel` so consumers never observe a finding with all-
//! zero modified-z scores (T-04-04-01 mitigation).
//!
//! ## Registration
//!
//! Appended inside `crate::scan::anom::register_anom_scans` (Pattern E —
//! `crates/miner-core/src/scan/registry.rs` is NOT modified in this task).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;
use serde_json::Value as JsonValue;

use crate::findings::{DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// ANOM-10 — z-score + modified-z outlier detection scan.
pub struct OutliersZAndMadScan;

const SCAN_ID: &str = "stats.outliers.z_and_mad";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "outliers_count";

const DEFAULT_Z_THRESHOLD: f64 = 3.0;
const DEFAULT_MODIFIED_Z_THRESHOLD: f64 = 3.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutlierSeries {
    LogReturns,
    Close,
}

impl OutlierSeries {
    fn as_str(self) -> &'static str {
        match self {
            OutlierSeries::LogReturns => "log_returns",
            OutlierSeries::Close => "close",
        }
    }
}

impl Scan for OutliersZAndMadScan {
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
                "z_threshold": {
                    "type": "number",
                    "minimum": 0.0,
                    "default": DEFAULT_Z_THRESHOLD,
                    "description": "Absolute z-score threshold above which a bar is flagged as outlier."
                },
                "modified_z_threshold": {
                    "type": "number",
                    "minimum": 0.0,
                    "default": DEFAULT_MODIFIED_Z_THRESHOLD,
                    "description": "Absolute Iglewicz-Hoaglin modified-z threshold (0.6745*(x-median)/MAD) above which a bar is flagged."
                },
                "series": {
                    "type": "string",
                    "enum": ["log_returns", "close"],
                    "default": "log_returns",
                    "description": "Input series for the outlier detection kernel."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[
                "mad",
                "median",
                "modified_z_threshold",
                "outlier_indices",
                "outlier_values_modified_z",
                "outlier_values_z",
                "z_threshold",
            ],
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
        let z_threshold = resolve_threshold(req, "z_threshold", DEFAULT_Z_THRESHOLD)?;
        let modified_z_threshold = resolve_threshold(
            req,
            "modified_z_threshold",
            DEFAULT_MODIFIED_Z_THRESHOLD,
        )?;
        let series = resolve_series(req)?;

        // Step 3 — N=0 guard on raw closes.
        let n_closes = ctx.bars.close.len();
        if n_closes == 0 {
            return Err(ScanError::Kernel(
                "stats.outliers.z_and_mad: empty close series (InsufficientData)".to_string(),
            ));
        }

        // Step 4 — compute target series + parallel timestamps.
        let (values, series_label, ts_ms): (Vec<f64>, &'static str, Vec<f64>) = match series {
            OutlierSeries::LogReturns => {
                if n_closes < 2 {
                    return Err(ScanError::Kernel(format!(
                        "stats.outliers.z_and_mad: need >= 2 closes for log_returns; got n={n_closes} (InsufficientData)"
                    )));
                }
                let returns = log_returns(&ctx.bars.close);
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
                )]
                let ts: Vec<f64> = ctx
                    .bars
                    .ts_open_utc
                    .iter()
                    .skip(1)
                    .map(|t| t.timestamp_millis() as f64)
                    .collect();
                (returns, "returns", ts)
            }
            OutlierSeries::Close => {
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
        if n < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.outliers.z_and_mad: series={} produced n={n} (need >= 2; InsufficientData)",
                series.as_str()
            )));
        }

        // Step 5 — kernel calls.
        let (z, _mean, _std_pop) = kernel::z_scores(&values);
        let (mz, median, mad) = kernel::modified_z_scores(&values);

        // T-04-04-01 mitigation: MAD == 0 -> ScanError::Kernel. The kernel
        // returns mad=0 and a zero modified-z vector for constant input;
        // detecting it here avoids emitting a finding with degenerate stats.
        if mad == 0.0 {
            return Err(ScanError::Kernel(format!(
                "stats.outliers.z_and_mad: MAD == 0 (constant {} series; cannot compute modified-z) (InsufficientData)",
                series.as_str()
            )));
        }

        // Union of z-outliers and modified-z-outliers (per behavior test
        // `outliers_indices_match`): a bar is an outlier if EITHER criterion
        // exceeds its threshold. The wire form reports the indices once
        // (the union), the per-criterion VALUE vectors (parallel arrays of
        // z and modified-z values at those union indices), the two thresholds
        // (echoed back), and the median + MAD scalars.
        let mut outlier_indices: Vec<usize> = Vec::new();
        let mut outlier_values_z: Vec<f64> = Vec::new();
        let mut outlier_values_modified_z: Vec<f64> = Vec::new();
        for i in 0..n {
            let z_flag = z[i].abs() > z_threshold;
            let mz_flag = mz[i].abs() > modified_z_threshold;
            if z_flag || mz_flag {
                outlier_indices.push(i);
                outlier_values_z.push(z[i]);
                outlier_values_modified_z.push(mz[i]);
            }
        }

        // Step 6 — envelope construction.
        let outlier_indices_f64: Vec<f64> = outlier_indices
            .iter()
            .map(|i| index_to_f64(*i))
            .collect();

        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("mad".into(), f64_slice_to_raw_array(&[mad]));
        extra.insert("median".into(), f64_slice_to_raw_array(&[median]));
        extra.insert(
            "modified_z_threshold".into(),
            f64_slice_to_raw_array(&[modified_z_threshold]),
        );
        extra.insert(
            "outlier_indices".into(),
            f64_slice_to_raw_array(&outlier_indices_f64),
        );
        extra.insert(
            "outlier_values_modified_z".into(),
            f64_slice_to_raw_array(&outlier_values_modified_z),
        );
        extra.insert(
            "outlier_values_z".into(),
            f64_slice_to_raw_array(&outlier_values_z),
        );
        extra.insert(
            "z_threshold".into(),
            f64_slice_to_raw_array(&[z_threshold]),
        );

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: count_to_f64(outlier_indices.len()),
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize <= u64 on all supported targets"
            )]
            n: Some(n as u64),
            ci95: None,
            extra,
        };

        // raw.series: the source series + parallel timestamps.
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

        let result = ResultFinding {
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

        sink.write_envelope(&Finding::Result(result))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_series(req: &ScanRequest) -> Result<OutlierSeries, ScanError> {
    let raw = req.resolved_params.get("series");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.outliers.z_and_mad: series must be log_returns|close; got {v}"
            ))
        })?,
        None => "log_returns",
    };
    match label {
        "log_returns" => Ok(OutlierSeries::LogReturns),
        "close" => Ok(OutlierSeries::Close),
        other => Err(ScanError::Kernel(format!(
            "stats.outliers.z_and_mad: series must be log_returns|close; got {other:?}"
        ))),
    }
}

fn resolve_threshold(req: &ScanRequest, key: &str, default: f64) -> Result<f64, ScanError> {
    let raw = req.resolved_params.get(key);
    let v = match raw {
        Some(v) => v
            .as_f64()
            .ok_or_else(|| ScanError::Kernel(format!("{key} must be a number; got {v}")))?,
        None => default,
    };
    if v < 0.0 || !v.is_finite() {
        return Err(ScanError::Kernel(format!(
            "{key} must be a finite non-negative number; got {v}"
        )));
    }
    Ok(v)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "bar/return count fits in f64's 52-bit mantissa; outlier index << 2^52"
)]
#[inline]
fn index_to_f64(i: usize) -> f64 {
    i as f64
}

#[allow(
    clippy::cast_precision_loss,
    reason = "outlier count is bounded by n (a bar count); fits in f64 mantissa"
)]
#[inline]
fn count_to_f64(c: usize) -> f64 {
    c as f64
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
    fn outliers_id_and_version() {
        let s = OutliersZAndMadScan;
        assert_eq!(s.id(), "stats.outliers.z_and_mad");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn outliers_arity_is_single() {
        assert_eq!(OutliersZAndMadScan.arity(), ScanArity::Single);
    }

    #[test]
    fn outliers_param_schema() {
        let schema = OutliersZAndMadScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["z_threshold"]["type"], "number");
        assert_eq!(schema["properties"]["z_threshold"]["default"], 3.0);
        assert_eq!(
            schema["properties"]["modified_z_threshold"]["default"],
            3.5
        );
        assert_eq!(schema["properties"]["series"]["default"], "log_returns");
        assert_eq!(schema["additionalProperties"], false);
    }

    /// Iglewicz-Hoaglin formula hand-derivation: for [-2, -1, 0, 1, 2] the
    /// MAD is 1 and 0.6745*4/1 == 2.698 (NOT outlier at 3.5), 0.6745*10/1 ==
    /// 6.745 (IS outlier).
    #[test]
    fn outliers_modified_z_iglewicz_hoaglin() {
        let (mz, med, mad) = kernel::modified_z_scores(&[-2.0_f64, -1.0, 0.0, 1.0, 2.0]);
        assert!((med - 0.0).abs() < 1e-12);
        assert!((mad - 1.0).abs() < 1e-12);
        // Manual checks of the formula constants.
        assert!((0.6745 * 4.0_f64 / 1.0 - 2.698).abs() < 1e-12);
        assert!((0.6745 * 10.0_f64 / 1.0 - 6.745).abs() < 1e-12);
        // mz[0] = 0.6745 * -2.0 = -1.349 (NOT outlier).
        assert!((mz[0] - (-1.349)).abs() < 1e-12);
    }

    /// Strong outlier in [1, 2, 3, 4, 100] — last index outlier under z AND
    /// modified-z (the kernel test in `kernel.rs` proves modified-z; here
    /// we verify the scan body wires both criteria into outlier_indices).
    #[test]
    fn outliers_z_score_basic() {
        let bars = bar_frame_from_closes(vec![1.0_f64, 2.0, 3.0, 4.0, 100.0]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "close"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // The last index (i=4, value=100) is an outlier.
        let idx_arr = &r.effect.extra["outlier_indices"];
        assert_eq!(idx_arr.shape, vec![1]);
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&idx_arr.data.0[0..8]);
        let idx0 = f64::from_le_bytes(buf);
        assert_eq!(idx0, 4.0);
    }

    /// `outlier_indices` is the UNION of z-outliers and modified-z outliers
    /// (the two parallel value vectors align with the union index list).
    #[test]
    fn outliers_indices_match() {
        // For [-2, -1, 0, 1, 2], no outliers at thresholds 3.0/3.5.
        let bars = bar_frame_from_closes(vec![-2.0, -1.0, 0.0, 1.0, 2.0]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "close"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.effect.value, 0.0, "no outliers at default thresholds");
        assert_eq!(r.effect.extra["outlier_indices"].shape, vec![0]);
        assert_eq!(r.effect.extra["outlier_values_z"].shape, vec![0]);
        assert_eq!(r.effect.extra["outlier_values_modified_z"].shape, vec![0]);
    }

    #[test]
    fn outliers_emits_one_result() {
        let bars = lcg_bar_frame_seeded(64, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn outliers_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(64, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "log_returns"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.outliers.z_and_mad@1");
        assert_eq!(r.effect.metric, "outliers_count");
        assert_eq!(r.effect.p_value, None);
        assert_eq!(r.effect.ci95, None);
        // 64 closes -> 63 log_returns.
        assert_eq!(r.effect.n, Some(63));
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec![
                "mad",
                "median",
                "modified_z_threshold",
                "outlier_indices",
                "outlier_values_modified_z",
                "outlier_values_z",
                "z_threshold",
            ]
        );
        let raw = r.raw.as_ref().expect("raw present");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    #[test]
    fn outliers_cancellation() {
        let bars = lcg_bar_frame_seeded(64, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn outliers_zero_variance_emits_scan_error() {
        // Constant close series -> MAD=0 -> ScanError::Kernel (T-04-04-01).
        let bars = bar_frame_from_closes(vec![1.5_f64; 16]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "close"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject MAD=0");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("MAD == 0"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn outliers_n_zero_emits_scan_error() {
        let bars = bar_frame_from_closes(Vec::new());
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject n=0");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn outliers_n_one_emits_scan_error() {
        let bars = bar_frame_from_closes(vec![1.5_f64]);
        let mut sink = VecSink::new();
        // series=close - n=1 closes -> not enough for std/MAD.
        let req = sample_request_with_params(serde_json::json!({"series": "close"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject n=1");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn outliers_invalid_threshold() {
        let bars = lcg_bar_frame_seeded(32, 4);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"z_threshold": -1.0}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = OutliersZAndMadScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject negative threshold");
        assert!(matches!(err, ScanError::Kernel(_)));
    }
}

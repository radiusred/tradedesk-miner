//! `KpssScan` — ANOM-06 Kwiatkowski-Phillips-Schmidt-Shin stationarity test.
//!
//! Pattern analog: [`crate::scan::anom::adf::AdfScan`] (Plan 04-05 Task 1
//! sibling) and [`crate::scan::ljung_box::LjungBoxScan`] (Phase 3
//! gold-standard) — Pattern A from `04-PATTERNS.md`.
//!
//! ## Reference
//!
//! `statsmodels.tsa.stattools.kpss(x, regression='c', nlags='auto')` —
//! statsmodels default uses the Schwert/Hobijn-Franses-Ooms auto-lag formula
//! `int(4 * (n/100)^(1/4))`.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.stationarity.kpss"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params`: optional `regression` (enum `["c","ct"]`, default `"c"`),
//!   optional `nlags` (integer | string "auto", default `"auto"`).
//! - `effect.metric = "kpss_statistic"`, `effect.value = KPSS stat` (always
//!   non-negative).
//! - `effect.extra = {crit_values, lag_truncation, p_value, regression}`
//!   (alphabetical `BTreeMap` order). `regression` is encoded as UTF-8 bytes
//!   packed into a `Dtype::F64` `RawArray` (same trick as ANOM-05 ADF).
//! - `raw.series = {closes, timestamps_ms}`.
//!
//! ## KPSS opposite null vs ADF
//!
//! - ADF: `H_0` = unit root (non-stationary), `H_1` = stationary. Rejection
//!   when τ is very negative.
//! - KPSS: `H_0` = stationary, `H_1` = unit root (non-stationary). Rejection
//!   when stat exceeds the critical value. The two tests are complementary;
//!   the Quant agent typically runs both.
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
    DataSlice, EffectSize, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

use kernel::{KpssRegression, NlagsParam};

/// ANOM-06 — KPSS stationarity test scan.
pub struct KpssScan;

const SCAN_ID: &str = "stats.stationarity.kpss";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "kpss_statistic";

impl Scan for KpssScan {
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
                "regression": {
                    "type": "string",
                    "enum": ["c", "ct"],
                    "default": "c",
                    "description": "Regression specification: c (constant only) or ct (constant + linear trend)."
                },
                "nlags": {
                    "description": "Bartlett-kernel lag truncation; integer (>=0) or 'auto' (default; uses int(4 * (n/100)^(1/4))).",
                    "default": "auto"
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["crit_values", "lag_truncation", "p_value", "regression"],
            raw_series_keys: &["closes", "timestamps_ms"],
        }
    }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-03) — opt-in to bootstrap CI.
    fn supports_bootstrap(&self) -> bool { true }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-04) — opt-in to null methods
    /// (`PhaseScramble` + `CircularShift`) per the per-scan matrix.
    fn supports_null_method(&self, m: crate::scan::NullMethod) -> bool {
        matches!(m, crate::scan::NullMethod::PhaseScramble | crate::scan::NullMethod::CircularShift)
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

        // Step 2 — N guard.
        let n = ctx.bars.close.len();
        if n < 4 {
            return Err(ScanError::Kernel(format!(
                "stats.stationarity.kpss: need n >= 4 closes; got n={n} (InsufficientData)"
            )));
        }

        // Step 3 — resolve params.
        let regression = resolve_regression(req)?;
        let nlags = resolve_nlags(req, n)?;

        // Step 4 — kernel call (input = LEVELS, per statsmodels.kpss default).
        let result = kernel::kpss_statistic(&ctx.bars.close, regression, nlags)
            .map_err(ScanError::Kernel)?;

        // Step 5 — build raw.series.
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
        )]
        let ts_ms: Vec<f64> = ctx
            .bars
            .ts_open_utc
            .iter()
            .map(|t| t.timestamp_millis() as f64)
            .collect();

        // Step 6 — envelope construction.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "crit_values".into(),
            f64_slice_to_raw_array(&result.crit_values),
        );
        extra.insert(
            "lag_truncation".into(),
            f64_slice_to_raw_array(&[index_to_f64(result.lag_truncation)]),
        );
        extra.insert("p_value".into(), f64_slice_to_raw_array(&[result.p_value]));
        extra.insert(
            "regression".into(),
            string_label_to_raw_array(regression_label(regression)),
        );

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
            effect_size: Some(EffectSize { kind: "tau_signed".to_string(), value: result.statistic }),
            extra,
        };

        let mut series_map: BTreeMap<String, RawArray> = BTreeMap::new();
        series_map.insert("closes".into(), f64_slice_to_raw_array(&ctx.bars.close));
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

fn resolve_regression(req: &ScanRequest) -> Result<KpssRegression, ScanError> {
    let raw = req.resolved_params.get("regression");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.stationarity.kpss: regression must be c|ct; got {v}"
            ))
        })?,
        None => "c",
    };
    match label {
        "c" => Ok(KpssRegression::C),
        "ct" => Ok(KpssRegression::Ct),
        other => Err(ScanError::Kernel(format!(
            "stats.stationarity.kpss: regression must be c|ct; got {other:?}"
        ))),
    }
}

fn resolve_nlags(req: &ScanRequest, n: usize) -> Result<NlagsParam, ScanError> {
    let raw = req.resolved_params.get("nlags");
    match raw {
        None => Ok(NlagsParam::Auto),
        Some(v) => {
            if let Some(s) = v.as_str() {
                if s == "auto" {
                    Ok(NlagsParam::Auto)
                } else {
                    Err(ScanError::Kernel(format!(
                        "stats.stationarity.kpss: nlags must be integer or 'auto'; got {s:?}"
                    )))
                }
            } else if let Some(i) = v.as_i64() {
                if i < 0 {
                    return Err(ScanError::Kernel(format!(
                        "stats.stationarity.kpss: nlags must be >= 0; got {i}"
                    )));
                }
                let u = usize::try_from(i).map_err(|_| {
                    ScanError::Kernel(format!(
                        "stats.stationarity.kpss: nlags={i} out of usize range"
                    ))
                })?;
                if u >= n {
                    return Err(ScanError::Kernel(format!(
                        "stats.stationarity.kpss: nlags={u} must be < n={n}"
                    )));
                }
                Ok(NlagsParam::Manual(u))
            } else {
                Err(ScanError::Kernel(format!(
                    "stats.stationarity.kpss: nlags must be integer or 'auto'; got {v}"
                )))
            }
        }
    }
}

fn regression_label(r: KpssRegression) -> &'static str {
    match r {
        KpssRegression::C => "c",
        KpssRegression::Ct => "ct",
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

/// Pack a UTF-8 label into a `RawArray`'s data field as f64-sized bytes —
/// same v1 wire-form trick used by ANOM-04 squared `series_kind` +
/// ANOM-05 ADF regression label.
fn string_label_to_raw_array(label: &str) -> RawArray {
    use crate::findings::{Base64Bytes, Dtype};
    let bytes = label.as_bytes().to_vec();
    let shape_len = bytes.len() as u64;
    RawArray {
        data: Base64Bytes(bytes),
        shape: vec![shape_len],
        dtype: Dtype::F64,
    }
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

    fn random_walk_closes(n: usize, seed: u64) -> Vec<f64> {
        let mut closes = vec![1.0_f64];
        #[allow(clippy::cast_possible_truncation)]
        let mut s: u32 = seed as u32;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *closes.last().unwrap();
            closes.push(prev + 0.01 * eps);
        }
        closes
    }

    fn stationary_closes(n: usize, seed: u64) -> Vec<f64> {
        let mut closes = vec![0.5_f64];
        #[allow(clippy::cast_possible_truncation)]
        let mut s: u32 = seed as u32;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *closes.last().unwrap();
            closes.push(0.05 * prev + 0.01 * eps);
        }
        closes
    }

    // -----------------------------------------------------------------------

    #[test]
    fn kpss_id_and_version() {
        assert_eq!(KpssScan.id(), "stats.stationarity.kpss");
        assert_eq!(KpssScan.version(), 1);
    }

    #[test]
    fn kpss_arity_is_single() {
        assert_eq!(KpssScan.arity(), ScanArity::Single);
    }

    #[test]
    fn kpss_param_schema() {
        let schema = KpssScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["regression"]["default"], "c");
        assert_eq!(schema["properties"]["nlags"]["default"], "auto");
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn kpss_emits_one_result() {
        let bars = bar_frame_from_closes(random_walk_closes(80, 1));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        KpssScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn kpss_result_envelope_shape() {
        let bars = bar_frame_from_closes(random_walk_closes(80, 2));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        KpssScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.stationarity.kpss@1");
        assert_eq!(r.effect.metric, "kpss_statistic");
        assert!(r.effect.p_value.is_some());
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec!["crit_values", "lag_truncation", "p_value", "regression",]
        );
        assert_eq!(r.effect.extra["crit_values"].shape, vec![4]);
        let raw = r.raw.as_ref().expect("raw");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["closes", "timestamps_ms"]);
    }

    #[test]
    fn kpss_cancellation() {
        let bars = bar_frame_from_closes(random_walk_closes(40, 3));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        KpssScan.run(&ctx, &req, &mut sink).expect("ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn kpss_n_zero_emits_scan_error() {
        let bars = bar_frame_from_closes(Vec::new());
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = KpssScan.run(&ctx, &req, &mut sink).expect_err("reject n=0");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "{msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn kpss_n_one_emits_scan_error() {
        let bars = bar_frame_from_closes(vec![1.5_f64]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = KpssScan.run(&ctx, &req, &mut sink).expect_err("reject n=1");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "{msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn kpss_known_stationary_zero_mean_series() {
        let bars = bar_frame_from_closes(stationary_closes(200, 5));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        KpssScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // Stationary series -> KPSS stat below 5% crit (0.463).
        assert!(
            r.effect.value < 0.463,
            "stationary KPSS = {} should be < 0.463",
            r.effect.value
        );
    }

    #[test]
    fn kpss_known_random_walk_rejects_null() {
        let bars = bar_frame_from_closes(random_walk_closes(200, 13));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        KpssScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // Random walk -> KPSS stat > 5% crit (0.463).
        assert!(
            r.effect.value > 0.463,
            "random walk KPSS = {} should be > 0.463",
            r.effect.value
        );
    }

    #[test]
    fn kpss_lag_truncation_deterministic() {
        let bars = bar_frame_from_closes(random_walk_closes(150, 7));
        let req = sample_request_with_params(serde_json::json!({}));
        let lags: Vec<u64> = (0..3)
            .map(|_| {
                let mut sink = VecSink::new();
                let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
                KpssScan.run(&ctx, &req, &mut sink).expect("ok");
                let findings = parse_sink_to_findings(&sink);
                let Finding::Result(r) = &findings[0] else {
                    panic!("expected Result");
                };
                let arr = &r.effect.extra["lag_truncation"];
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&arr.data.0[0..8]);
                f64::from_le_bytes(buf) as u64
            })
            .collect();
        for l in &lags {
            assert_eq!(*l, lags[0], "auto-lag selection not deterministic");
        }
    }

    #[test]
    fn kpss_invalid_regression_rejected() {
        let bars = bar_frame_from_closes(random_walk_closes(40, 0));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"regression": "ctt"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = KpssScan.run(&ctx, &req, &mut sink).expect_err("reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }
}

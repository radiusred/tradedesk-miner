//! `AdfScan` — ANOM-05 Augmented Dickey-Fuller stationarity test with AIC lag
//! selection.
//!
//! Pattern analog: [`crate::scan::ljung_box::LjungBoxScan`] and
//! [`crate::scan::anom::outliers::OutliersZAndMadScan`] — Pattern A from
//! `04-PATTERNS.md`.
//!
//! ## Reference
//!
//! `statsmodels.tsa.stattools.adfuller(x, maxlag=None, regression='c',
//! autolag='AIC')`. The kernel is hand-derived (no Rust crate ships ADF) —
//! see `04-RESEARCH.md` §1.4 + the kernel module-doc for the algorithm and
//! the `MacKinnon` p-value simplification.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.stationarity.adf"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params`: optional `max_lag` (integer, default `int(12 * (n/100)^0.25)`
//!   per statsmodels), optional `regression` (enum `["nc","c","ct","ctt"]`,
//!   default `"c"`), optional `autolag` (enum `["AIC","BIC","None"]`, default
//!   `"AIC"`).
//! - `effect.metric = "adf_statistic"`, `effect.value = τ` (the test
//!   statistic — negative for stationary series), `effect.p_value =
//!   MacKinnon-approximated p-value`.
//! - `effect.extra = {crit_values, lag_selected, nobs, p_value, regression}`
//!   (alphabetical `BTreeMap` order). The `regression` key encodes the variant
//!   name as a UTF-8 bytes `RawArray` (same `Dtype::F64` wire-form trick used by
//!   ANOM-04 squared variant).
//! - `raw.series = {closes, timestamps_ms}`.
//!
//! ## ADF input is LEVELS, not returns
//!
//! Unit-root tests run on the LEVEL series (closes). The kernel does NOT call
//! `log_returns` — ADF tests whether the level `y_t` has a unit root (i.e.,
//! is a random walk).
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
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

use kernel::{AutoLagVariant, RegressionVariant};

/// ANOM-05 — Augmented Dickey-Fuller stationarity test scan.
pub struct AdfScan;

const SCAN_ID: &str = "stats.stationarity.adf";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "adf_statistic";

impl Scan for AdfScan {
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
                "max_lag": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Upper bound on the AIC lag search (or fixed lag when autolag=None). Default: int(12 * (n/100)^0.25) per statsmodels."
                },
                "regression": {
                    "type": "string",
                    "enum": ["nc", "c", "ct", "ctt"],
                    "default": "c",
                    "description": "Regression specification: nc (no constant), c (constant only), ct (constant + trend), ctt (constant + trend + trend^2)."
                },
                "autolag": {
                    "type": "string",
                    "enum": ["AIC", "BIC", "None"],
                    "default": "AIC",
                    "description": "Lag-selection method (AIC default, BIC alternative, or None to use max_lag as fixed)."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[
                "crit_values",
                "lag_selected",
                "nobs",
                "p_value",
                "regression",
            ],
            raw_series_keys: &["closes", "timestamps_ms"],
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Scan::run is the linear dispatch + envelope build path; splitting into helpers obscures the 7-step Pattern A structure"
    )]
    /// Phase 5 (Plan 05-03 / D5-04 / HYG-03) — opt-in to bootstrap CI.
    fn supports_bootstrap(&self) -> bool { true }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-04) — opt-in to null methods
    /// (PhaseScramble + CircularShift) per the per-scan matrix.
    fn supports_null_method(&self, m: crate::scan::NullMethod) -> bool {
        matches!(m, crate::scan::NullMethod::PhaseScramble | crate::scan::NullMethod::CircularShift)
    }

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

        // Step 2 — N guard. ADF needs at least a handful of observations.
        let n = ctx.bars.close.len();
        if n < 4 {
            return Err(ScanError::Kernel(format!(
                "stats.stationarity.adf: need n >= 4 closes; got n={n} (InsufficientData)"
            )));
        }

        // Step 3 — resolve params.
        let regression = resolve_regression(req)?;
        let autolag = resolve_autolag(req)?;
        let max_lag = resolve_max_lag(req, n)?;

        // Step 4 — kernel call (input = LEVELS, not returns).
        let result = kernel::adfuller(&ctx.bars.close, max_lag, regression, autolag)
            .map_err(ScanError::Kernel)?;

        // Step 5 — build raw.series (closes + timestamps).
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
            "lag_selected".into(),
            f64_slice_to_raw_array(&[index_to_f64(result.lag_selected)]),
        );
        extra.insert(
            "nobs".into(),
            f64_slice_to_raw_array(&[index_to_f64(result.nobs)]),
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
                reason = "nobs <= u64 on all supported targets"
            )]
            n: Some(result.nobs as u64),
            ci95: None,
            effect_size: None,
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

fn resolve_regression(req: &ScanRequest) -> Result<RegressionVariant, ScanError> {
    let raw = req.resolved_params.get("regression");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.stationarity.adf: regression must be nc|c|ct|ctt; got {v}"
            ))
        })?,
        None => "c",
    };
    match label {
        "nc" => Ok(RegressionVariant::Nc),
        "c" => Ok(RegressionVariant::C),
        "ct" => Ok(RegressionVariant::Ct),
        "ctt" => Ok(RegressionVariant::Ctt),
        other => Err(ScanError::Kernel(format!(
            "stats.stationarity.adf: regression must be nc|c|ct|ctt; got {other:?}"
        ))),
    }
}

fn resolve_autolag(req: &ScanRequest) -> Result<AutoLagVariant, ScanError> {
    let raw = req.resolved_params.get("autolag");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.stationarity.adf: autolag must be AIC|BIC|None; got {v}"
            ))
        })?,
        None => "AIC",
    };
    match label {
        "AIC" => Ok(AutoLagVariant::Aic),
        "BIC" => Ok(AutoLagVariant::Bic),
        "None" => Ok(AutoLagVariant::None),
        other => Err(ScanError::Kernel(format!(
            "stats.stationarity.adf: autolag must be AIC|BIC|None; got {other:?}"
        ))),
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "max_lag follows statsmodels' int(12 * (n/100)^0.25); n << 2^52, result << 100"
)]
fn resolve_max_lag(req: &ScanRequest, n: usize) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("max_lag");
    let max_lag = if let Some(v) = raw {
        let i = v.as_i64().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.stationarity.adf: max_lag must be an integer; got {v}"
            ))
        })?;
        if i < 0 {
            return Err(ScanError::Kernel(format!(
                "stats.stationarity.adf: max_lag must be >= 0; got {i}"
            )));
        }
        usize::try_from(i).map_err(|_| {
            ScanError::Kernel(format!(
                "stats.stationarity.adf: max_lag={i} out of usize range"
            ))
        })?
    } else {
        // statsmodels default: int(12 * (n/100)^0.25).
        let nf = n as f64;
        let v = 12.0 * (nf / 100.0).powf(0.25);
        v.floor().max(0.0) as usize
    };
    if max_lag >= n {
        return Err(ScanError::Kernel(format!(
            "stats.stationarity.adf: max_lag={max_lag} must be < n={n}"
        )));
    }
    Ok(max_lag)
}

fn regression_label(r: RegressionVariant) -> &'static str {
    match r {
        RegressionVariant::Nc => "nc",
        RegressionVariant::C => "c",
        RegressionVariant::Ct => "ct",
        RegressionVariant::Ctt => "ctt",
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

/// Pack a UTF-8 label into a `RawArray`'s data field as f64-sized bytes — same
/// trick used by ANOM-04 squared `series_kind`. The catalogue's v1 wire-form
/// supports only `Dtype::F64`; consumers decode via
/// `std::str::from_utf8(&extra["regression"].data.0)`.
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
    fn adf_id_and_version() {
        assert_eq!(AdfScan.id(), "stats.stationarity.adf");
        assert_eq!(AdfScan.version(), 1);
    }

    #[test]
    fn adf_arity_is_single() {
        assert_eq!(AdfScan.arity(), ScanArity::Single);
    }

    #[test]
    fn adf_param_schema() {
        let schema = AdfScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["regression"]["default"], "c");
        assert_eq!(schema["properties"]["autolag"]["default"], "AIC");
        assert_eq!(schema["additionalProperties"], false);
    }

    /// Pitfall 4 pin — AIC lag selection must be deterministic across repeated
    /// invocations on the same input (sequential summation, NOT `par_iter`).
    #[test]
    fn adf_aic_lag_selection_deterministic_seq_summation() {
        let bars = lcg_bar_frame_seeded(120, 42);
        let req = sample_request_with_params(serde_json::json!({"max_lag": 5}));

        let lags: Vec<u64> = (0..5)
            .map(|_| {
                let mut sink = VecSink::new();
                let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
                AdfScan.run(&ctx, &req, &mut sink).expect("ok");
                let findings = parse_sink_to_findings(&sink);
                let Finding::Result(r) = &findings[0] else {
                    panic!("expected Result");
                };
                let arr = &r.effect.extra["lag_selected"];
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&arr.data.0[0..8]);
                f64::from_le_bytes(buf) as u64
            })
            .collect();
        // All five runs select the same lag.
        for k in &lags {
            assert_eq!(*k, lags[0], "AIC lag selection not deterministic");
        }
    }

    #[test]
    fn adf_invalid_regression_rejected() {
        let bars = lcg_bar_frame_seeded(40, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"regression": "garbage"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = AdfScan.run(&ctx, &req, &mut sink).expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn adf_emits_one_result() {
        let bars = lcg_bar_frame_seeded(80, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        AdfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn adf_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(80, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"max_lag": 3}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        AdfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.stationarity.adf@1");
        assert_eq!(r.effect.metric, "adf_statistic");
        assert!(r.effect.p_value.is_some());
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec![
                "crit_values",
                "lag_selected",
                "nobs",
                "p_value",
                "regression",
            ]
        );
        // crit_values has length 3.
        assert_eq!(r.effect.extra["crit_values"].shape, vec![3]);
        let raw = r.raw.as_ref().expect("raw");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["closes", "timestamps_ms"]);
    }

    #[test]
    fn adf_cancellation() {
        let bars = lcg_bar_frame_seeded(80, 4);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        AdfScan.run(&ctx, &req, &mut sink).expect("ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn adf_n_zero_emits_scan_error() {
        let bars = bar_frame_from_closes(Vec::new());
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = AdfScan.run(&ctx, &req, &mut sink).expect_err("reject n=0");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "{msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn adf_n_one_emits_scan_error() {
        let bars = bar_frame_from_closes(vec![1.5_f64]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = AdfScan.run(&ctx, &req, &mut sink).expect_err("reject n=1");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "{msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    /// Sanity check: a strongly mean-reverting series should yield a strongly
    /// negative ADF statistic (rejects the unit-root null).
    #[test]
    fn adf_known_stationary_series_rejects_null() {
        // y_t = 0.05 * y_{t-1} + tiny noise — very mean-reverting.
        let n = 200;
        let mut closes = vec![0.5_f64];
        let mut s: u32 = 7;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *closes.last().unwrap();
            closes.push(0.05 * prev + 0.001 * eps);
        }
        let bars = bar_frame_from_closes(closes);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"max_lag": 4}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        AdfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!(
            r.effect.value < -2.5,
            "stationary series τ = {} should be < -2.5",
            r.effect.value
        );
    }

    /// Sanity check: a pure cumulative-sum random walk should yield an ADF
    /// statistic near 0 (FAILS to reject unit-root null).
    #[test]
    fn adf_known_random_walk_fails_to_reject() {
        // Cumulative-sum random walk: y_t = y_{t-1} + ε.
        let n = 200;
        let mut closes = vec![0.5_f64];
        let mut s: u32 = 31;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *closes.last().unwrap();
            closes.push(prev + 0.01 * eps);
        }
        let bars = bar_frame_from_closes(closes);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"max_lag": 4}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        AdfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // The 5% critical for regression='c' is -2.861. A random walk should
        // be ABOVE that (i.e., we do NOT reject the unit-root null).
        assert!(
            r.effect.value > -2.861,
            "random walk τ = {} should be > -2.861 (5% crit)",
            r.effect.value
        );
    }
}

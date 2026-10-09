//! `OuHalfLifeScan` — `stats.meanrev.ou_halflife@1` single-leg OU half-life
//! scan (RAD-3627 / Tier-1 build for RAD-3545).
//!
//! Pattern analog: [`crate::scan::anom::adf::AdfScan`] (sibling single-leg
//! stationarity-family scan) — Pattern A from `04-PATTERNS.md`.
//!
//! ## Purpose
//!
//! ADF/KPSS (`stats.stationarity.*`) tell us mean reversion *exists* for a
//! single series; they do not say *how fast* it decays. This scan closes that
//! gap: it fits an AR(1) on a single-leg series and reports the
//! Ornstein-Uhlenbeck mean-reversion rate `λ`, the half-life `ln2/λ`, the AR(1)
//! coefficient `φ`, and its Dickey-Fuller t-stat — the hold-period-floor /
//! sizing input (alpha filter pt 7). It is the standalone single-leg analogue
//! of the half-life already computed on the Engle-Granger cointegration
//! residual (CROSS-05); both share the
//! [`crate::scan::primitives::ar1::ou_ar1_fit`] primitive.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.meanrev.ou_halflife"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params`: `on` (enum `["level","returns"]`, default `"level"`) selects
//!   the basis the AR(1) is fitted on; `min_n` (integer >= 3, default 30) the
//!   minimum close count.
//! - `effect.metric = "ou_lambda"`, `effect.value = λ` (the OU mean-reversion
//!   rate `-ln φ`; **always finite** — `0.0` when the series is not
//!   mean-reverting). `effect.p_value = None`.
//! - `effect.effect_size = { kind: "ou_lambda", value: λ }`.
//! - `effect.extra = {ar1_coeff, ar1_t_stat, half_life, nobs, on}`
//!   (alphabetical `BTreeMap`). `half_life = ln2/λ` is carried here as
//!   `Dtype::F64` bytes (NOT `effect.value`) because it is `f64::INFINITY`
//!   for a non-mean-reverting series and the schema's `effect.value` must be a
//!   finite JSON number — same convention as CROSS-05 `ou_half_life`. `on` is
//!   the UTF-8 basis label packed into a `RawArray` (the ANOM-05 `regression`
//!   trick).
//! - `raw.series = {closes, timestamps_ms}`.
//!
//! ## Basis
//!
//! `on = "level"` (default) fits the AR(1) on the raw close LEVEL series;
//! `on = "returns"` fits on `log_returns(closes)` (length `n - 1`).
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
    DataSlice, Effect, EffectSize, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

use kernel::SeriesBasis;

/// ANOM single-leg Ornstein-Uhlenbeck half-life scan.
pub struct OuHalfLifeScan;

const SCAN_ID: &str = "stats.meanrev.ou_halflife";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "ou_lambda";
const EFFECT_SIZE_KIND: &str = "ou_lambda";

/// Default minimum close count. Mirrors the CROSS-05 Engle-Granger
/// `MIN_ALIGNED_N = 30` floor — below this the AR(1) fit is underpowered.
const DEFAULT_MIN_N: usize = 30;

/// Hard lower bound on `min_n` — the `Δy ~ y_{t-1}` regression needs at least
/// 3 points (and the t-stat needs `m = n - 1 >= 3`).
const MIN_N_FLOOR: usize = 3;

impl Scan for OuHalfLifeScan {
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
                "on": {
                    "type": "string",
                    "enum": ["level", "returns"],
                    "default": "level",
                    "description": "Basis the AR(1) is fitted on: 'level' (raw close series, default) or 'returns' (log returns of closes)."
                },
                "min_n": {
                    "type": "integer",
                    "minimum": 3,
                    "default": 30,
                    "description": "Minimum number of closes required to fit the AR(1). Default 30 (mirrors the CROSS-05 Engle-Granger floor)."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["ar1_coeff", "ar1_t_stat", "half_life", "nobs", "on"],
            raw_series_keys: &["closes", "timestamps_ms"],
        }
    }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-03) — opt-in to bootstrap CI, matching
    /// the sibling single-leg stationarity scans.
    fn supports_bootstrap(&self) -> bool {
        true
    }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-04) — opt-in to null methods
    /// (`PhaseScramble` + `CircularShift`), matching the ADF/KPSS family.
    fn supports_null_method(&self, m: crate::scan::NullMethod) -> bool {
        matches!(
            m,
            crate::scan::NullMethod::PhaseScramble | crate::scan::NullMethod::CircularShift
        )
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
        let on = resolve_on(req)?;
        let min_n = resolve_min_n(req)?;

        // Step 3 — N guard.
        let n = ctx.bars.close.len();
        if n < min_n {
            return Err(ScanError::Kernel(format!(
                "stats.meanrev.ou_halflife: need n >= {min_n} closes; got n={n} (InsufficientData)"
            )));
        }

        // Step 4 — kernel call (level or log-returns basis).
        let fit = kernel::ou_halflife(&ctx.bars.close, on);

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
        extra.insert("ar1_coeff".into(), f64_slice_to_raw_array(&[fit.ar1_coeff]));
        extra.insert("ar1_t_stat".into(), f64_slice_to_raw_array(&[fit.t_stat]));
        // half_life is INFINITY for a non-mean-reverting series; carried as
        // LE-f64 bytes (infinity-safe) NOT as effect.value (which the schema
        // requires to be a finite JSON number).
        extra.insert("half_life".into(), f64_slice_to_raw_array(&[fit.half_life]));
        extra.insert(
            "nobs".into(),
            f64_slice_to_raw_array(&[index_to_f64(fit.nobs)]),
        );
        extra.insert("on".into(), string_label_to_raw_array(on.label()));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            // λ — the OU mean-reversion rate, always finite (0.0 when the
            // series is not mean-reverting). half_life = ln2/λ lives in extra.
            value: fit.lambda,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "nobs <= u64 on all supported targets"
            )]
            n: Some(fit.nobs as u64),
            ci95: None,
            effect_size: Some(EffectSize {
                kind: EFFECT_SIZE_KIND.to_string(),
                value: fit.lambda,
            }),
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

fn resolve_on(req: &ScanRequest) -> Result<SeriesBasis, ScanError> {
    let raw = req.resolved_params.get("on");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.meanrev.ou_halflife: on must be level|returns; got {v}"
            ))
        })?,
        None => "level",
    };
    match label {
        "level" => Ok(SeriesBasis::Level),
        "returns" => Ok(SeriesBasis::Returns),
        other => Err(ScanError::Kernel(format!(
            "stats.meanrev.ou_halflife: on must be level|returns; got {other:?}"
        ))),
    }
}

fn resolve_min_n(req: &ScanRequest) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("min_n");
    let min_n = if let Some(v) = raw {
        let i = v.as_i64().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.meanrev.ou_halflife: min_n must be an integer; got {v}"
            ))
        })?;
        if i < 0 {
            return Err(ScanError::Kernel(format!(
                "stats.meanrev.ou_halflife: min_n must be >= {MIN_N_FLOOR}; got {i}"
            )));
        }
        usize::try_from(i).map_err(|_| {
            ScanError::Kernel(format!(
                "stats.meanrev.ou_halflife: min_n={i} out of usize range"
            ))
        })?
    } else {
        DEFAULT_MIN_N
    };
    if min_n < MIN_N_FLOOR {
        return Err(ScanError::Kernel(format!(
            "stats.meanrev.ou_halflife: min_n must be >= {MIN_N_FLOOR}; got {min_n}"
        )));
    }
    Ok(min_n)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "index << 2^52 for any realistic bar count"
)]
#[inline]
fn index_to_f64(i: usize) -> f64 {
    i as f64
}

/// Pack a UTF-8 label into a `RawArray`'s data field as bytes — same trick
/// used by ANOM-05 `regression`. The v1 wire-form supports only `Dtype::F64`;
/// consumers decode via `std::str::from_utf8(&extra["on"].data.0)`.
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

    /// Exact AR(1) decay-to-mean series with coefficient φ (recovered exactly
    /// by OLS since the points are perfectly collinear in `(y_lag, Δy)`).
    fn ar1_decay_closes(phi: f64, n: usize) -> Vec<f64> {
        let mu = 1.5_f64;
        let y0 = 2.0_f64;
        (0..n)
            .map(|t| {
                let tt = i32::try_from(t).expect("fits");
                mu + phi.powi(tt) * (y0 - mu)
            })
            .collect()
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

    fn decode_f64(arr: &RawArray) -> f64 {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&arr.data.0[0..8]);
        f64::from_le_bytes(buf)
    }

    #[test]
    fn ou_id_and_version() {
        assert_eq!(OuHalfLifeScan.id(), "stats.meanrev.ou_halflife");
        assert_eq!(OuHalfLifeScan.version(), 1);
    }

    #[test]
    fn ou_arity_is_single() {
        assert_eq!(OuHalfLifeScan.arity(), ScanArity::Single);
    }

    #[test]
    fn ou_param_schema() {
        let schema = OuHalfLifeScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["on"]["default"], "level");
        assert_eq!(schema["properties"]["min_n"]["default"], 30);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn ou_emits_one_result_with_expected_shape() {
        let bars = bar_frame_from_closes(ar1_decay_closes(0.5, 40));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OuHalfLifeScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.meanrev.ou_halflife@1");
        assert_eq!(r.effect.metric, "ou_lambda");
        assert!(r.effect.p_value.is_none());
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec!["ar1_coeff", "ar1_t_stat", "half_life", "nobs", "on"]
        );
        let raw = r.raw.as_ref().expect("raw");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["closes", "timestamps_ms"]);
    }

    /// Acceptance #2 (part 1): φ = 0.5 ⇒ half-life ≈ 1.0 and λ ≈ ln2.
    #[test]
    fn ou_known_phi_recovers_half_life() {
        let bars = bar_frame_from_closes(ar1_decay_closes(0.5, 40));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OuHalfLifeScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let half_life = decode_f64(&r.effect.extra["half_life"]);
        assert!(
            (half_life - 1.0).abs() < 1e-6,
            "half_life = {half_life} expected ~1.0"
        );
        let ar1 = decode_f64(&r.effect.extra["ar1_coeff"]);
        assert!((ar1 - 0.5).abs() < 1e-9, "φ = {ar1} expected 0.5");
        // λ = ln2 / half_life ≈ ln2.
        assert!(
            (r.effect.value - std::f64::consts::LN_2).abs() < 1e-6,
            "λ = {} expected ln2",
            r.effect.value
        );
    }

    /// Acceptance #2 (part 2): an explosive (φ > 1) series is not
    /// mean-reverting ⇒ half-life = INFINITY sentinel, λ = 0.
    #[test]
    fn ou_non_mean_reverting_yields_infinity() {
        let bars = bar_frame_from_closes(ar1_decay_closes(1.5, 30));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OuHalfLifeScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let half_life = decode_f64(&r.effect.extra["half_life"]);
        assert!(
            half_life.is_infinite(),
            "half_life = {half_life} expected INF"
        );
        assert_eq!(r.effect.value, 0.0, "λ sentinel = 0");
    }

    #[test]
    fn ou_returns_basis_label_echoed() {
        let bars = bar_frame_from_closes(ar1_decay_closes(0.5, 40));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"on": "returns"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OuHalfLifeScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let on_bytes = &r.effect.extra["on"].data.0;
        assert_eq!(std::str::from_utf8(on_bytes).unwrap(), "returns");
    }

    #[test]
    fn ou_invalid_on_rejected() {
        let bars = bar_frame_from_closes(ar1_decay_closes(0.5, 40));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"on": "garbage"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = OuHalfLifeScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn ou_below_min_n_rejected() {
        let bars = bar_frame_from_closes(ar1_decay_closes(0.5, 10));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = OuHalfLifeScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject n < 30");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "{msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn ou_custom_min_n_allows_shorter_series() {
        let bars = bar_frame_from_closes(ar1_decay_closes(0.5, 12));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"min_n": 10}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OuHalfLifeScan
            .run(&ctx, &req, &mut sink)
            .expect("ok with min_n=10");
        assert!(!sink.0.is_empty());
    }

    #[test]
    fn ou_cancellation() {
        let bars = bar_frame_from_closes(ar1_decay_closes(0.5, 40));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        OuHalfLifeScan.run(&ctx, &req, &mut sink).expect("ok");
        assert!(sink.0.is_empty());
    }
}

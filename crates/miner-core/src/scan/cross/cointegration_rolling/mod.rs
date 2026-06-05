//! Rolling Engle-Granger cointegration scan (CROSS-06, RAD-3626).
//!
//! Pair-arity scan that slides a fixed-size window over two aligned price-LEVEL
//! series and, per window step, emits the rolling hedge ratio (β), residual
//! ADF stat + p-value, residual OU half-life, and a **breakdown flag** that
//! trips when the cointegration relationship degrades. It is the rolling
//! complement to the single-shot `cross.cointegration.engle_granger@1`
//! (CROSS-05): whole-sample Engle-Granger hides regime breaks, the #1 way
//! pairs blow up in production.
//!
//! The per-window math is NOT re-derived — every window delegates to the
//! factored per-fit kernel
//! [`crate::scan::cross::engle_granger::kernel::engle_granger`]. This module
//! owns the surface, parameter resolution, window→timestamp mapping, and
//! envelope assembly; [`kernel`] owns the iteration + breakdown detector.
//!
//! ## Emission shape
//!
//! Following the CROSS rolling-sibling convention (`cross.ols.rolling`,
//! `cross.corr.*_rolling`), the scan emits ONE `Finding::Result` carrying ONE
//! ENTRY PER WINDOW STEP in its `effect.extra` arrays (NOT one envelope per
//! window — the whole codebase emits a single Result per `Scan::run`). The
//! per-window vectors are index-aligned:
//!
//! - `betas`, `alphas`, `adf_stats`, `adf_p_values`, `ou_half_lives`,
//!   `residual_stds` — the per-window Engle-Granger stats (`ou_half_lives`
//!   uses the `f64::INFINITY` sentinel for non-mean-reverting windows,
//!   matching `engle_granger`).
//! - `breakdown_flags` — `1.0` when the window tripped breakdown, else `0.0`.
//! - `breakdown_reason_codes` — `0=none, 1=adf_lost_stationarity,
//!   2=beta_drift, 3=both` (see [`kernel::BreakdownReason`]).
//! - `window_starts_ms` / `window_ends_ms` — the aligned-series open timestamp
//!   of the window's first / last bar.
//! - `window_length` / `step` — scalar (length-1) echoes of the params.
//!
//! `effect.value` = the breakdown **count** (number of tripped windows) — the
//! headline a consumer scans for ("did this pair break down, and how often");
//! `effect.effect_size` mirrors it as `kind = "breakdown_window_count"`.
//! `effect.p_value` is `None`: there is no single hypothesis test here — the
//! per-window ADF p-values live in the `adf_p_values` array.
//!
//! `raw.series` carries the LEVELS (`close_a`, `close_b`) plus the canonical
//! D-03 `timestamps_ms` key for the full joint aligned series — Engle-Granger
//! operates on levels, not returns.
//!
//! ## Coalesce contract (RAD-2397)
//!
//! This scan does NOT override [`Scan::coalesce_subranges`] (stays `false`,
//! like the other rolling CROSS scans): a rolling window must not span a gap,
//! so the engine dispatches per contiguous gap-removed sub-range. Aligned-N is
//! therefore evaluated on the post-join, gap-removed series within each
//! sub-range, and the per-window floor (`min_aligned_n`, default
//! [`engle_granger::MIN_ALIGNED_N`] = 30) is enforced on that series.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, EffectSize, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::cross::engle_granger;
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::time_alignment::inner_join;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

pub use kernel::{BreakdownReason, BreakdownThresholds};

/// Pair-arity rolling Engle-Granger cointegration scan (CROSS-06).
pub struct CointegrationRollingScan;

const SCAN_ID: &str = "cross.cointegration.rolling";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "cointegration_rolling_breakdown_count";

/// Default `step` (slide one bar at a time).
const DEFAULT_STEP: i64 = 1;

const FINDING_SHAPE: ScanFindingShape = ScanFindingShape {
    effect_extra_keys: &[
        "adf_p_values",
        "adf_stats",
        "alphas",
        "betas",
        "breakdown_flags",
        "breakdown_reason_codes",
        "ou_half_lives",
        "residual_stds",
        "step",
        "window_ends_ms",
        "window_length",
        "window_starts_ms",
    ],
    // D-03 invariant: Raw::new requires a `timestamps_ms` key. Engle-Granger
    // operates on LEVELS, so the raw block carries the leg closes under the
    // joint aligned timestamps (mirrors cross.cointegration.engle_granger).
    raw_series_keys: &["close_a", "close_b", "timestamps_ms"],
};

fn cointegration_rolling_param_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "window": {
                "type": "integer",
                "minimum": 2,
                "description": "Rolling-window size in aligned bars (required). Must be >= min_aligned_n (the per-window Engle-Granger floor) and <= the post-join aligned-bar count."
            },
            "step": {
                "type": "integer",
                "minimum": 1,
                "default": 1,
                "description": "Bars to advance the window start between fits (default 1)."
            },
            "min_aligned_n": {
                "type": "integer",
                "minimum": 2,
                "default": 30,
                "description": "Minimum aligned bars required per window for a meaningful fit. Defaults to the Engle-Granger MIN_ALIGNED_N=30 floor; `window` must be >= this."
            },
            "adf_p_enter": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "default": 0.05,
                "description": "Residual ADF p-value at or below which a window is treated as stationary (cointegrated). The 'established' half of the breakdown hysteresis."
            },
            "adf_p_exit": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "default": 0.10,
                "description": "Residual ADF p-value above which an already-established pair counts as having lost stationarity. Must be >= adf_p_enter."
            },
            "beta_band": {
                "type": "number",
                "exclusiveMinimum": 0.0,
                "default": 0.25,
                "description": "Fractional band around the trailing median of |beta|. 0.25 ⇒ a window trips beta-drift when |beta| leaves ±25% of the trailing median."
            },
            "regression": {
                "type": "string",
                "enum": ["c", "ct"],
                "default": "c",
                "description": "Trend spec for the embedded residual ADF test, matching cross.cointegration.engle_granger. v1 ships 'c'; 'ct' is accepted but downgraded to 'c' in the local kernel."
            }
        },
        "required": ["window"],
        "additionalProperties": false
    })
}

/// Resolved + validated parameters.
struct ResolvedParams {
    window: usize,
    step: usize,
    thresholds: BreakdownThresholds,
    regression: engle_granger::AdfRegression,
}

impl Scan for CointegrationRollingScan {
    fn id(&self) -> &'static str {
        SCAN_ID
    }
    fn version(&self) -> u32 {
        SCAN_VERSION
    }
    fn arity(&self) -> ScanArity {
        ScanArity::Pair
    }
    fn param_schema(&self) -> serde_json::Value {
        cointegration_rolling_param_schema()
    }
    fn finding_fields(&self) -> ScanFindingShape {
        FINDING_SHAPE
    }
    /// Phase 5 (D5-04 / HYG-03) — opt-in to bootstrap CI (matches the rolling
    /// siblings + the single-shot Engle-Granger scan).
    fn supports_bootstrap(&self) -> bool {
        true
    }

    /// Phase 5 (D5-04 / HYG-04) — opt-in to `CircularShift` (the rolling-scan
    /// convention; `PhaseScramble` is not meaningful for a per-window slide).
    fn supports_null_method(&self, m: crate::scan::NullMethod) -> bool {
        matches!(m, crate::scan::NullMethod::CircularShift)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Scan::run is the linear dispatch + envelope build path; splitting into helpers obscures the Pattern A structure"
    )]
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        // 1. Cancel-at-entry.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 2. Pair-arity borrow.
        let (bars_a, bars_b) = ctx.bars_pair().ok_or_else(|| {
            ScanError::Kernel("expected Pair arity (ctx.bars_pair is None)".into())
        })?;

        // 3. Inner-join (CROSS-01 primitive) — operates on LEVELS (closes).
        let aligned = inner_join(bars_a, bars_b);
        let aligned_n = aligned.timestamps_ms.len();
        if aligned_n == 0 {
            return Err(ScanError::Kernel(
                "inner join produced zero overlap between legs".into(),
            ));
        }

        // 4. Resolve + validate params (window vs min_aligned_n / aligned_n,
        //    hysteresis ordering, positive band).
        let params = resolve_params(req, aligned_n)?;

        // 5. Cancel before kernel.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 6. Roll the per-fit kernel over the aligned levels (y=leg_a, x=leg_b).
        let roll = kernel::rolling_cointegration(
            &aligned.close_a,
            &aligned.close_b,
            params.window,
            params.step,
            params.regression,
            params.thresholds,
        );
        debug_assert!(
            !roll.is_empty(),
            "window <= aligned_n guarantees >= 1 window"
        );

        // 7. NaN detection — a zero-variance regressor window surfaces as a NaN
        //    β (singular OLS system); reject the run as the rolling siblings do.
        if let Some(idx) = roll.betas.iter().position(|v| v.is_nan()) {
            return Err(ScanError::Kernel(format!(
                "zero-variance regressor (leg b) at window {idx}: Engle-Granger OLS system singular"
            )));
        }

        // 8. Cancel-poll between kernel and envelope.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 9. Window→timestamp mapping. Levels map 1:1 to aligned indices (no
        //    +1 returns shift); window i opens at aligned index window_start_idx
        //    and closes at window_end_idx.
        let window_starts_ms: Vec<f64> = roll
            .window_start_idx
            .iter()
            .map(|&i| ms_as_f64(aligned.timestamps_ms[i]))
            .collect();
        let window_ends_ms: Vec<f64> = roll
            .window_end_idx
            .iter()
            .map(|&i| ms_as_f64(aligned.timestamps_ms[i]))
            .collect();

        let breakdown_flags_f: Vec<f64> = roll
            .breakdown_flags
            .iter()
            .map(|&b| if b { 1.0 } else { 0.0 })
            .collect();
        let breakdown_reason_codes: Vec<f64> =
            roll.breakdown_reasons.iter().map(|r| r.as_f64()).collect();

        // 10. effect.extra arrays (BTreeMap → alphabetical wire order).
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "adf_p_values".into(),
            f64_slice_to_raw_array(&roll.adf_p_values),
        );
        extra.insert("adf_stats".into(), f64_slice_to_raw_array(&roll.adf_stats));
        extra.insert("alphas".into(), f64_slice_to_raw_array(&roll.alphas));
        extra.insert("betas".into(), f64_slice_to_raw_array(&roll.betas));
        extra.insert(
            "breakdown_flags".into(),
            f64_slice_to_raw_array(&breakdown_flags_f),
        );
        extra.insert(
            "breakdown_reason_codes".into(),
            f64_slice_to_raw_array(&breakdown_reason_codes),
        );
        extra.insert(
            "ou_half_lives".into(),
            f64_slice_to_raw_array(&roll.ou_half_lives),
        );
        extra.insert(
            "residual_stds".into(),
            f64_slice_to_raw_array(&roll.residual_stds),
        );
        extra.insert(
            "step".into(),
            f64_slice_to_raw_array(&[usize_as_f64(params.step)]),
        );
        extra.insert(
            "window_ends_ms".into(),
            f64_slice_to_raw_array(&window_ends_ms),
        );
        extra.insert(
            "window_length".into(),
            f64_slice_to_raw_array(&[usize_as_f64(params.window)]),
        );
        extra.insert(
            "window_starts_ms".into(),
            f64_slice_to_raw_array(&window_starts_ms),
        );

        let breakdown_count = roll.breakdown_count();
        let breakdown_count_f = usize_as_f64(breakdown_count);

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: breakdown_count_f,
            // No single hypothesis test — per-window ADF p-values live in the
            // adf_p_values array; the headline is the breakdown count.
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "n_windows <= aligned_n <= u64 on supported targets (64-bit only)"
            )]
            n: Some(roll.len() as u64),
            ci95: None,
            effect_size: Some(EffectSize {
                kind: "breakdown_window_count".to_string(),
                value: breakdown_count_f,
            }),
            extra,
        };

        // 11. raw.series — leg-labelled CLOSES + canonical aligned timestamps.
        let timestamps_ms_f: Vec<f64> = aligned
            .timestamps_ms
            .iter()
            .map(|ms| ms_as_f64(*ms))
            .collect();
        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert("close_a".into(), f64_slice_to_raw_array(&aligned.close_a));
        series.insert("close_b".into(), f64_slice_to_raw_array(&aligned.close_b));
        series.insert(
            "timestamps_ms".into(),
            f64_slice_to_raw_array(&timestamps_ms_f),
        );
        let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;

        // 12. D4-03: two-leg sources Vec.
        let per_leg_source_ids = (bars_a.source_id.clone(), bars_b.source_id.clone());
        let mut sources: Vec<Source> = Vec::with_capacity(2);
        if let (Some(spec_a), Some(spec_b)) = (req.instruments.first(), req.instruments.get(1)) {
            sources.push(Source {
                source_id: per_leg_source_ids.0,
                symbol: spec_a.symbol.clone(),
                side: spec_a.side.as_str().to_string(),
                timeframe: req.timeframe.as_str().to_string(),
            });
            sources.push(Source {
                source_id: per_leg_source_ids.1,
                symbol: spec_b.symbol.clone(),
                side: spec_b.side.as_str().to_string(),
                timeframe: req.timeframe.as_str().to_string(),
            });
        } else {
            return Err(ScanError::Kernel(format!(
                "{SCAN_ID}: expected 2 instruments; got {}",
                req.instruments.len()
            )));
        }

        let result_finding = ResultFinding {
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

        sink.write_envelope(&Finding::Result(result_finding))?;
        Ok(())
    }
}

/// Resolve + validate the parameter surface against the aligned-bar count.
fn resolve_params(req: &ScanRequest, aligned_n: usize) -> Result<ResolvedParams, ScanError> {
    let p = &req.resolved_params;

    // min_aligned_n — defaults to the Engle-Granger floor.
    let min_aligned_n = match p.get("min_aligned_n") {
        None => engle_granger::MIN_ALIGNED_N,
        Some(v) => {
            let n = v.as_i64().ok_or_else(|| {
                ScanError::Kernel(format!("min_aligned_n must be an integer; got {v}"))
            })?;
            if n < 2 {
                return Err(ScanError::Kernel(format!(
                    "min_aligned_n must be >= 2; got {n}"
                )));
            }
            usize::try_from(n).map_err(|_| {
                ScanError::Kernel(format!("min_aligned_n out of range for usize: {n}"))
            })?
        }
    };

    // window — required.
    let window_raw = p
        .get("window")
        .ok_or_else(|| ScanError::Kernel("rolling cointegration: window param required".into()))?;
    let window_i64 = window_raw
        .as_i64()
        .ok_or_else(|| ScanError::Kernel(format!("window must be an integer; got {window_raw}")))?;
    if window_i64 < 2 {
        return Err(ScanError::Kernel(format!(
            "window must be >= 2; got {window_i64}"
        )));
    }
    let window = usize::try_from(window_i64)
        .map_err(|_| ScanError::Kernel(format!("window out of range for usize: {window_i64}")))?;
    if window < min_aligned_n {
        return Err(ScanError::Kernel(format!(
            "window must be >= min_aligned_n (= {min_aligned_n}); got window={window}"
        )));
    }
    if window > aligned_n {
        return Err(ScanError::Kernel(format!(
            "window must be <= aligned_n (= {aligned_n}); got window={window}"
        )));
    }

    // step — default 1.
    let step_i64 = match p.get("step") {
        None => DEFAULT_STEP,
        Some(v) => v
            .as_i64()
            .ok_or_else(|| ScanError::Kernel(format!("step must be an integer; got {v}")))?,
    };
    if step_i64 < 1 {
        return Err(ScanError::Kernel(format!(
            "step must be >= 1; got {step_i64}"
        )));
    }
    let step = usize::try_from(step_i64)
        .map_err(|_| ScanError::Kernel(format!("step out of range for usize: {step_i64}")))?;

    // Breakdown thresholds — defaults from BreakdownThresholds::default().
    let defaults = BreakdownThresholds::default();
    let adf_p_enter = resolve_unit_prob(p, "adf_p_enter", defaults.adf_p_enter)?;
    let adf_p_exit = resolve_unit_prob(p, "adf_p_exit", defaults.adf_p_exit)?;
    if adf_p_enter > adf_p_exit {
        return Err(ScanError::Kernel(format!(
            "adf_p_enter ({adf_p_enter}) must be <= adf_p_exit ({adf_p_exit}) for breakdown hysteresis"
        )));
    }
    let beta_band = match p.get("beta_band") {
        None => defaults.beta_band,
        Some(v) => {
            let b = v
                .as_f64()
                .ok_or_else(|| ScanError::Kernel(format!("beta_band must be a number; got {v}")))?;
            if !b.is_finite() || b <= 0.0 {
                return Err(ScanError::Kernel(format!(
                    "beta_band must be a finite positive number; got {b}"
                )));
            }
            b
        }
    };

    let regression = resolve_regression(req)?;

    Ok(ResolvedParams {
        window,
        step,
        thresholds: BreakdownThresholds {
            adf_p_enter,
            adf_p_exit,
            beta_band,
        },
        regression,
    })
}

/// Resolve an optional `[0, 1]` probability-typed param with a default.
fn resolve_unit_prob(
    params: &serde_json::Value,
    key: &str,
    default: f64,
) -> Result<f64, ScanError> {
    match params.get(key) {
        None => Ok(default),
        Some(v) => {
            let x = v
                .as_f64()
                .ok_or_else(|| ScanError::Kernel(format!("{key} must be a number; got {v}")))?;
            if !x.is_finite() || !(0.0..=1.0).contains(&x) {
                return Err(ScanError::Kernel(format!(
                    "{key} must be in [0, 1]; got {x}"
                )));
            }
            Ok(x)
        }
    }
}

/// Resolve the `regression` param — optional enum `"c"` (default) or `"ct"`.
/// Mirrors `cross.cointegration.engle_granger`.
fn resolve_regression(req: &ScanRequest) -> Result<engle_granger::AdfRegression, ScanError> {
    match req.resolved_params.get("regression") {
        None => Ok(engle_granger::AdfRegression::Constant),
        Some(v) => {
            let s = v.as_str().ok_or_else(|| {
                ScanError::Kernel(format!("regression must be a string; got {v}"))
            })?;
            match s {
                "c" => Ok(engle_granger::AdfRegression::Constant),
                "ct" => Ok(engle_granger::AdfRegression::ConstantTrend),
                other => Err(ScanError::Kernel(format!(
                    "regression must be one of [c, ct]; got {other:?}"
                ))),
            }
        }
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
)]
fn ms_as_f64(ms: i64) -> f64 {
    ms as f64
}

#[allow(
    clippy::cast_precision_loss,
    reason = "window/step/count bounded by aligned_n; realistic values are << 2^52"
)]
fn usize_as_f64(v: usize) -> f64 {
    v as f64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless, clippy::float_cmp)]
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

    fn make_ts(n: usize) -> Vec<DateTime<Utc>> {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect()
    }

    fn build_bars(symbol: &str, ts: &[DateTime<Utc>], closes: &[f64]) -> BarFrame {
        assert_eq!(ts.len(), closes.len());
        BarFrame {
            source_id: "dukascopy".into(),
            symbol: symbol.into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts.to_vec(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
            open: closes.to_vec(),
            high: closes.iter().map(|c| c + 0.001).collect(),
            low: closes.iter().map(|c| c - 0.001).collect(),
            close: closes.to_vec(),
            tick_volume: vec![1.0; ts.len()],
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn random_walk(n: usize, seed: u64, scale: f64) -> Vec<f64> {
        let mut s = seed as u32;
        let mut acc = 1.0_f64;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX) - 0.5;
            acc += frac * scale;
            out.push(acc);
        }
        out
    }

    fn sample_request(params: serde_json::Value) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: SCAN_ID.into(),
            version: SCAN_VERSION,
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

    fn make_ctx<'a>(a: &'a BarFrame, b: &'a BarFrame, cancel: Arc<AtomicBool>) -> ScanCtx<'a> {
        ScanCtx {
            bars: a,
            bars_pair: Some((a, b)),
            gap_manifest: None,
            run_id: RunId::new(),
            code_revision: "abc1234",
            cancel,
            sleep_after_first_finding_ms: None,
        }
    }

    fn parse_findings(sink: &VecSink) -> Vec<Finding> {
        sink.0
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<Finding>(line).expect("parse"))
            .collect()
    }

    fn decode_f64(arr: &RawArray) -> Vec<f64> {
        let bytes = &arr.data.0;
        assert_eq!(bytes.len() % 8, 0);
        let mut out = Vec::with_capacity(bytes.len() / 8);
        for chunk in bytes.chunks_exact(8) {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(chunk);
            out.push(f64::from_le_bytes(buf));
        }
        out
    }

    /// Cointegrated pair: b is a walk, a = b + mean-reverting AR(1) noise.
    fn cointegrated_pair(n: usize, seed_b: u64, seed_e: u64) -> (Vec<f64>, Vec<f64>) {
        let b = random_walk(n, seed_b, 0.02);
        let mut se = seed_e as u32;
        let mut e_prev = 0.0_f64;
        let mut a = Vec::with_capacity(n);
        for &bi in &b {
            se = se.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(se) / f64::from(u32::MAX) - 0.5) * 0.01;
            let e = 0.3_f64 * e_prev + noise;
            a.push(bi + e);
            e_prev = e;
        }
        (a, b)
    }

    #[test]
    fn id_arity_version() {
        let s = CointegrationRollingScan;
        assert_eq!(s.id(), "cross.cointegration.rolling");
        assert_eq!(s.version(), 1);
        assert_eq!(s.arity(), ScanArity::Pair);
        assert_eq!(s.arity().expected_len(), 2);
    }

    #[test]
    fn param_schema_requires_window() {
        let schema = CointegrationRollingScan.param_schema();
        assert_eq!(schema["type"], "object");
        let required = schema["required"].as_array().expect("required array");
        assert!(required.iter().any(|v| v == "window"));
        assert_eq!(schema["properties"]["step"]["default"], 1);
        assert_eq!(schema["properties"]["min_aligned_n"]["default"], 30);
        assert_eq!(schema["properties"]["adf_p_exit"]["default"], 0.10);
    }

    /// Does NOT coalesce sub-ranges — rolling windows must not span gaps.
    #[test]
    fn does_not_coalesce_subranges() {
        assert!(!CointegrationRollingScan.coalesce_subranges());
    }

    #[test]
    fn emits_one_result_with_per_window_arrays() {
        let n = 200;
        let (a_closes, b_closes) = cointegrated_pair(n, 7, 11);
        let ts = make_ts(n);
        let a = build_bars("EURUSD", &ts, &a_closes);
        let b = build_bars("GBPUSD", &ts, &b_closes);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 60, "step": 10}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        CointegrationRollingScan
            .run(&ctx, &req, &mut sink)
            .expect("ok");
        let findings = parse_findings(&sink);
        assert_eq!(findings.len(), 1);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "cross.cointegration.rolling@1");
        assert_eq!(r.effect.metric, "cointegration_rolling_breakdown_count");
        assert!(r.effect.p_value.is_none());
        // n_windows = (200 - 60)/10 + 1 = 15.
        assert_eq!(r.effect.n, Some(15));
        // extra keys alphabetical via BTreeMap.
        let keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "adf_p_values",
                "adf_stats",
                "alphas",
                "betas",
                "breakdown_flags",
                "breakdown_reason_codes",
                "ou_half_lives",
                "residual_stds",
                "step",
                "window_ends_ms",
                "window_length",
                "window_starts_ms",
            ]
        );
        // per-window arrays all length 15.
        for key in [
            "adf_p_values",
            "adf_stats",
            "alphas",
            "betas",
            "breakdown_flags",
            "breakdown_reason_codes",
            "ou_half_lives",
            "residual_stds",
            "window_ends_ms",
            "window_starts_ms",
        ] {
            assert_eq!(decode_f64(&r.effect.extra[key]).len(), 15, "len {key}");
        }
        // scalars.
        assert_eq!(decode_f64(&r.effect.extra["window_length"]), vec![60.0]);
        assert_eq!(decode_f64(&r.effect.extra["step"]), vec![10.0]);
        // window_ends are strictly after window_starts.
        let starts = decode_f64(&r.effect.extra["window_starts_ms"]);
        let ends = decode_f64(&r.effect.extra["window_ends_ms"]);
        for (s, e) in starts.iter().zip(ends.iter()) {
            assert!(e > s, "window end {e} must be after start {s}");
        }
        // raw carries levels + canonical timestamps.
        let raw = r.raw.as_ref().expect("raw present");
        assert!(raw.series.contains_key("close_a"));
        assert!(raw.series.contains_key("close_b"));
        assert!(raw.series.contains_key("timestamps_ms"));
        assert_eq!(r.data_slice.sources.len(), 2);
    }

    #[test]
    fn window_below_min_aligned_n_rejected() {
        let n = 100;
        let (a_closes, b_closes) = cointegrated_pair(n, 1, 2);
        let ts = make_ts(n);
        let a = build_bars("EURUSD", &ts, &a_closes);
        let b = build_bars("GBPUSD", &ts, &b_closes);
        let mut sink = VecSink::new();
        // window 20 < default min_aligned_n 30.
        let req = sample_request(serde_json::json!({"window": 20}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = CointegrationRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("window < min_aligned_n must reject");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("min_aligned_n"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn window_larger_than_aligned_rejected() {
        let n = 40;
        let (a_closes, b_closes) = cointegrated_pair(n, 1, 2);
        let ts = make_ts(n);
        let a = build_bars("EURUSD", &ts, &a_closes);
        let b = build_bars("GBPUSD", &ts, &b_closes);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 50}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = CointegrationRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("window > aligned_n must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn hysteresis_ordering_validated() {
        let n = 120;
        let (a_closes, b_closes) = cointegrated_pair(n, 1, 2);
        let ts = make_ts(n);
        let a = build_bars("EURUSD", &ts, &a_closes);
        let b = build_bars("GBPUSD", &ts, &b_closes);
        let mut sink = VecSink::new();
        let req = sample_request(
            serde_json::json!({"window": 60, "adf_p_enter": 0.2, "adf_p_exit": 0.1}),
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = CointegrationRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("enter > exit must reject");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("hysteresis"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn cancellation_emits_nothing() {
        let n = 100;
        let (a_closes, b_closes) = cointegrated_pair(n, 1, 2);
        let ts = make_ts(n);
        let a = build_bars("EURUSD", &ts, &a_closes);
        let b = build_bars("GBPUSD", &ts, &b_closes);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 60}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(true)));
        CointegrationRollingScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel returns Ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn zero_variance_leg_b_rejected() {
        let n = 100;
        let ts = make_ts(n);
        let closes_b = vec![1.0_f64; n];
        let closes_a: Vec<f64> = (0..n)
            .map(|i| 1.0 + 0.01 * f64::from(i32::try_from(i).unwrap()))
            .collect();
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 60}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = CointegrationRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("constant leg b must reject");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("singular"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }
}

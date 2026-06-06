//! `CusumBreakScan` — `stats.regime.cusum_break@1` single-leg CUSUM
//! structural-break / regime-detection scan (RAD-3841 / Tier-2 build for
//! RAD-3545).
//!
//! Pattern analog: [`crate::scan::anom::meanrev::OuHalfLifeScan`] (sibling
//! single-leg ANOM scan) for the surface; emission shape mirrors
//! [`crate::scan::cross::cointegration_rolling`] (one located
//! `Finding::Result` per detected break, not one aggregated envelope) because a
//! structural break is a *located* event the consumer locates in time.
//!
//! ## Purpose
//!
//! Scout ranking (RAD-3548): ★★ standalone, ★★★ as a *conditioning filter* on
//! other signals. It detects where the mean (drift) and/or volatility of the
//! single-leg log-return series structurally changes — a meta-signal that
//! strengthens RAD-3626's cointegration-breakdown detection and gates entries
//! to a single volatility/drift regime. The break statistics and per-segment
//! stats are surfaced; the consumer decides how to condition on them.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.regime.cusum_break"`, `version = 1`, `arity =
//!   ScanArity::Single`.
//! - `params`: `target` (enum `["mean","vol","both"]`, default `"both"`);
//!   `threshold` (number > 0, default 1.358 — the Kolmogorov 5 % critical
//!   value, LOWER ⇒ more sensitive); `min_segment` (integer >= 2, default 30 —
//!   minimum regime length in return bars).
//! - Emits ONE `Finding::Result` per detected break. A stationary series emits
//!   none (acceptance #2).
//! - `effect.metric = "cusum_break"`, `effect.value` = the normalized CUSUM
//!   statistic at the break, `effect.p_value = None`, `effect.n` = the analysed
//!   return-series length. `effect.effect_size` = `{kind: "mean_shift", value:
//!   standardized mean shift}` for a mean break or `{kind: "vol_shift", value:
//!   post/pre std ratio}` for a vol break.
//! - `effect.extra` (alphabetical `BTreeMap`, all length-1 `F64` except the
//!   `target` UTF-8 label): `break_index` (CLOSE-series index of the break),
//!   `break_ts_ms`, `post_mean`, `post_median`, `post_n`, `post_std`,
//!   `pre_mean`, `pre_median`, `pre_n`, `pre_std`, `regime_index` (1-based
//!   ordinal of the regime the break opens), `target` (`"mean"`/`"vol"`).
//! - `raw.series = {closes, timestamps_ms}` (the full close series, so the
//!   located break is reconstructable).
//!
//! ## Registration
//!
//! Appended inside [`crate::scan::anom::register_anom_scans`] (Pattern E —
//! `registry.rs` is NOT modified).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;
use serde_json::Value as JsonValue;

use crate::findings::{
    Base64Bytes, DataSlice, Dtype, Effect, EffectSize, Finding, FindingSink, Raw, RawArray,
    ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

use kernel::{BreakKind, BreakPoint, BreakTarget};

/// ANOM single-leg CUSUM structural-break / regime-detection scan.
pub struct CusumBreakScan;

const SCAN_ID: &str = "stats.regime.cusum_break";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "cusum_break";

/// A dispersion at or below this makes a standardized effect-size undefined;
/// the effect-size falls back to its neutral value (0 shift / ratio 1).
const EFFECT_EPS: f64 = 1e-12;

impl Scan for CusumBreakScan {
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
                "target": {
                    "type": "string",
                    "enum": ["mean", "vol", "both"],
                    "default": "both",
                    "description": "Which structural break(s) to detect in the log-return series: 'mean' (drift), 'vol' (volatility), or 'both' (default; each segmented independently)."
                },
                "threshold": {
                    "type": "number",
                    "exclusiveMinimum": 0,
                    "default": 1.358,
                    "description": "Break threshold on the normalized CUSUM statistic (Kolmogorov 5% critical value default). LOWER is more sensitive (more breaks)."
                },
                "min_segment": {
                    "type": "integer",
                    "minimum": 2,
                    "default": 30,
                    "description": "Minimum regime length in return bars. A segment is never split below 2*min_segment, and an accepted break leaves both sides >= min_segment."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[
                "break_index",
                "break_ts_ms",
                "post_mean",
                "post_median",
                "post_n",
                "post_std",
                "pre_mean",
                "pre_median",
                "pre_n",
                "pre_std",
                "regime_index",
                "target",
            ],
            raw_series_keys: &["closes", "timestamps_ms"],
        }
    }

    /// A located-event scan (one finding per break) does not carry a single
    /// effect-size sampling distribution, so — like the sibling located-event
    /// scans (`outliers`, `drawdown`) — it opts out of bootstrap / null passes.
    fn supports_bootstrap(&self) -> bool {
        false
    }

    fn supports_null_method(&self, _m: crate::scan::NullMethod) -> bool {
        false
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Scan::run is the linear param-resolve + per-break envelope build path; splitting obscures the Pattern A structure"
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

        // Step 2 — resolve + validate params.
        let target = resolve_target(req)?;
        let threshold = resolve_threshold(req)?;
        let min_segment = resolve_min_segment(req)?;

        // Step 3 — detect breaks on the log returns of the close series. A
        // stationary / too-short / degenerate series yields zero breaks ⇒ zero
        // findings (acceptance #2), mirroring the cointegration_rolling
        // empty-window path.
        let closes = &ctx.bars.close;
        let breaks = kernel::cusum_break(closes, target, threshold, min_segment);
        if breaks.is_empty() {
            return Ok(());
        }
        let return_len = closes.len().saturating_sub(1);

        // Step 4 — shared timestamp vector + sources (identical across the
        // per-break envelopes for this dispatch).
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

        // Step 5 — one envelope per detected break.
        for bp in &breaks {
            // Map RETURN-series index → CLOSE-series index (r_t = ln(c_t/c_{t-1})
            // ⇒ return index i corresponds to close index i+1).
            let close_idx = bp.index + 1;
            let break_ts_ms = ts_ms.get(close_idx).copied().unwrap_or(f64::NAN);

            let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
            extra.insert(
                "break_index".into(),
                f64_slice_to_raw_array(&[index_to_f64(close_idx)]),
            );
            extra.insert("break_ts_ms".into(), f64_slice_to_raw_array(&[break_ts_ms]));
            extra.insert("post_mean".into(), f64_slice_to_raw_array(&[bp.post_mean]));
            extra.insert(
                "post_median".into(),
                f64_slice_to_raw_array(&[bp.post_median]),
            );
            extra.insert(
                "post_n".into(),
                f64_slice_to_raw_array(&[index_to_f64(bp.post_n)]),
            );
            extra.insert("post_std".into(), f64_slice_to_raw_array(&[bp.post_std]));
            extra.insert("pre_mean".into(), f64_slice_to_raw_array(&[bp.pre_mean]));
            extra.insert(
                "pre_median".into(),
                f64_slice_to_raw_array(&[bp.pre_median]),
            );
            extra.insert(
                "pre_n".into(),
                f64_slice_to_raw_array(&[index_to_f64(bp.pre_n)]),
            );
            extra.insert("pre_std".into(), f64_slice_to_raw_array(&[bp.pre_std]));
            extra.insert(
                "regime_index".into(),
                f64_slice_to_raw_array(&[index_to_f64(bp.regime_index)]),
            );
            extra.insert("target".into(), string_label_to_raw_array(bp.kind.label()));

            let effect = Effect {
                metric: EFFECT_METRIC.to_string(),
                value: bp.statistic,
                p_value: None,
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "return-series length <= u64 on all supported targets"
                )]
                n: Some(return_len as u64),
                ci95: None,
                effect_size: Some(effect_size_for(bp)),
                extra,
            };

            let mut series_map: BTreeMap<String, RawArray> = BTreeMap::new();
            series_map.insert("closes".into(), f64_slice_to_raw_array(closes));
            series_map.insert("timestamps_ms".into(), f64_slice_to_raw_array(&ts_ms));
            let raw_block = Raw::new(series_map).map_err(|m| ScanError::Kernel(m.to_string()))?;

            let finding = ResultFinding {
                schema_version: 1,
                scan_id_at_version: format!("{SCAN_ID}@{SCAN_VERSION}"),
                param_hash: req.param_hash.as_str().to_string(),
                code_revision: ctx.code_revision.to_string(),
                data_slice: DataSlice {
                    range: req.sub_range.clone(),
                    gap_manifest_ref: None,
                    gap_manifest: ctx.gap_manifest.cloned(),
                    sources: sources.clone(),
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
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Standardized effect size for a break: a Cohen-style standardized mean shift
/// for a mean break, or the post/pre std ratio for a vol break. Both fall back
/// to a neutral value when the relevant dispersion is degenerate.
fn effect_size_for(bp: &BreakPoint) -> EffectSize {
    match bp.kind {
        BreakKind::Mean => {
            let pooled = pooled_std(bp);
            let value = if pooled > EFFECT_EPS {
                (bp.post_mean - bp.pre_mean) / pooled
            } else {
                0.0
            };
            EffectSize {
                kind: "mean_shift".into(),
                value,
            }
        }
        BreakKind::Vol => {
            let value = if bp.pre_std > EFFECT_EPS && bp.post_std.is_finite() {
                bp.post_std / bp.pre_std
            } else {
                1.0
            };
            EffectSize {
                kind: "vol_shift".into(),
                value,
            }
        }
    }
}

/// Pooled sample std of the pre/post segments (ddof = 1), for the standardized
/// mean-shift effect size.
fn pooled_std(bp: &BreakPoint) -> f64 {
    let dof = bp.pre_n + bp.post_n;
    if dof < 3 {
        return 0.0;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "segment lengths << 2^52 for any realistic bar count"
    )]
    let denom = (dof - 2) as f64;
    #[allow(
        clippy::cast_precision_loss,
        reason = "segment lengths << 2^52 for any realistic bar count"
    )]
    let num = (bp.pre_n.saturating_sub(1)) as f64 * bp.pre_std.powi(2)
        + (bp.post_n.saturating_sub(1)) as f64 * bp.post_std.powi(2);
    (num / denom).sqrt()
}

fn resolve_target(req: &ScanRequest) -> Result<BreakTarget, ScanError> {
    let raw = req.resolved_params.get("target");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.regime.cusum_break: target must be mean|vol|both; got {v}"
            ))
        })?,
        None => "both",
    };
    BreakTarget::from_label(label).ok_or_else(|| {
        ScanError::Kernel(format!(
            "stats.regime.cusum_break: target must be mean|vol|both; got {label:?}"
        ))
    })
}

fn resolve_threshold(req: &ScanRequest) -> Result<f64, ScanError> {
    let Some(v) = req.resolved_params.get("threshold") else {
        return Ok(kernel::DEFAULT_THRESHOLD);
    };
    let t = v.as_f64().ok_or_else(|| {
        ScanError::Kernel(format!(
            "stats.regime.cusum_break: threshold must be a number; got {v}"
        ))
    })?;
    if !t.is_finite() || t <= 0.0 {
        return Err(ScanError::Kernel(format!(
            "stats.regime.cusum_break: threshold must be a finite number > 0; got {t}"
        )));
    }
    Ok(t)
}

fn resolve_min_segment(req: &ScanRequest) -> Result<usize, ScanError> {
    let Some(v) = req.resolved_params.get("min_segment") else {
        return Ok(kernel::DEFAULT_MIN_SEGMENT);
    };
    let i = v.as_i64().ok_or_else(|| {
        ScanError::Kernel(format!(
            "stats.regime.cusum_break: min_segment must be an integer; got {v}"
        ))
    })?;
    let min_segment = usize::try_from(i).map_err(|_| {
        ScanError::Kernel(format!(
            "stats.regime.cusum_break: min_segment must be >= {}; got {i}",
            kernel::MIN_SEGMENT_FLOOR
        ))
    })?;
    if min_segment < kernel::MIN_SEGMENT_FLOOR {
        return Err(ScanError::Kernel(format!(
            "stats.regime.cusum_break: min_segment must be >= {}; got {min_segment}",
            kernel::MIN_SEGMENT_FLOOR
        )));
    }
    Ok(min_segment)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "index << 2^52 for any realistic bar count"
)]
#[inline]
fn index_to_f64(i: usize) -> f64 {
    i as f64
}

/// Pack a UTF-8 label into a `RawArray`'s data field as bytes — same trick used
/// by ANOM-05 `regression` / `meanrev` `on`. The v1 wire-form supports only
/// `Dtype::F64`; consumers decode via `std::str::from_utf8(&extra[k].data.0)`.
fn string_label_to_raw_array(label: &str) -> RawArray {
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

    /// Build closes from a return series: `c_0 = 1.0`, `c_t = c_{t-1}·exp(r_t)`
    /// so `log_returns(closes) == returns`.
    fn closes_from_returns(returns: &[f64]) -> Vec<f64> {
        let mut c = 1.0_f64;
        let mut out = Vec::with_capacity(returns.len() + 1);
        out.push(c);
        for r in returns {
            c *= r.exp();
            out.push(c);
        }
        out
    }

    fn noise(n: usize, seed: u32, scale: f64) -> Vec<f64> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (f64::from(s) / f64::from(u32::MAX) - 0.5) * scale
            })
            .collect()
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

    fn sample_request(params: serde_json::Value) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap();
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
    fn id_arity_version() {
        assert_eq!(CusumBreakScan.id(), "stats.regime.cusum_break");
        assert_eq!(CusumBreakScan.version(), 1);
        assert_eq!(CusumBreakScan.arity(), ScanArity::Single);
    }

    #[test]
    fn param_schema_defaults() {
        let s = CusumBreakScan.param_schema();
        assert_eq!(s["properties"]["target"]["default"], "both");
        assert_eq!(s["properties"]["threshold"]["default"], 1.358);
        assert_eq!(s["properties"]["min_segment"]["default"], 30);
        assert_eq!(s["additionalProperties"], false);
    }

    #[test]
    fn stationary_emits_nothing() {
        let closes = closes_from_returns(&noise(400, 0x1111_2222, 0.01));
        let mut sink = VecSink::new();
        CusumBreakScan
            .run(
                &make_ctx(
                    &bar_frame_from_closes(closes),
                    Arc::new(AtomicBool::new(false)),
                ),
                &sample_request(serde_json::json!({})),
                &mut sink,
            )
            .expect("ok");
        assert!(sink.0.is_empty(), "stationary series must emit no findings");
    }

    #[test]
    fn mean_break_emits_located_finding() {
        let cp = 200usize;
        let mut rets = noise(400, 0xABCD_0001, 0.01);
        for v in rets.iter_mut().skip(cp) {
            *v += 0.05;
        }
        let closes = closes_from_returns(&rets);
        let mut sink = VecSink::new();
        CusumBreakScan
            .run(
                &make_ctx(
                    &bar_frame_from_closes(closes),
                    Arc::new(AtomicBool::new(false)),
                ),
                &sample_request(serde_json::json!({"target": "mean"})),
                &mut sink,
            )
            .expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert!(!findings.is_empty(), "must emit at least one break");
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.regime.cusum_break@1");
        assert_eq!(r.effect.metric, "cusum_break");
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec![
                "break_index",
                "break_ts_ms",
                "post_mean",
                "post_median",
                "post_n",
                "post_std",
                "pre_mean",
                "pre_median",
                "pre_n",
                "pre_std",
                "regime_index",
                "target",
            ]
        );
        // target label echoed.
        let target_bytes = &r.effect.extra["target"].data.0;
        assert_eq!(std::str::from_utf8(target_bytes).unwrap(), "mean");
        // located near the true changepoint (close-space index ≈ cp+1).
        let nearest = findings
            .iter()
            .filter_map(|f| match f {
                Finding::Result(r) => Some(decode_f64(&r.effect.extra["break_index"]) as usize),
                _ => None,
            })
            .map(|bi| bi.abs_diff(cp + 1))
            .min()
            .expect("non-empty");
        assert!(nearest <= 16, "nearest break {nearest} bars from cp");
        // effect_size is a mean_shift.
        assert_eq!(r.effect.effect_size.as_ref().unwrap().kind, "mean_shift");
    }

    #[test]
    fn invalid_target_rejected() {
        let closes = closes_from_returns(&noise(100, 1, 0.01));
        let mut sink = VecSink::new();
        let err = CusumBreakScan
            .run(
                &make_ctx(
                    &bar_frame_from_closes(closes),
                    Arc::new(AtomicBool::new(false)),
                ),
                &sample_request(serde_json::json!({"target": "garbage"})),
                &mut sink,
            )
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn invalid_threshold_rejected() {
        let closes = closes_from_returns(&noise(100, 1, 0.01));
        let mut sink = VecSink::new();
        let err = CusumBreakScan
            .run(
                &make_ctx(
                    &bar_frame_from_closes(closes),
                    Arc::new(AtomicBool::new(false)),
                ),
                &sample_request(serde_json::json!({"threshold": 0.0})),
                &mut sink,
            )
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn cancellation_emits_nothing() {
        let cp = 200usize;
        let mut rets = noise(400, 0xDEAD_0001, 0.01);
        for v in rets.iter_mut().skip(cp) {
            *v += 0.05;
        }
        let closes = closes_from_returns(&rets);
        let mut sink = VecSink::new();
        CusumBreakScan
            .run(
                &make_ctx(
                    &bar_frame_from_closes(closes),
                    Arc::new(AtomicBool::new(true)),
                ),
                &sample_request(serde_json::json!({})),
                &mut sink,
            )
            .expect("ok");
        assert!(sink.0.is_empty());
    }
}

//! `TsmomScan` — ANOM-12 intraday time-series momentum / return continuation.
//!
//! Pattern analog: [`crate::scan::anom::variance_ratio::VarianceRatioScan`]
//! (Plan 04-05 sibling) — multi-`k` ANOM scan emitting parallel arrays in
//! `effect.extra` (Pattern A from `04-PATTERNS.md`). Tier-2 build for
//! RAD-3545 / RAD-3839.
//!
//! ## Reference
//!
//! Moskowitz, T. J., Ooi, Y. H. & Pedersen, L. H. (2012), "Time Series
//! Momentum", Journal of Financial Economics 104(2), 228-250. Unlike
//! `stats.autocorr.ljung_box` (which only reports an autocorrelation
//! p-value), this scan frames the dependence of the next-`k`-bar return on the
//! past-`k`-bar return as a *tradeable continuation signal*: sign + magnitude
//! (`β`) + significance (t-stat / p) + directional hit-rate, plus the hold
//! horizon (which `k` persists) so the Quant agent can read natural turnover
//! straight off the finding.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.momentum.tsmom"`, `version = 1`, `arity = ScanArity::Single`.
//! - `params`: optional `k_values` (lookback/holding horizons in bars, default
//!   `[1, 5, 10, 20]`; each `k >= 1`), optional `scaling` (vol-normalised
//!   returns, default `true`).
//! - `effect.metric = "tsmom_continuation"`, `effect.value = continuation
//!   coefficient at the selected hold horizon` (the `k` whose positive
//!   continuation is most significant; falls back to the strongest |t-stat|
//!   when no horizon shows positive continuation).
//! - `effect.p_value = p of the continuation coefficient at the selected k`.
//! - `effect.effect_size = {kind: "hit_rate", value: hit-rate at selected k}`.
//! - `effect.extra = {continuation_coefs, hit_rates, k_values, p_values,
//!   selected_hold_bars, t_stats, tsmom_means, turnover_per_bar}` (alphabetical
//!   `BTreeMap` order) — parallel per-`k` arrays plus the single-element
//!   `selected_hold_bars` framing the dominant hold horizon.
//! - `raw.series = {returns, timestamps_ms}` (the raw log returns; `scaling`
//!   is a reproducible transform applied to the analysis copy).
//!
//! ## Determinism
//!
//! The `k`-grid loop is SEQUENTIAL (not `par_iter`) — same Pitfall 4
//! discipline as the variance-ratio / ADF scans.
//!
//! ## Registration
//!
//! Appended inside [`crate::scan::anom::register_anom_scans`] (Pattern E —
//! `crates/miner-core/src/scan/registry.rs` is NOT modified).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;
use serde_json::Value as JsonValue;

use crate::findings::{
    DataSlice, Effect, EffectSize, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// ANOM-12 — intraday time-series momentum (return continuation) scan.
pub struct TsmomScan;

const SCAN_ID: &str = "stats.momentum.tsmom";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "tsmom_continuation";

const DEFAULT_K_VALUES: &[i64] = &[1, 5, 10, 20];

impl Scan for TsmomScan {
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
                    "items": { "type": "integer", "minimum": 1 },
                    "default": [1, 5, 10, 20],
                    "description": "Lookback/holding horizons in bars; each k >= 1. Continuation is measured over non-overlapping k-blocks."
                },
                "scaling": {
                    "type": "boolean",
                    "default": true,
                    "description": "If true (default), normalise each return by a trailing ex-ante volatility before forming blocks (vol-scaled momentum). If false, use raw log returns."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[
                "continuation_coefs",
                "hit_rates",
                "k_values",
                "p_values",
                "selected_hold_bars",
                "t_stats",
                "tsmom_means",
                "turnover_per_bar",
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

        // Step 2 — N guard. Need at least 2 closes for log_returns.
        let n_closes = ctx.bars.close.len();
        if n_closes < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.momentum.tsmom: need n >= 2 closes; got n={n_closes} (InsufficientData)"
            )));
        }

        // Step 3 — resolve params.
        let k_values = resolve_k_values(req)?;
        let scaling = resolve_scaling(req)?;

        // Step 4 — compute returns + validate k_values vs returns length.
        let returns = log_returns(&ctx.bars.close);
        let n_returns = returns.len();
        // Smallest meaningful horizon (k=1) needs >= 4 blocks => >= 4 returns.
        if n_returns < 4 {
            return Err(ScanError::Kernel(format!(
                "stats.momentum.tsmom: need >= 4 returns; got {n_returns} (InsufficientData)"
            )));
        }
        for &k in &k_values {
            if k < 1 {
                return Err(ScanError::Kernel(format!(
                    "stats.momentum.tsmom: k_values entries must be >= 1; got {k}"
                )));
            }
            if n_returns / k < 4 {
                return Err(ScanError::Kernel(format!(
                    "stats.momentum.tsmom: k={k} too large for n_returns={n_returns} (need n_returns >= 4*k)"
                )));
            }
        }

        // Analysis series: ex-ante vol-normalised when scaling is on. A global
        // rescale is OLS scale-invariant, so the normalisation is deliberately
        // time-varying (trailing vol) to genuinely vol-scale the signal.
        let analysis: Vec<f64> = if scaling {
            kernel::vol_normalize(&returns, kernel::VOL_WINDOW)
        } else {
            returns.clone()
        };

        // Step 5 — kernel calls (sequential k loop — Pitfall 4 determinism).
        let mut continuation_coefs: Vec<f64> = Vec::with_capacity(k_values.len());
        let mut t_stats: Vec<f64> = Vec::with_capacity(k_values.len());
        let mut p_values: Vec<f64> = Vec::with_capacity(k_values.len());
        let mut hit_rates: Vec<f64> = Vec::with_capacity(k_values.len());
        let mut tsmom_means: Vec<f64> = Vec::with_capacity(k_values.len());
        let mut turnover_per_bar: Vec<f64> = Vec::with_capacity(k_values.len());
        for &k in &k_values {
            let k_result = kernel::tsmom_continuation(&analysis, k).map_err(ScanError::Kernel)?;
            continuation_coefs.push(k_result.continuation_coef);
            t_stats.push(k_result.t_stat);
            p_values.push(k_result.p_value);
            hit_rates.push(k_result.hit_rate);
            tsmom_means.push(k_result.tsmom_mean);
            turnover_per_bar.push(1.0 / usize_to_f64(k));
        }

        let k_values_f64: Vec<f64> = k_values.iter().map(|k| usize_to_f64(*k)).collect();

        // Step 6 — select the hold horizon that best persists (AC3 framing).
        let sel = select_hold_index(&continuation_coefs, &t_stats);
        let selected_hold_bars = vec![k_values_f64[sel]];

        // Step 7 — build raw.series (raw log returns + parallel timestamps).
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

        // Envelope construction. effect.value = continuation at the selected
        // (dominant) hold horizon; p / hit-rate are taken at that same k.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "continuation_coefs".into(),
            f64_slice_to_raw_array(&continuation_coefs),
        );
        extra.insert("hit_rates".into(), f64_slice_to_raw_array(&hit_rates));
        extra.insert("k_values".into(), f64_slice_to_raw_array(&k_values_f64));
        extra.insert("p_values".into(), f64_slice_to_raw_array(&p_values));
        extra.insert(
            "selected_hold_bars".into(),
            f64_slice_to_raw_array(&selected_hold_bars),
        );
        extra.insert("t_stats".into(), f64_slice_to_raw_array(&t_stats));
        extra.insert("tsmom_means".into(), f64_slice_to_raw_array(&tsmom_means));
        extra.insert(
            "turnover_per_bar".into(),
            f64_slice_to_raw_array(&turnover_per_bar),
        );

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: continuation_coefs[sel],
            p_value: Some(p_values[sel]),
            #[allow(
                clippy::cast_possible_truncation,
                reason = "n_returns <= u64 on all supported targets"
            )]
            n: Some(n_returns as u64),
            ci95: None,
            // Tradeable effect size: directional hit-rate at the selected k.
            effect_size: Some(EffectSize {
                kind: "hit_rate".to_string(),
                value: hit_rates[sel],
            }),
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

/// Select the hold horizon (index into the k-grid) that best persists:
/// the largest POSITIVE continuation t-stat (strongest momentum). When no
/// horizon shows positive continuation, fall back to the largest `|t-stat|`.
/// Ties resolve to the earliest index (smallest `k`).
fn select_hold_index(coefs: &[f64], t_stats: &[f64]) -> usize {
    let mut best_pos: Option<(usize, f64)> = None;
    for (i, (&c, &t)) in coefs.iter().zip(t_stats).enumerate() {
        if c > 0.0 && t.is_finite() {
            match best_pos {
                Some((_, bt)) if bt >= t => {}
                _ => best_pos = Some((i, t)),
            }
        }
    }
    if let Some((i, _)) = best_pos {
        return i;
    }
    let mut best = (0usize, f64::NEG_INFINITY);
    for (i, &t) in t_stats.iter().enumerate() {
        let a = if t.is_finite() { t.abs() } else { 0.0 };
        if a > best.1 {
            best = (i, a);
        }
    }
    best.0
}

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
                "stats.momentum.tsmom: k_values must be an array; got {v}"
            ))
        })?,
    };
    if arr.is_empty() {
        return Err(ScanError::Kernel(
            "stats.momentum.tsmom: k_values must be non-empty".into(),
        ));
    }
    let mut out: Vec<usize> = Vec::with_capacity(arr.len());
    for v in arr {
        let i = v.as_i64().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.momentum.tsmom: k_values entries must be integers; got {v}"
            ))
        })?;
        if i < 1 {
            return Err(ScanError::Kernel(format!(
                "stats.momentum.tsmom: k_values entries must be >= 1; got {i}"
            )));
        }
        let u = usize::try_from(i).map_err(|_| {
            ScanError::Kernel(format!(
                "stats.momentum.tsmom: k_values entry {i} out of usize range"
            ))
        })?;
        out.push(u);
    }
    Ok(out)
}

fn resolve_scaling(req: &ScanRequest) -> Result<bool, ScanError> {
    let raw = req.resolved_params.get("scaling");
    match raw {
        None => Ok(true),
        Some(v) => v.as_bool().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.momentum.tsmom: scaling must be a boolean; got {v}"
            ))
        }),
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "k << 2^52 for any realistic horizon"
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

    /// LCG white-noise returns -> random-walk closes via `exp` accumulation.
    #[allow(clippy::cast_possible_truncation)]
    fn random_walk_closes(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed as u32;
        let mut out = Vec::with_capacity(n);
        let mut price = 1.0_f64;
        out.push(price);
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX) - 0.5) * 0.01;
            price *= eps.exp();
            out.push(price);
        }
        out
    }

    /// AR(1) positive-momentum log-returns -> closes via `exp` accumulation,
    /// so `log_returns(closes)` recovers the AR(1) series.
    #[allow(clippy::cast_possible_truncation)]
    fn ar1_momentum_closes(n: usize, seed: u64, phi: f64) -> Vec<f64> {
        let mut s = seed as u32;
        let mut out = Vec::with_capacity(n);
        let mut price = 1.0_f64;
        out.push(price);
        let mut prev = 0.0_f64;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX) - 0.5) * 0.01;
            let r = phi * prev + eps;
            price *= r.exp();
            out.push(price);
            prev = r;
        }
        out
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

    fn read_f64(arr: &RawArray, idx: usize) -> f64 {
        let off = idx * 8;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&arr.data.0[off..off + 8]);
        f64::from_le_bytes(buf)
    }

    // -----------------------------------------------------------------------

    #[test]
    fn tsmom_id_and_version() {
        assert_eq!(TsmomScan.id(), "stats.momentum.tsmom");
        assert_eq!(TsmomScan.version(), 1);
    }

    #[test]
    fn tsmom_arity_is_single() {
        assert_eq!(TsmomScan.arity(), ScanArity::Single);
    }

    #[test]
    fn tsmom_param_schema() {
        let schema = TsmomScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["scaling"]["default"], true);
        assert_eq!(
            schema["properties"]["k_values"]["default"],
            serde_json::json!([1, 5, 10, 20])
        );
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn tsmom_default_k_values_shapes() {
        let bars = bar_frame_from_closes(random_walk_closes(600, 1));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        TsmomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        for key in [
            "continuation_coefs",
            "hit_rates",
            "k_values",
            "p_values",
            "t_stats",
            "tsmom_means",
            "turnover_per_bar",
        ] {
            assert_eq!(r.effect.extra[key].shape, vec![4], "{key} length");
        }
        // selected_hold_bars is a single-element array.
        assert_eq!(r.effect.extra["selected_hold_bars"].shape, vec![1]);
    }

    #[test]
    fn tsmom_emits_one_result() {
        let bars = bar_frame_from_closes(random_walk_closes(600, 2));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        TsmomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn tsmom_result_envelope_shape() {
        let bars = bar_frame_from_closes(random_walk_closes(600, 3));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [1, 5]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        TsmomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.momentum.tsmom@1");
        assert_eq!(r.effect.metric, "tsmom_continuation");
        assert!(
            r.effect.p_value.is_some(),
            "headline continuation p present"
        );
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec![
                "continuation_coefs",
                "hit_rates",
                "k_values",
                "p_values",
                "selected_hold_bars",
                "t_stats",
                "tsmom_means",
                "turnover_per_bar",
            ]
        );
        let raw = r.raw.as_ref().expect("raw");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
        // effect_size carries the tradeable hit-rate.
        let es = r.effect.effect_size.as_ref().expect("effect_size");
        assert_eq!(es.kind, "hit_rate");
        assert!(es.value >= 0.0 && es.value <= 1.0);
    }

    /// AC2 (momentum half): injected positive serial dependence yields a
    /// significant POSITIVE continuation at the matching short horizon.
    #[test]
    fn tsmom_ar1_significant_positive_continuation() {
        let bars = bar_frame_from_closes(ar1_momentum_closes(3000, 99, 0.5));
        let mut sink = VecSink::new();
        // Disable scaling so the assertion reads the raw-return continuation.
        let req =
            sample_request_with_params(serde_json::json!({"k_values": [1], "scaling": false}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        TsmomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let coef = read_f64(&r.effect.extra["continuation_coefs"], 0);
        let p = read_f64(&r.effect.extra["p_values"], 0);
        assert!(coef > 0.0, "AR(1) continuation = {coef} should be > 0");
        assert!(p < 0.01, "AR(1) p = {p} should be significant");
        // Headline reflects the same (only) horizon.
        assert!(r.effect.value > 0.0);
        assert_eq!(read_f64(&r.effect.extra["selected_hold_bars"], 0), 1.0);
    }

    /// AC2 (random-walk half): a random walk yields ~zero continuation and a
    /// directional hit-rate ≈ 0.5 at the matching horizon. (Robust to the seed
    /// — a `p > 0.05` check would carry an inherent ~5% per-seed false-positive
    /// rate under the null; the near-zero coefficient + 0.5 hit-rate are the
    /// substantive non-significance evidence and sit many σ inside tolerance.)
    #[test]
    fn tsmom_random_walk_near_zero_continuation() {
        let bars = bar_frame_from_closes(random_walk_closes(3000, 7));
        let mut sink = VecSink::new();
        let req =
            sample_request_with_params(serde_json::json!({"k_values": [1], "scaling": false}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        TsmomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let coef = read_f64(&r.effect.extra["continuation_coefs"], 0);
        let hit = read_f64(&r.effect.extra["hit_rates"], 0);
        let p = read_f64(&r.effect.extra["p_values"], 0);
        assert!(coef.abs() < 0.15, "random-walk continuation = {coef} ≈ 0");
        assert!(
            (hit - 0.5).abs() < 0.1,
            "random-walk hit-rate = {hit} ≈ 0.5"
        );
        assert!((0.0..=1.0).contains(&p), "p in [0,1]: {p}");
    }

    #[test]
    fn tsmom_turnover_is_reciprocal_k() {
        let bars = bar_frame_from_closes(random_walk_closes(600, 11));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [2, 10]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        TsmomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!((read_f64(&r.effect.extra["turnover_per_bar"], 0) - 0.5).abs() < 1e-12);
        assert!((read_f64(&r.effect.extra["turnover_per_bar"], 1) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn tsmom_cancellation() {
        let bars = bar_frame_from_closes(random_walk_closes(64, 4));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        TsmomScan.run(&ctx, &req, &mut sink).expect("ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn tsmom_invalid_k_zero() {
        let bars = bar_frame_from_closes(random_walk_closes(200, 5));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [0]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = TsmomScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject k=0");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn tsmom_k_too_large_rejected() {
        // 20 closes -> 19 returns. k=5 -> 19/5 = 3 blocks < 4 -> error.
        let bars = bar_frame_from_closes(random_walk_closes(20, 6));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [5]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = TsmomScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject k too large");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn tsmom_n_too_small_emits_scan_error() {
        let bars = bar_frame_from_closes(vec![1.0, 1.01, 1.02]); // 2 returns < 4.
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = TsmomScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject small n");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "{msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn tsmom_invalid_k_values_non_array() {
        let bars = bar_frame_from_closes(random_walk_closes(200, 7));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": "garbage"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = TsmomScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject non-array");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn tsmom_invalid_scaling_non_bool() {
        let bars = bar_frame_from_closes(random_walk_closes(200, 8));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"scaling": "yes"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = TsmomScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject non-bool scaling");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn tsmom_selected_hold_is_one_of_k() {
        let bars = bar_frame_from_closes(ar1_momentum_closes(3000, 21, 0.4));
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"k_values": [1, 5, 10, 20]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        TsmomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let sel = read_f64(&r.effect.extra["selected_hold_bars"], 0);
        assert!(
            [1.0, 5.0, 10.0, 20.0].contains(&sel),
            "selected hold {sel} must be one of the k grid"
        );
    }

    #[test]
    fn tsmom_select_hold_index_prefers_positive_max_t() {
        // coefs: [+, -, +], t: [1.0, 5.0, 3.0] -> pick index 2 (positive, max t).
        let idx = select_hold_index(&[0.1, -0.2, 0.3], &[1.0, 5.0, 3.0]);
        assert_eq!(idx, 2);
    }

    #[test]
    fn tsmom_select_hold_index_fallback_abs_t() {
        // All negative coefs -> fall back to largest |t| (index 1).
        let idx = select_hold_index(&[-0.1, -0.2, -0.05], &[1.0, -6.0, 2.0]);
        assert_eq!(idx, 1);
    }
}

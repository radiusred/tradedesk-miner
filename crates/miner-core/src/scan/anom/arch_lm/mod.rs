//! `ArchLmScan` — ANOM-08 Engle (1982) ARCH-LM test for conditional
//! heteroskedasticity.
//!
//! Pattern analog: [`crate::scan::anom::adf::AdfScan`] — Pattern A from
//! `04-PATTERNS.md`. Both scans share the "user supplies a lag, kernel fits
//! a runtime-variable-column OLS regression via nalgebra DMatrix" shape.
//!
//! ## Reference
//!
//! `statsmodels.stats.diagnostic.het_arch(returns, nlags=L)` — emits
//! `(lm_stat, lm_pvalue, f_stat, f_pvalue)`. Engle (1982), Econometrica 50.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.heteroskedasticity.arch_lm"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params`: optional `lag: integer default 5` (Engle 1982 standard).
//! - `effect.metric = "arch_lm_statistic"`, `effect.value = LM stat`,
//!   `effect.p_value = chi-squared(df=lag) tail`.
//! - `effect.extra = {f_p_value, f_statistic, lag, p_value}` (alphabetical
//!   `BTreeMap` order).
//! - `raw.series = {returns, timestamps_ms}` (log_returns input; timestamps
//!   from bars[1..]).
//!
//! ## Input series
//!
//! ARCH-LM operates on log_returns (not levels). The kernel internally mean-
//! adjusts the returns to construct the squared residuals series for the
//! AR(L) regression.
//!
//! ## Registration
//!
//! Appended inside `crate::scan::anom::register_anom_scans` (Pattern E —
//! `crates/miner-core/src/scan/registry.rs` is NOT modified).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;
use serde_json::Value as JsonValue;

use crate::findings::{DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// ANOM-08 — Engle (1982) ARCH-LM test for conditional heteroskedasticity.
pub struct ArchLmScan;

const SCAN_ID: &str = "stats.heteroskedasticity.arch_lm";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "arch_lm_statistic";

const DEFAULT_LAG: usize = 5; // Engle 1982 standard.

impl Scan for ArchLmScan {
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
                "lag": {
                    "type": "integer",
                    "minimum": 1,
                    "default": DEFAULT_LAG,
                    "description": "AR lag count for the squared-residuals regression (Engle 1982 default 5). Must be >= 1 and <= n/3 where n is the number of returns."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["f_p_value", "f_statistic", "lag", "p_value"],
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

        // Step 2 — N guard on raw closes. log_returns needs >= 2 closes.
        let n_closes = ctx.bars.close.len();
        if n_closes < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.heteroskedasticity.arch_lm: need n >= 2 closes for log_returns; got n={n_closes} (InsufficientData)"
            )));
        }

        // Step 3 — compute log returns.
        let returns = log_returns(&ctx.bars.close);
        let n = returns.len();

        // Step 4 — resolve and validate lag param. Reject lag = 0 or lag > n/3
        // per T-04-06-01 (DOS mitigation: insufficient observations for the
        // regression).
        let lag = resolve_lag(req, n)?;

        // Step 5 — kernel call.
        let result = kernel::arch_lm_test(&returns, lag).map_err(ScanError::Kernel)?;

        // Step 6 — build raw.series.
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
        )]
        let ts_ms: Vec<f64> = ctx
            .bars
            .ts_open_utc
            .iter()
            .skip(1) // returns are aligned with bars[1..].
            .map(|t| t.timestamp_millis() as f64)
            .collect();

        // Step 7 — envelope construction.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "f_p_value".into(),
            f64_slice_to_raw_array(&[result.f_pvalue]),
        );
        extra.insert(
            "f_statistic".into(),
            f64_slice_to_raw_array(&[result.f_stat]),
        );
        extra.insert(
            "lag".into(),
            f64_slice_to_raw_array(&[index_to_f64(result.lag)]),
        );
        extra.insert(
            "p_value".into(),
            f64_slice_to_raw_array(&[result.lm_pvalue]),
        );

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: result.lm,
            p_value: Some(result.lm_pvalue),
            #[allow(
                clippy::cast_possible_truncation,
                reason = "n <= u64 on all supported targets"
            )]
            n: Some(n as u64),
            ci95: None,
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
        };

        sink.write_envelope(&Finding::Result(finding))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve and validate the `lag` parameter. T-04-06-01 mitigation: reject
/// `lag = 0` and `lag > n/3` (insufficient observations to fit the AR(L)
/// regression on squared residuals).
fn resolve_lag(req: &ScanRequest, n: usize) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("lag");
    let lag = match raw {
        Some(v) => {
            let i = v.as_i64().ok_or_else(|| {
                ScanError::Kernel(format!(
                    "stats.heteroskedasticity.arch_lm: lag must be an integer; got {v}"
                ))
            })?;
            if i < 1 {
                return Err(ScanError::Kernel(format!(
                    "stats.heteroskedasticity.arch_lm: lag must be >= 1; got {i}"
                )));
            }
            usize::try_from(i).map_err(|_| {
                ScanError::Kernel(format!(
                    "stats.heteroskedasticity.arch_lm: lag={i} out of usize range"
                ))
            })?
        }
        None => DEFAULT_LAG,
    };
    // T-04-06-01: lag > n/3 leaves insufficient observations for the regression.
    if lag == 0 {
        return Err(ScanError::Kernel(format!(
            "stats.heteroskedasticity.arch_lm: lag must be >= 1; got {lag}"
        )));
    }
    if lag > n / 3 {
        return Err(ScanError::Kernel(format!(
            "stats.heteroskedasticity.arch_lm: lag={lag} must be <= n/3 (n={n}); insufficient observations"
        )));
    }
    Ok(lag)
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
    fn arch_lm_id_and_version() {
        assert_eq!(ArchLmScan.id(), "stats.heteroskedasticity.arch_lm");
        assert_eq!(ArchLmScan.version(), 1);
    }

    #[test]
    fn arch_lm_arity_is_single() {
        assert_eq!(ArchLmScan.arity(), ScanArity::Single);
    }

    #[test]
    fn arch_lm_param_schema() {
        let schema = ArchLmScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["lag"]["type"], "integer");
        assert_eq!(schema["properties"]["lag"]["default"], 5);
        assert_eq!(schema["properties"]["lag"]["minimum"], 1);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn arch_lm_emits_one_result() {
        let bars = lcg_bar_frame_seeded(80, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ArchLmScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn arch_lm_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(80, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lag": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ArchLmScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.heteroskedasticity.arch_lm@1");
        assert_eq!(r.effect.metric, "arch_lm_statistic");
        assert!(r.effect.p_value.is_some());
        // 80 closes -> 79 returns.
        assert_eq!(r.effect.n, Some(79));
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec!["f_p_value", "f_statistic", "lag", "p_value"]
        );
        let raw = r.raw.as_ref().expect("raw");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    #[test]
    fn arch_lm_cancellation() {
        let bars = lcg_bar_frame_seeded(80, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        ArchLmScan.run(&ctx, &req, &mut sink).expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    /// T-04-06-01 — lag = 0 must be rejected.
    #[test]
    fn arch_lm_invalid_lag_zero() {
        let bars = lcg_bar_frame_seeded(60, 4);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lag": 0}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = ArchLmScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject lag=0");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    /// T-04-06-01 — lag > n/3 must be rejected.
    #[test]
    fn arch_lm_invalid_lag_too_large() {
        let bars = lcg_bar_frame_seeded(30, 5);
        let mut sink = VecSink::new();
        // n_returns = 29; n/3 = 9. lag=20 is too large.
        let req = sample_request_with_params(serde_json::json!({"lag": 20}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = ArchLmScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject lag > n/3");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn arch_lm_n_zero_emits_scan_error() {
        let bars = bar_frame_from_closes(Vec::new());
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = ArchLmScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject n=0");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    /// Homoskedastic-shaped input (white-noise returns): LM stat should not
    /// strongly reject the null at lag=5. Sanity bound, not a tight pin.
    #[test]
    fn arch_lm_homoskedastic_input_below_critical() {
        // 200 closes — random walk-like, uniform increments => homoskedastic returns.
        let n = 200;
        let mut closes = vec![1.0_f64];
        let mut s: u32 = 7;
        for _ in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            let prev = *closes.last().unwrap();
            closes.push(prev * (1.0 + 0.001 * eps));
        }
        let bars = bar_frame_from_closes(closes);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lag": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ArchLmScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // chi²(5) 5% critical = 11.07. For homoskedastic input, LM should
        // typically be well below this (we use a generous bound to keep the
        // test stable across LCG seeds).
        assert!(
            r.effect.value < 20.0,
            "homoskedastic LM = {} should be < 20.0",
            r.effect.value
        );
    }

    /// GARCH(1,1)-like volatility clustering: LM should reject the homo-
    /// skedasticity null (LM > chi²(5) 5% critical = 11.07). Uses regime-
    /// switching variance on top of strong-ARCH persistence to ensure
    /// reliable rejection across LCG seeds.
    #[test]
    fn arch_lm_garch_like_input_rejects_null() {
        let n = 1000;
        let mut closes = vec![1.0_f64];
        let mut s: u32 = 31;
        let mut prev_sq = 0.0001_f64;
        for i in 1..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let eps = (f64::from(s) / f64::from(u32::MAX)) - 0.5;
            // Strong ARCH + regime-switching variance for reliable LM > 11.07.
            let regime = if (i / 50) % 2 == 0 { 1.0 } else { 8.0 };
            let vol = (regime * 0.0001 + 0.99 * prev_sq).sqrt();
            let r = vol * eps;
            let prev = *closes.last().unwrap();
            closes.push(prev * (1.0 + r));
            prev_sq = r * r;
        }
        let bars = bar_frame_from_closes(closes);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lag": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ArchLmScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!(
            r.effect.value > 11.07,
            "GARCH-like LM = {} should exceed chi²(5) 5% crit = 11.07",
            r.effect.value
        );
        let p = r.effect.p_value.expect("p_value present");
        assert!(p < 0.05, "GARCH-like p = {p} should be < 0.05");
    }
}

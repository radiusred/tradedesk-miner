//! `SummaryWelfordScan` — ANOM-02 single-leg Welford running-moments summary
//! statistics (mean / std / skew / excess-kurtosis / IQR / min / max).
//!
//! Pattern analog: `crate::scan::ljung_box::LjungBoxScan` (Phase 3 gold-
//! standard / `04-PATTERNS.md` Pattern A).
//!
//! ## D4-02 surface
//!
//! - `id = "stats.summary.welford"`, `version = 1`, `arity = ScanArity::Single`.
//! - `params.series ∈ {"close", "log_returns"}` (default `"log_returns"`).
//! - `effect.metric = "summary_welford_mean"`, `effect.value = arithmetic
//!   mean`, `effect.extra = {excess_kurtosis, iqr, max, min, n, skew, std}`.
//! - `raw.series = {returns, timestamps_ms}` (or `{closes, timestamps_ms}`
//!   under `series=close`).
//!
//! ## Registration
//!
//! Appended inside `crate::scan::anom::register_anom_scans` (Pattern E —
//! `crates/miner-core/src/scan/registry.rs` is NOT modified in this task).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// ANOM-02 — Welford running-moments + IQR summary scan.
pub struct SummaryWelfordScan;

const SCAN_ID: &str = "stats.summary.welford";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "summary_welford_mean";

/// Which input series the scan summarises. Wire-form maps to
/// `params.series ∈ {"close","log_returns"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummarySeries {
    Close,
    LogReturns,
}

impl SummarySeries {
    fn as_str(self) -> &'static str {
        match self {
            SummarySeries::Close => "close",
            SummarySeries::LogReturns => "log_returns",
        }
    }
}

impl Scan for SummaryWelfordScan {
    fn id(&self) -> &'static str {
        SCAN_ID
    }

    fn version(&self) -> u32 {
        SCAN_VERSION
    }

    fn arity(&self) -> ScanArity {
        ScanArity::Single
    }

    fn param_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "series": {
                    "type": "string",
                    "enum": ["close", "log_returns"],
                    "default": "log_returns",
                    "description": "Which input series to summarise — bar closes or log returns."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        // BTreeMap iteration order is alphabetical, so the keys list reflects
        // the wire-form order. NOTE: `raw_series_keys` is the static-key
        // contract for the catalogue; the per-invocation raw.series keys
        // depend on `series` param ("returns" vs "closes"). We surface the
        // log_returns default in the catalogue; the per-finding keys are
        // visible in the emitted envelope.
        ScanFindingShape {
            effect_extra_keys: &["excess_kurtosis", "iqr", "max", "min", "n", "skew", "std"],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Scan::run is the single linear dispatch + envelope build path; splitting into 4-5 sub-functions per Pattern A scan obscures the 7-step structure documented in LjungBoxScan"
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

        // Step 2 — resolve series param.
        let series = resolve_series(req)?;

        // Step 3 — N=0 guard.
        let n_closes = ctx.bars.close.len();
        if n_closes == 0 {
            return Err(ScanError::Kernel(
                "stats.summary.welford: empty close series (InsufficientData)".to_string(),
            ));
        }

        // Step 4 — compute the target series.
        let (values, series_label, ts_label, ts_ms): (Vec<f64>, &'static str, &'static str, Vec<f64>) =
            match series {
                SummarySeries::Close => {
                    let ts: Vec<f64> = ctx
                        .bars
                        .ts_open_utc
                        .iter()
                        .map(|t| ts_to_f64(t.timestamp_millis()))
                        .collect();
                    (ctx.bars.close.clone(), "closes", "timestamps_ms", ts)
                }
                SummarySeries::LogReturns => {
                    if n_closes < 2 {
                        return Err(ScanError::Kernel(format!(
                            "stats.summary.welford: need >= 2 closes for log_returns; got n={n_closes} (InsufficientData)"
                        )));
                    }
                    let returns = log_returns(&ctx.bars.close);
                    let ts: Vec<f64> = ctx
                        .bars
                        .ts_open_utc
                        .iter()
                        .skip(1)
                        .map(|t| ts_to_f64(t.timestamp_millis()))
                        .collect();
                    (returns, "returns", "timestamps_ms", ts)
                }
            };

        let n = values.len();
        if n == 0 {
            return Err(ScanError::Kernel(format!(
                "stats.summary.welford: series={} produced n=0 (InsufficientData)",
                series.as_str()
            )));
        }

        // Step 5 — Welford stats + IQR + min/max. n=1 is a "trivial result"
        // path per the plan's behaviour test: emit a Result with mean=value,
        // std=skew=kurt=0.0, iqr=0.0, min==max==value.
        let (mean, std, skew, kurt, iqr_val, lo, hi) = if n == 1 {
            (values[0], 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, values[0], values[0])
        } else {
            let stats = kernel::welford_pass(&values);
            let q = kernel::iqr(&values);
            let (lo, hi) = kernel::min_max(&values);
            (stats.mean, stats.std, stats.skew, stats.excess_kurtosis, q, lo, hi)
        };

        // Step 6 — envelope.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("excess_kurtosis".into(), f64_slice_to_raw_array(&[kurt]));
        extra.insert("iqr".into(), f64_slice_to_raw_array(&[iqr_val]));
        extra.insert("max".into(), f64_slice_to_raw_array(&[hi]));
        extra.insert("min".into(), f64_slice_to_raw_array(&[lo]));
        extra.insert("n".into(), f64_slice_to_raw_array(&[n_to_f64(n)]));
        extra.insert("skew".into(), f64_slice_to_raw_array(&[skew]));
        extra.insert("std".into(), f64_slice_to_raw_array(&[std]));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: mean,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize <= u64 on all supported targets"
            )]
            n: Some(n as u64),
            ci95: None,
            extra,
        };

        let mut series_map: BTreeMap<String, RawArray> = BTreeMap::new();
        series_map.insert(series_label.into(), f64_slice_to_raw_array(&values));
        series_map.insert(ts_label.into(), f64_slice_to_raw_array(&ts_ms));
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

fn resolve_series(req: &ScanRequest) -> Result<SummarySeries, ScanError> {
    let raw = req.resolved_params.get("series");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.summary.welford: series must be close|log_returns; got {v}"
            ))
        })?,
        None => "log_returns",
    };
    match label {
        "close" => Ok(SummarySeries::Close),
        "log_returns" => Ok(SummarySeries::LogReturns),
        other => Err(ScanError::Kernel(format!(
            "stats.summary.welford: series must be close|log_returns; got {other:?}"
        ))),
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "n is the bar/return count; bar counts << 2^52"
)]
#[inline]
fn n_to_f64(n: usize) -> f64 {
    n as f64
}

#[allow(
    clippy::cast_precision_loss,
    reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
)]
#[inline]
fn ts_to_f64(ms: i64) -> f64 {
    ms as f64
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
                let i_i64 = i64::try_from(i).expect("test n fits in i64");
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
    fn summary_welford_id_and_version() {
        let s = SummaryWelfordScan;
        assert_eq!(s.id(), "stats.summary.welford");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn summary_welford_arity_is_single() {
        let s = SummaryWelfordScan;
        assert_eq!(s.arity(), ScanArity::Single);
        assert_eq!(s.arity().expected_len(), 1);
    }

    #[test]
    fn summary_welford_param_schema() {
        let s = SummaryWelfordScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        let series = &schema["properties"]["series"];
        assert_eq!(series["type"], "string");
        assert_eq!(series["default"], "log_returns");
        let enum_arr = series["enum"].as_array().expect("enum");
        let labels: Vec<&str> = enum_arr.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(labels, vec!["close", "log_returns"]);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn summary_welford_emits_one_result() {
        let bars = lcg_bar_frame_seeded(64, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SummaryWelfordScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn summary_welford_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(64, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "log_returns"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SummaryWelfordScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.summary.welford@1");
        assert_eq!(r.effect.metric, "summary_welford_mean");
        assert_eq!(r.effect.n, Some(63));
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec!["excess_kurtosis", "iqr", "max", "min", "n", "skew", "std"]
        );
        let raw = r.raw.as_ref().expect("raw present");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    /// Hand-derived skew within 1e-12.
    #[test]
    fn summary_welford_skew_matches_hand_derived() {
        let values = [1.0_f64, 2.0, 3.0, 4.0, 10.0];
        let s = kernel::welford_pass(&values);
        // scipy.stats.skew([1,2,3,4,10], bias=False) per scipy 1.14.1.
        let expected = 1.697_056_274_847_714_3_f64;
        assert!(
            (s.skew - expected).abs() < 1e-12,
            "skew={} expected {}",
            s.skew,
            expected
        );
    }

    /// Hand-derived excess kurtosis within 1e-12.
    #[test]
    fn summary_welford_excess_kurtosis_matches_hand_derived() {
        let values: Vec<f64> = (1..=8).map(|i| f64::from(i)).collect();
        let s = kernel::welford_pass(&values);
        let expected = -1.2_f64;
        assert!(
            (s.excess_kurtosis - expected).abs() < 1e-12,
            "kurt={} expected {}",
            s.excess_kurtosis,
            expected
        );
    }

    /// IQR via linear interpolation on odd-length input.
    #[test]
    fn summary_welford_iqr_matches_hand_derived() {
        // [1,2,3,4,5]: P75 = 4.0, P25 = 2.0, IQR = 2.0.
        let q = kernel::iqr(&[1.0_f64, 2.0, 3.0, 4.0, 5.0]);
        assert!((q - 2.0).abs() < 1e-12, "iqr={q}");
    }

    #[test]
    fn summary_welford_cancellation() {
        let bars = lcg_bar_frame_seeded(64, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        SummaryWelfordScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn summary_welford_n_zero_emits_scan_error() {
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: Vec::new(),
            ts_close_utc: Vec::new(),
            open: Vec::new(),
            high: Vec::new(),
            low: Vec::new(),
            close: Vec::new(),
            tick_volume: Vec::new(),
        };
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = SummaryWelfordScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("InsufficientData"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    /// N=1 emits a "trivial" Result (mean=value, all moments = 0). This is
    /// the explicit kernel decision pinned via test per the plan: don't
    /// raise an error for n=1 under series=close; instead emit a degenerate
    /// finding so downstream consumers can decide.
    #[test]
    fn summary_welford_n_one_emits_trivial_result() {
        let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: vec![t0],
            ts_close_utc: vec![t0 + Duration::minutes(15)],
            open: vec![1.0],
            high: vec![1.001],
            low: vec![0.999],
            close: vec![1.5],
            tick_volume: vec![1.0],
        };
        let mut sink = VecSink::new();
        // series=close lets n=1 emit; series=log_returns would error.
        let req = sample_request_with_params(serde_json::json!({"series": "close"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SummaryWelfordScan
            .run(&ctx, &req, &mut sink)
            .expect("trivial result ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.effect.n, Some(1));
        assert_eq!(r.effect.value, 1.5);
        // std / skew / kurt are zero for n=1.
        let std_bytes = &r.effect.extra["std"].data.0;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&std_bytes[0..8]);
        assert_eq!(f64::from_le_bytes(buf), 0.0_f64);
    }
}

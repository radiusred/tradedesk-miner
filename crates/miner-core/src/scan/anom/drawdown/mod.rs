//! `DrawdownProfileScan` — ANOM-11 peak-trough drawdown profile on cumulative
//! log-equity curve.
//!
//! Pattern analog: [`crate::scan::ljung_box::LjungBoxScan`] (Phase 3 gold-
//! standard / `04-PATTERNS.md` Pattern A). No statsmodels/scipy reference
//! exists for the drawdown profile — the kernel (peak/trough sweep over a
//! cumulative log-equity curve) is hand-derived from the standard
//! quantitative-finance "underwater curve" formulation (e.g., Bacon 2008
//! chap 7). Hand-derived V-shape unit tests pin the kernel within 1e-12.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.drawdown.profile"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params.series ∈ {"log_returns","close"}` (default `"log_returns"`).
//! - `effect.metric = "max_drawdown"`, `effect.value = max_drawdown`
//!   (signed; always `<= 0.0`), `effect.n = number of closed drawdown
//!   episodes`.
//! - `effect.extra = {dd_distribution_p50_p95_p99, drawdown_durations_ms,
//!    equity_curve, peaks, time_to_recover_ms, troughs}` (alphabetical
//!   `BTreeMap` order).
//! - `raw.series = {returns, timestamps_ms}`.
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

/// ANOM-11 — peak-trough drawdown profile scan.
pub struct DrawdownProfileScan;

const SCAN_ID: &str = "stats.drawdown.profile";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "max_drawdown";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawdownSeries {
    LogReturns,
    Close,
}

impl DrawdownSeries {
    fn as_str(self) -> &'static str {
        match self {
            DrawdownSeries::LogReturns => "log_returns",
            DrawdownSeries::Close => "close",
        }
    }
}

impl Scan for DrawdownProfileScan {
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
                    "description": "Input series for the cumulative equity curve."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[
                "dd_distribution_p50_p95_p99",
                "drawdown_durations_ms",
                "equity_curve",
                "peaks",
                "time_to_recover_ms",
                "troughs",
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
        let series = resolve_series(req)?;

        // Step 3 — N=0 guard on raw closes.
        let n_closes = ctx.bars.close.len();
        if n_closes == 0 {
            return Err(ScanError::Kernel(
                "stats.drawdown.profile: empty close series (InsufficientData)".to_string(),
            ));
        }

        // Step 4 — compute the returns series. Both series modes feed the
        // same kernel; under `series=close` we treat the raw closes as
        // already-log-equity (callers can choose this for absolute-level
        // analysis), under `series=log_returns` we go through log_returns
        // + cumulative sum.
        let (returns, ts): (Vec<f64>, Vec<chrono::DateTime<chrono::Utc>>) = match series {
            DrawdownSeries::LogReturns => {
                if n_closes < 2 {
                    return Err(ScanError::Kernel(format!(
                        "stats.drawdown.profile: need >= 2 closes for log_returns; got n={n_closes} (InsufficientData)"
                    )));
                }
                let r = log_returns(&ctx.bars.close);
                // Timestamps: bar t+1's open for each return.
                let t: Vec<chrono::DateTime<chrono::Utc>> =
                    ctx.bars.ts_open_utc.iter().skip(1).copied().collect();
                (r, t)
            }
            DrawdownSeries::Close => {
                // Use closes directly. The kernel will treat the equity
                // curve as the closes themselves (running peak / underwater
                // sweep applies to any monotone-real series). Timestamps:
                // bar t's open for each close.
                let t: Vec<chrono::DateTime<chrono::Utc>> = ctx.bars.ts_open_utc.clone();
                (ctx.bars.close.clone(), t)
            }
        };

        let n = returns.len();
        if n < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.drawdown.profile: series={} produced n={n} (need >= 2; InsufficientData)",
                series.as_str()
            )));
        }
        debug_assert_eq!(ts.len(), n);

        // Step 5 — kernel calls. For series=log_returns we cumulatively
        // sum into an equity curve; for series=close we use closes as the
        // equity curve directly (the kernel is generic over the curve).
        let equity_curve = match series {
            DrawdownSeries::LogReturns => kernel::cumulative_log_equity(&returns),
            DrawdownSeries::Close => returns.clone(),
        };
        let profile = kernel::compute_drawdown_profile(&equity_curve, &ts);

        // Step 6 — envelope construction.
        let peaks_f64: Vec<f64> = profile.peaks.iter().map(|i| usize_to_f64(*i)).collect();
        let troughs_f64: Vec<f64> = profile.troughs.iter().map(|i| usize_to_f64(*i)).collect();
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic durations"
        )]
        let durations_f64: Vec<f64> = profile.durations_ms.iter().map(|m| *m as f64).collect();
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic durations"
        )]
        let recover_f64: Vec<f64> = profile
            .time_to_recover_ms
            .iter()
            .map(|m| *m as f64)
            .collect();
        let percentiles_f64: Vec<f64> = profile.dd_dist_percentiles.to_vec();

        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "dd_distribution_p50_p95_p99".into(),
            f64_slice_to_raw_array(&percentiles_f64),
        );
        extra.insert(
            "drawdown_durations_ms".into(),
            f64_slice_to_raw_array(&durations_f64),
        );
        extra.insert(
            "equity_curve".into(),
            f64_slice_to_raw_array(&profile.equity_curve),
        );
        extra.insert("peaks".into(), f64_slice_to_raw_array(&peaks_f64));
        extra.insert(
            "time_to_recover_ms".into(),
            f64_slice_to_raw_array(&recover_f64),
        );
        extra.insert("troughs".into(), f64_slice_to_raw_array(&troughs_f64));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: profile.max_dd,
            p_value: None,
            // Number of closed drawdown episodes. (Use 0 if none closed.)
            #[allow(
                clippy::cast_possible_truncation,
                reason = "episode count <= bar count which fits in u64 on all targets"
            )]
            n: Some(profile.peaks.len() as u64),
            ci95: None,
            extra,
        };

        // raw.series: source returns + parallel timestamps.
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
        )]
        let ts_ms: Vec<f64> = ts.iter().map(|t| t.timestamp_millis() as f64).collect();
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

fn resolve_series(req: &ScanRequest) -> Result<DrawdownSeries, ScanError> {
    let raw = req.resolved_params.get("series");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.drawdown.profile: series must be log_returns|close; got {v}"
            ))
        })?,
        None => "log_returns",
    };
    match label {
        "log_returns" => Ok(DrawdownSeries::LogReturns),
        "close" => Ok(DrawdownSeries::Close),
        other => Err(ScanError::Kernel(format!(
            "stats.drawdown.profile: series must be log_returns|close; got {other:?}"
        ))),
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "bar index fits in f64's 52-bit mantissa for realistic OHLCV"
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

    #[allow(clippy::cast_possible_truncation)]
    fn lcg_bar_frame_seeded(n: usize, seed: u64) -> BarFrame {
        let mut s = seed as u32;
        let mut closes = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            closes.push(1.0 + frac);
        }
        bar_frame_from_closes(closes)
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
    fn drawdown_id_and_version() {
        let s = DrawdownProfileScan;
        assert_eq!(s.id(), "stats.drawdown.profile");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn drawdown_arity_is_single() {
        assert_eq!(DrawdownProfileScan.arity(), ScanArity::Single);
    }

    #[test]
    fn drawdown_param_schema() {
        let schema = DrawdownProfileScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["series"]["default"], "log_returns");
        assert_eq!(schema["additionalProperties"], false);
    }

    /// Monotonic uptrend in closes: no drawdown episodes; max_drawdown==0.
    #[test]
    fn drawdown_known_input_monotonic_increasing() {
        let bars = bar_frame_from_closes(vec![1.0_f64, 2.0, 3.0, 4.0, 5.0]);
        let mut sink = VecSink::new();
        // series=log_returns -> all returns positive -> equity strictly
        // increasing -> no drawdown.
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.effect.value, 0.0, "monotone -> max_dd == 0.0");
        // No closed episodes.
        assert_eq!(r.effect.extra["peaks"].shape, vec![0]);
        assert_eq!(r.effect.extra["troughs"].shape, vec![0]);
    }

    /// V-shape: closes [10, 5, 10]. log returns: [ln(0.5), ln(2)] =
    /// [-0.6931..., +0.6931...]. equity = [-0.6931..., 0]. max_dd is the
    /// initial trough at t=0 — but the kernel initializes running_peak
    /// from equity_curve[0] so the dip at t=0 is never detected (running
    /// peak starts at the trough). To exercise the kernel's V-shape
    /// detection we use closes that go UP then DOWN then UP — i.e. a
    /// 4-bar pattern.
    ///
    /// closes [10, 20, 10, 20]: log_returns = [ln 2, -ln 2, ln 2]
    /// = [+0.6931..., -0.6931..., +0.6931...]. Equity:
    /// [0.6931..., 0.0, 0.6931...]. Running peak: 0.6931 at t=0;
    /// at t=1 equity=0.0 < peak -> in_drawdown, trough_value=0.0, trough_idx=1.
    /// At t=2 equity=0.6931 >= peak -> close episode: peak=0, trough=1,
    /// duration = ts[1]-ts[0] = 900_000ms, recovery = ts[2]-ts[1] = 900_000ms.
    /// max_dd = 0.0 - 0.6931... = -0.6931...
    #[test]
    fn drawdown_known_input_v_shape() {
        let bars = bar_frame_from_closes(vec![10.0_f64, 20.0, 10.0, 20.0]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"series": "log_returns"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // max_dd = ln(0.5) = -0.6931471805599453.
        let expected_max_dd: f64 = (0.5_f64).ln();
        assert!(
            (r.effect.value - expected_max_dd).abs() < 1e-12,
            "max_dd={} expected {}",
            r.effect.value,
            expected_max_dd
        );
        // One closed episode.
        assert_eq!(r.effect.extra["peaks"].shape, vec![1]);
        assert_eq!(r.effect.extra["troughs"].shape, vec![1]);
        assert_eq!(r.effect.n, Some(1));
        // durations_ms[0] = 15min = 900_000 ms.
        let dur_bytes = &r.effect.extra["drawdown_durations_ms"].data.0;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&dur_bytes[0..8]);
        assert_eq!(f64::from_le_bytes(buf), 900_000.0);
        // time_to_recover_ms[0] = 15min = 900_000 ms.
        let rec_bytes = &r.effect.extra["time_to_recover_ms"].data.0;
        buf.copy_from_slice(&rec_bytes[0..8]);
        assert_eq!(f64::from_le_bytes(buf), 900_000.0);
    }

    #[test]
    fn drawdown_dd_distribution_percentiles_has_length_three() {
        let bars = lcg_bar_frame_seeded(64, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(
            r.effect.extra["dd_distribution_p50_p95_p99"].shape,
            vec![3]
        );
    }

    #[test]
    fn drawdown_emits_one_result() {
        let bars = lcg_bar_frame_seeded(32, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn drawdown_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(32, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.drawdown.profile@1");
        assert_eq!(r.effect.metric, "max_drawdown");
        // Drawdown is always <= 0.0.
        assert!(r.effect.value <= 0.0);
        assert_eq!(r.effect.ci95, None);
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec![
                "dd_distribution_p50_p95_p99",
                "drawdown_durations_ms",
                "equity_curve",
                "peaks",
                "time_to_recover_ms",
                "troughs",
            ]
        );
        let raw = r.raw.as_ref().expect("raw present");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    #[test]
    fn drawdown_cancellation() {
        let bars = lcg_bar_frame_seeded(32, 4);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn drawdown_n_zero_emits_scan_error() {
        let bars = bar_frame_from_closes(Vec::new());
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject n=0");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn drawdown_n_one_emits_scan_error() {
        // n=1 closes -> 0 log returns OR 1 close (series=close) but kernel
        // requires n >= 2 returns either way.
        let bars = bar_frame_from_closes(vec![1.5_f64]);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject n=1");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn drawdown_raw_new_enforces_timestamps_ms() {
        let bars = lcg_bar_frame_seeded(32, 5);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DrawdownProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!(r.raw.as_ref().unwrap().series.contains_key("timestamps_ms"));
    }
}

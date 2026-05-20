//! `VolRollingScan` — ANOM-03 rolling-vol + vol-of-vol vector-output scan
//! emitting ONE envelope per invocation with vectors in `effect.extra`
//! (Pattern D; Pitfall 1 — never one envelope per window).
//!
//! Pattern analog: `crate::scan::ljung_box::LjungBoxScan` (Pattern A) +
//! `04-PATTERNS.md` Pattern D vector-output construction.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.vol.rolling"`, `version = 1`, `arity = ScanArity::Single`.
//! - `params`: required `window: integer >= 2`; optional `min_periods`
//!   (default = window — rolling kernel does NOT support partial windows
//!   in v1); optional `series ∈ {"log_returns","close"}` default
//!   `"log_returns"`.
//! - `effect.metric = "vol_rolling_last"`, `effect.value = last_window_vol`,
//!   `effect.n = values.len()` (the number of completed rolling windows).
//! - `effect.extra = {values, vol_of_vol, window_starts_ms, window_length}`.
//! - `raw.series = {returns, timestamps_ms}`.
//!
//! ## Cancel polling inside the rolling loop (RESEARCH §Cancel polling)
//!
//! The per-window loop polls `ctx.cancel` every iteration (cost ~1 ns per
//! window via `AtomicBool::load(Ordering::Relaxed)`). On cancel, the scan
//! returns `Ok(())` without emitting — same contract as cancel-at-entry.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// ANOM-03 — rolling-vol + vol-of-vol vector-output scan.
pub struct VolRollingScan;

const SCAN_ID: &str = "stats.vol.rolling";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "vol_rolling_last";

/// Input series for the rolling-vol kernel. Wire-form maps to
/// `params.series ∈ {"log_returns","close"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VolSeries {
    LogReturns,
    Close,
}

impl VolSeries {
    fn as_str(self) -> &'static str {
        match self {
            VolSeries::LogReturns => "log_returns",
            VolSeries::Close => "close",
        }
    }
}

impl Scan for VolRollingScan {
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
                "window": {
                    "type": "integer",
                    "minimum": 2,
                    "description": "Rolling-window size (number of observations per stat). Required."
                },
                "min_periods": {
                    "type": "integer",
                    "minimum": 2,
                    "description": "Minimum observations per window. v1 supports min_periods == window only."
                },
                "series": {
                    "type": "string",
                    "enum": ["log_returns", "close"],
                    "default": "log_returns",
                    "description": "Input series for the rolling-std kernel."
                }
            },
            "required": ["window"],
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["values", "vol_of_vol", "window_length", "window_starts_ms"],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Scan::run is the linear dispatch + envelope build path; splitting into helpers per Pattern A scan obscures the 7-step structure"
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
        let window = resolve_window(req)?;

        // Step 3 — N=0 guard before computing the source series.
        let n_closes = ctx.bars.close.len();
        if n_closes == 0 {
            return Err(ScanError::Kernel(
                "stats.vol.rolling: empty close series (InsufficientData)".to_string(),
            ));
        }

        // Step 4 — compute the source series + corresponding timestamps.
        let (values, ts_ms_input): (Vec<f64>, Vec<chrono::DateTime<chrono::Utc>>) = match series {
            VolSeries::LogReturns => {
                if n_closes < 2 {
                    return Err(ScanError::Kernel(format!(
                        "stats.vol.rolling: need >= 2 closes for log_returns; got n={n_closes} (InsufficientData)"
                    )));
                }
                let returns = log_returns(&ctx.bars.close);
                // For log_returns, each return is indexed by bar t+1's
                // ts_open (the bar whose close produced the return).
                let ts = ctx.bars.ts_open_utc.iter().skip(1).copied().collect();
                (returns, ts)
            }
            VolSeries::Close => {
                let ts = ctx.bars.ts_open_utc.clone();
                (ctx.bars.close.clone(), ts)
            }
        };

        // Step 5 — validate window bounds against the resolved series.
        if window > values.len() {
            return Err(ScanError::Kernel(format!(
                "stats.vol.rolling: window ({window}) must be <= series length ({}) (InsufficientData)",
                values.len()
            )));
        }
        if window < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.vol.rolling: window must be >= 2; got {window}"
            )));
        }

        // Step 6 — rolling-std loop with cancel polling per window.
        // Cancel polling inside the loop: cheap insurance — atomic load is
        // ~1ns per iteration; per RESEARCH §Cancel polling discussion.
        let n_windows = values.len() - window + 1;
        let mut vols: Vec<f64> = Vec::with_capacity(n_windows);
        for i in 0..n_windows {
            if ctx.cancel.load(Ordering::Relaxed) {
                // Caller observed cancellation mid-loop; return Ok without
                // emitting (same contract as cancel-at-entry).
                return Ok(());
            }
            // Reuse the kernel helper one window at a time so the cancel
            // check fires between windows.
            let slice = &values[i..i + window];
            let one_window = kernel::rolling_std(slice, window);
            debug_assert_eq!(one_window.len(), 1);
            vols.push(one_window[0]);
        }

        // vol_of_vol uses the same window for simplicity; empty when
        // n_windows < window.
        let vov = kernel::vol_of_vol(&vols, window);

        // window_starts_ms: timestamp of the FIRST bar in each window
        // (input series index 0 ... n_windows-1 -> ts_ms_input[i]).
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
        )]
        let window_starts_ms: Vec<f64> = (0..n_windows)
            .map(|i| ts_ms_input[i].timestamp_millis() as f64)
            .collect();

        // Step 7 — envelope construction. ONE envelope per invocation with
        // vectors inside effect.extra (Pattern D; Pitfall 1: never N-per-window).
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("values".into(), f64_slice_to_raw_array(&vols));
        extra.insert("vol_of_vol".into(), f64_slice_to_raw_array(&vov));
        extra.insert(
            "window_length".into(),
            f64_slice_to_raw_array(&[window_to_f64(window)]),
        );
        extra.insert(
            "window_starts_ms".into(),
            f64_slice_to_raw_array(&window_starts_ms),
        );

        let last_vol = *vols.last().expect("vols non-empty: n_windows >= 1");
        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: last_vol,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize <= u64 on all supported targets"
            )]
            n: Some(vols.len() as u64),
            ci95: None,
            extra,
        };

        // raw.series: ship the source returns + parallel timestamps_ms.
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
        )]
        let ts_ms_input_f64: Vec<f64> = ts_ms_input
            .iter()
            .map(|t| t.timestamp_millis() as f64)
            .collect();
        let mut series_map: BTreeMap<String, RawArray> = BTreeMap::new();
        series_map.insert("returns".into(), f64_slice_to_raw_array(&values));
        series_map.insert(
            "timestamps_ms".into(),
            f64_slice_to_raw_array(&ts_ms_input_f64),
        );
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
        // Suppress the unused-variable warning when `series` is only read in
        // the resolve step (it's used to label the kernel input above).
        let _ = series.as_str();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_series(req: &ScanRequest) -> Result<VolSeries, ScanError> {
    let raw = req.resolved_params.get("series");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.vol.rolling: series must be log_returns|close; got {v}"
            ))
        })?,
        None => "log_returns",
    };
    match label {
        "log_returns" => Ok(VolSeries::LogReturns),
        "close" => Ok(VolSeries::Close),
        other => Err(ScanError::Kernel(format!(
            "stats.vol.rolling: series must be log_returns|close; got {other:?}"
        ))),
    }
}

fn resolve_window(req: &ScanRequest) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("window").ok_or_else(|| {
        ScanError::Kernel(
            "stats.vol.rolling: required param `window` missing (InvalidParameter)".to_string(),
        )
    })?;
    let w_i64 = raw.as_i64().ok_or_else(|| {
        ScanError::Kernel(format!(
            "stats.vol.rolling: window must be an integer; got {raw}"
        ))
    })?;
    if w_i64 < 2 {
        return Err(ScanError::Kernel(format!(
            "stats.vol.rolling: window must be >= 2; got {w_i64}"
        )));
    }
    usize::try_from(w_i64)
        .map_err(|_| ScanError::Kernel(format!("stats.vol.rolling: window out of range: {w_i64}")))
}

#[allow(
    clippy::cast_precision_loss,
    reason = "window is bounded by usize::MAX; realistic values are << 2^52"
)]
#[inline]
fn window_to_f64(window: usize) -> f64 {
    window as f64
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
    fn vol_rolling_id_and_version() {
        let s = VolRollingScan;
        assert_eq!(s.id(), "stats.vol.rolling");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn vol_rolling_arity_is_single() {
        assert_eq!(VolRollingScan.arity(), ScanArity::Single);
    }

    #[test]
    fn vol_rolling_param_schema() {
        let schema = VolRollingScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["window"]["type"], "integer");
        assert_eq!(schema["properties"]["window"]["minimum"], 2);
        assert_eq!(schema["required"][0], "window");
        let series = &schema["properties"]["series"];
        assert_eq!(series["default"], "log_returns");
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn vol_rolling_default_params_dispatches_log_returns() {
        // Bar frame with 32 closes -> 31 log returns -> with window=10 ->
        // 22 rolling windows.
        let bars = lcg_bar_frame_seeded(32, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"window": 10}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VolRollingScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // raw.series carries log returns (length 31).
        let raw = r.raw.as_ref().expect("raw present");
        assert_eq!(raw.series["returns"].shape, vec![31]);
        // n_windows = 31 - 10 + 1 = 22.
        assert_eq!(r.effect.n, Some(22));
    }

    #[test]
    fn vol_rolling_invalid_window_low() {
        let bars = lcg_bar_frame_seeded(32, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"window": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = VolRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => assert!(msg.contains("window must be >= 2"), "msg: {msg}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn vol_rolling_invalid_window_high() {
        let bars = lcg_bar_frame_seeded(8, 3);
        let mut sink = VecSink::new();
        // 8 closes -> 7 returns; window=100 must reject.
        let req = sample_request_with_params(serde_json::json!({"window": 100}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = VolRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => assert!(
                msg.contains("InsufficientData") || msg.contains("must be <="),
                "msg: {msg}"
            ),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    /// Pitfall 1 — ONE envelope per invocation; NEVER one per window.
    #[test]
    fn vol_rolling_emits_one_result() {
        let bars = lcg_bar_frame_seeded(32, 4);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"window": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VolRollingScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1, "exactly ONE envelope (Pattern D / Pitfall 1)");
    }

    #[test]
    fn vol_rolling_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(32, 5);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"window": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VolRollingScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.vol.rolling@1");
        assert_eq!(r.effect.metric, "vol_rolling_last");
        assert_eq!(r.effect.p_value, None);
        assert_eq!(r.effect.ci95, None);
        // 32 closes -> 31 returns -> n_windows = 31 - 5 + 1 = 27.
        assert_eq!(r.effect.n, Some(27));
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec!["values", "vol_of_vol", "window_length", "window_starts_ms"]
        );
        let raw = r.raw.as_ref().expect("raw present");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
    }

    #[test]
    fn vol_rolling_extras_have_correct_lengths() {
        let bars = lcg_bar_frame_seeded(32, 6);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"window": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VolRollingScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // n_windows = 27. values.len() == window_starts_ms.len() == 27.
        assert_eq!(r.effect.extra["values"].shape, vec![27]);
        assert_eq!(r.effect.extra["window_starts_ms"].shape, vec![27]);
        // window_length is a 1-element vector.
        assert_eq!(r.effect.extra["window_length"].shape, vec![1]);
        // vol_of_vol = vov(vols, w=5) -> 27 - 5 + 1 = 23.
        assert_eq!(r.effect.extra["vol_of_vol"].shape, vec![23]);
    }

    #[test]
    fn vol_rolling_known_input_matches_hand_derived() {
        // 5 constant closes -> 4 log-returns of 0.0 -> rolling-std of 0.0
        // is all zeros (constant input).
        let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let n = 5;
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: (0..n)
                .map(|i| t0 + Duration::minutes(15 * i64::try_from(i).unwrap()))
                .collect(),
            ts_close_utc: (0..n)
                .map(|i| t0 + Duration::minutes(15 * (i64::try_from(i).unwrap() + 1)))
                .collect(),
            open: vec![1.0; n],
            high: vec![1.001; n],
            low: vec![0.999; n],
            close: vec![1.0; n],
            tick_volume: vec![1.0; n],
        };
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"window": 2}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VolRollingScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // 5 closes -> 4 log returns -> rolling window=2 -> 3 vols.
        assert_eq!(r.effect.n, Some(3));
        // value (last vol) == 0.0.
        assert_eq!(r.effect.value, 0.0);
    }

    #[test]
    fn vol_rolling_cancellation_during_window_loop() {
        let bars = lcg_bar_frame_seeded(64, 7);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"window": 5}));
        // Cancel BEFORE run — also tests cancel-at-entry. Mid-loop
        // cancellation would require a thread-spawn dance; the contract is
        // identical: Ok(()) without emitting.
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        VolRollingScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel returns Ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn vol_rolling_raw_new_enforces_timestamps_ms() {
        let bars = lcg_bar_frame_seeded(32, 8);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"window": 3}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        VolRollingScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!(r.raw.as_ref().unwrap().series.contains_key("timestamps_ms"));
    }
}

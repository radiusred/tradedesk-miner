//! `EventWindowScan` — SEAS-06 (Plan 04-10 Task 3).
//!
//! Caller-supplied event-timestamp aligned pre/post window aggregation. For
//! each event timestamp in `params.event_timestamps` the scan resolves the
//! event's bar index via `partition_point` over the bar-open epoch-ms
//! timestamps, then computes the mean + ddof=1 std of returns over the
//! preceding `pre_window_bars` and following `post_window_bars`. Events
//! whose pre/post windows don't fit inside the bar range are silently
//! skipped (consistent with the SEAS-04 middle-of-month behaviour).
//!
//! ## D4-09 contract
//!
//! - `id = "seas.event.pre_post_window"`, `version = 1`.
//! - `arity = ScanArity::Single`.
//! - `param_schema`: required `event_timestamps: array of i64` (ms-since-epoch
//!   UTC); optional `pre_window_bars: integer >= 1` (default 5); optional
//!   `post_window_bars: integer >= 1` (default 5).
//! - `effect.metric = "event_post_window_mean"`, `effect.value` = arithmetic
//!   mean of `post_window_means` across the processed events.
//! - `effect.extra = {event_count, post_window_bars, post_window_means,
//!   post_window_stds, pre_window_bars, pre_window_means, pre_window_stds}`.
//! - `raw.series = {returns, timestamps_ms}`.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// SEAS-06 — event-window scan.
pub struct EventWindowScan;

const SCAN_ID: &str = "seas.event.pre_post_window";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "event_post_window_mean";
const DEFAULT_PRE_WINDOW_BARS: i64 = 5;
const DEFAULT_POST_WINDOW_BARS: i64 = 5;
/// T-04-10-01 mitigation — DOS via large `event_timestamps` array. 10^5 events
/// is well beyond any realistic use case; bounded above to keep the
/// O(events * log(bars)) work tractable.
const MAX_EVENT_TIMESTAMPS: usize = 100_000;

impl Scan for EventWindowScan {
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
                "event_timestamps": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_EVENT_TIMESTAMPS,
                    "items": { "type": "integer" },
                    "description": "UTC event timestamps in ms-since-epoch. Events outside the bar range OR with insufficient pre/post bars are silently skipped. The event bar is the first bar of the post window; the pre window stops one bar before the event."
                },
                "pre_window_bars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Number of bars BEFORE the event to aggregate. Default 5."
                },
                "post_window_bars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Number of bars AT-OR-AFTER the event to aggregate (the event bar is the first post-window bar). Default 5."
                }
            },
            "required": ["event_timestamps"],
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[
                "event_count",
                "post_window_bars",
                "post_window_means",
                "post_window_stds",
                "pre_window_bars",
                "pre_window_means",
                "pre_window_stds",
            ],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "envelope construction + per-event window aggregation live together per Pattern A"
    )]
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        let returns = log_returns(&ctx.bars.close);
        let n = returns.len();
        if n < 2 {
            return Err(ScanError::Kernel(format!(
                "seas.event.pre_post_window: need at least 2 returns; got n={n}"
            )));
        }

        let pre_bars = resolve_window_bars(req, "pre_window_bars", DEFAULT_PRE_WINDOW_BARS)?;
        let post_bars = resolve_window_bars(req, "post_window_bars", DEFAULT_POST_WINDOW_BARS)?;
        let event_timestamps = resolve_event_timestamps(req)?;

        // Per-return bar-open epoch-ms (aligned with returns vector).
        let timestamps_ms: Vec<i64> = ctx
            .bars
            .ts_open_utc
            .iter()
            .skip(1)
            .map(chrono::DateTime::timestamp_millis)
            .collect();
        debug_assert_eq!(timestamps_ms.len(), n);

        let r = kernel::event_window_stats(
            &returns,
            &timestamps_ms,
            &event_timestamps,
            pre_bars,
            post_bars,
        );

        // effect.value = arithmetic mean of post_window_means across events.
        // When event_count == 0, fold yields 0.0 — consistent with "no
        // signal" when no events met the boundary check.
        #[allow(
            clippy::cast_precision_loss,
            reason = "event_count <= MAX_EVENT_TIMESTAMPS (10^5); fits f64 mantissa"
        )]
        let value = if r.event_count == 0 {
            0.0
        } else {
            r.post_means.iter().copied().sum::<f64>() / r.event_count as f64
        };

        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "pre_window_means".into(),
            f64_slice_to_raw_array(&r.pre_means),
        );
        extra.insert(
            "post_window_means".into(),
            f64_slice_to_raw_array(&r.post_means),
        );
        extra.insert(
            "pre_window_stds".into(),
            f64_slice_to_raw_array(&r.pre_stds),
        );
        extra.insert(
            "post_window_stds".into(),
            f64_slice_to_raw_array(&r.post_stds),
        );
        #[allow(
            clippy::cast_precision_loss,
            reason = "small scalars; fit f64 mantissa"
        )]
        let event_count_arr: Vec<f64> = vec![r.event_count as f64];
        #[allow(
            clippy::cast_precision_loss,
            reason = "pre/post bars <= u32 bounded; fit f64 mantissa"
        )]
        let pre_bars_arr: Vec<f64> = vec![pre_bars as f64];
        #[allow(
            clippy::cast_precision_loss,
            reason = "pre/post bars <= u32 bounded; fit f64 mantissa"
        )]
        let post_bars_arr: Vec<f64> = vec![post_bars as f64];
        extra.insert(
            "event_count".into(),
            f64_slice_to_raw_array(&event_count_arr),
        );
        extra.insert(
            "pre_window_bars".into(),
            f64_slice_to_raw_array(&pre_bars_arr),
        );
        extra.insert(
            "post_window_bars".into(),
            f64_slice_to_raw_array(&post_bars_arr),
        );

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize -> u64 lossless on 64-bit"
            )]
            n: Some(n as u64),
            ci95: None,
            effect_size: None,
            extra,
        };

        let timestamps_ms_f: Vec<f64> = timestamps_ms
            .iter()
            .map(|&v| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "epoch-ms fits f64 mantissa for realistic timestamps"
                )]
                let f = v as f64;
                f
            })
            .collect();
        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert("returns".into(), f64_slice_to_raw_array(&returns));
        series.insert(
            "timestamps_ms".into(),
            f64_slice_to_raw_array(&timestamps_ms_f),
        );
        let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;

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
            repro: None,
        };

        sink.write_envelope(&Finding::Result(result))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_window_bars(req: &ScanRequest, name: &str, default: i64) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get(name);
    let v: i64 = match raw {
        Some(v) => v
            .as_i64()
            .ok_or_else(|| ScanError::Kernel(format!("{name} must be an integer; got {v}")))?,
        None => default,
    };
    if v < 1 {
        return Err(ScanError::Kernel(format!("{name} must be >= 1; got {v}")));
    }
    usize::try_from(v).map_err(|_| ScanError::Kernel(format!("{name} out of range for usize")))
}

fn resolve_event_timestamps(req: &ScanRequest) -> Result<Vec<i64>, ScanError> {
    let raw = req
        .resolved_params
        .get("event_timestamps")
        .ok_or_else(|| ScanError::Kernel("event_timestamps is required".into()))?;
    let arr = raw.as_array().ok_or_else(|| {
        ScanError::Kernel(format!("event_timestamps must be an array; got {raw}"))
    })?;
    if arr.is_empty() {
        return Err(ScanError::Kernel(
            "event_timestamps must be non-empty".into(),
        ));
    }
    if arr.len() > MAX_EVENT_TIMESTAMPS {
        return Err(ScanError::Kernel(format!(
            "event_timestamps too large: {} > {MAX_EVENT_TIMESTAMPS} (T-04-10-01)",
            arr.len()
        )));
    }
    let mut out = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        let ts = v.as_i64().ok_or_else(|| {
            ScanError::Kernel(format!(
                "event_timestamps[{i}] must be an integer (ms-since-epoch); got {v}"
            ))
        })?;
        out.push(ts);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless)]
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
    fn lcg_bar_frame(n: usize, seed: u64, start: DateTime<Utc>) -> BarFrame {
        let mut s = seed as u32;
        let mut closes = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            closes.push(1.0 + frac);
        }
        let ts_open: Vec<DateTime<Utc>> = (0..n)
            .map(|i| start + Duration::minutes(15 * i as i64))
            .collect();
        let ts_close: Vec<DateTime<Utc>> =
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
            code_revision: "test-rev-abc1234",
            cancel,
            sleep_after_first_finding_ms: None,
        }
    }

    fn parse_sink(sink: &VecSink) -> Vec<Finding> {
        sink.0
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<Finding>(line).expect("parse"))
            .collect()
    }

    #[test]
    fn event_window_id_and_version() {
        let s = EventWindowScan;
        assert_eq!(s.id(), "seas.event.pre_post_window");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn event_window_arity_is_single() {
        let s = EventWindowScan;
        assert_eq!(s.arity(), ScanArity::Single);
    }

    #[test]
    fn event_window_param_schema() {
        let s = EventWindowScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["event_timestamps"]["type"], "array");
        let req: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(req.contains(&"event_timestamps"));
    }

    #[test]
    fn event_window_emits_one_result() {
        let bars = lcg_bar_frame(672, 1, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        // Pick an event timestamp inside the range — bar 100's epoch-ms.
        let event_ts = bars.ts_open_utc[100].timestamp_millis() + 60_000;
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"event_timestamps": [event_ts]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EventWindowScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn event_window_result_envelope_shape() {
        let bars = lcg_bar_frame(672, 2, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let event_ts_a = bars.ts_open_utc[100].timestamp_millis() + 60_000;
        let event_ts_b = bars.ts_open_utc[200].timestamp_millis() + 60_000;
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"event_timestamps": [event_ts_a, event_ts_b]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EventWindowScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "seas.event.pre_post_window@1");
        assert_eq!(r.effect.metric, "event_post_window_mean");
        for key in [
            "event_count",
            "post_window_bars",
            "post_window_means",
            "post_window_stds",
            "pre_window_bars",
            "pre_window_means",
            "pre_window_stds",
        ] {
            assert!(
                r.effect.extra.contains_key(key),
                "effect.extra[{key}] missing"
            );
        }
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    /// Event at an exact bar timestamp produces pre/post means computed from
    /// the right slices. Build a 20-bar series; place event at bar index 5
    /// (`timestamps_ms`[5] is the timestamp of bar 6 in 0-indexed terms, since
    /// `log_returns` skips the first bar).
    #[test]
    fn event_window_event_at_exact_bar() {
        // Construct deterministic bars where returns are predictable.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let n = 20_usize;
        // Closes designed so log_returns produce e.g. 0.1, 0.2, ...
        // Use exp(returns): close[i+1] = close[i] * exp(0.1)
        let mut close = vec![1.0_f64];
        for _ in 1..n {
            close.push(close.last().unwrap() * (0.1_f64).exp());
        }
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| start + Duration::minutes(15 * i as i64))
            .collect();
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts.clone(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
            open: close.clone(),
            high: close.iter().map(|c| c + 0.001).collect(),
            low: close.iter().map(|c| c - 0.001).collect(),
            close: close.clone(),
            tick_volume: vec![1.0; n],
        };
        // log_returns has 19 entries, all ≈ 0.1. timestamps_ms is the bar
        // timestamps of bars 1..=19 (i.e. ts[1..]).
        let event_ts = bars.ts_open_utc[6].timestamp_millis(); // idx 5 in returns
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({
            "event_timestamps": [event_ts],
            "pre_window_bars": 3,
            "post_window_bars": 3
        }));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EventWindowScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let event_count = decode_f64(&r.effect.extra, "event_count");
        assert_eq!(event_count[0], 1.0);
        let pre_means = decode_f64(&r.effect.extra, "pre_window_means");
        let post_means = decode_f64(&r.effect.extra, "post_window_means");
        // Every return ≈ 0.1 -> means ≈ 0.1.
        assert!((pre_means[0] - 0.1).abs() < 1e-12);
        assert!((post_means[0] - 0.1).abs() < 1e-12);
    }

    /// Event outside the bar range -> skipped.
    #[test]
    fn event_window_event_outside_bar_range_ignored() {
        let bars = lcg_bar_frame(672, 3, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        // Event one year after the last bar.
        let event_ts =
            bars.ts_open_utc.last().unwrap().timestamp_millis() + 365 * 24 * 60 * 60 * 1000;
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"event_timestamps": [event_ts]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EventWindowScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let event_count = decode_f64(&r.effect.extra, "event_count");
        assert_eq!(event_count[0], 0.0);
    }

    /// Event with insufficient pre window -> skipped.
    #[test]
    fn event_window_event_with_insufficient_pre_window() {
        let bars = lcg_bar_frame(672, 4, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        // Event at bar 1's timestamp -> idx=1 in returns; pre_window_bars=5 -> skip.
        let event_ts = bars.ts_open_utc[1].timestamp_millis();
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"event_timestamps": [event_ts]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EventWindowScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let event_count = decode_f64(&r.effect.extra, "event_count");
        assert_eq!(event_count[0], 0.0);
    }

    #[test]
    fn event_window_multiple_events_aggregated() {
        let bars = lcg_bar_frame(672, 5, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let event_ts: Vec<i64> = [100_usize, 200, 300, 400]
            .iter()
            .map(|&i| bars.ts_open_utc[i].timestamp_millis())
            .collect();
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"event_timestamps": event_ts}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EventWindowScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let event_count = decode_f64(&r.effect.extra, "event_count");
        assert_eq!(event_count[0], 4.0);
        let pre_means = decode_f64(&r.effect.extra, "pre_window_means");
        assert_eq!(pre_means.len(), 4);
    }

    #[test]
    fn event_window_cancellation() {
        let bars = lcg_bar_frame(672, 6, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let event_ts = bars.ts_open_utc[100].timestamp_millis();
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"event_timestamps": [event_ts]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        EventWindowScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn event_window_empty_event_list_emits_scan_error() {
        let bars = lcg_bar_frame(672, 7, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"event_timestamps": []}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = EventWindowScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn event_window_invalid_window_bars_zero_pre() {
        let bars = lcg_bar_frame(672, 8, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let event_ts = bars.ts_open_utc[100].timestamp_millis();
        let mut sink = VecSink::new();
        let req = sample_request(
            serde_json::json!({"event_timestamps": [event_ts], "pre_window_bars": 0}),
        );
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = EventWindowScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn event_window_invalid_window_bars_zero_post() {
        let bars = lcg_bar_frame(672, 9, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let event_ts = bars.ts_open_utc[100].timestamp_millis();
        let mut sink = VecSink::new();
        let req = sample_request(
            serde_json::json!({"event_timestamps": [event_ts], "post_window_bars": 0}),
        );
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = EventWindowScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn event_window_rejects_oversized_event_list() {
        let bars = lcg_bar_frame(64, 10, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut huge: Vec<i64> = Vec::with_capacity(MAX_EVENT_TIMESTAMPS + 1);
        for i in 0..=MAX_EVENT_TIMESTAMPS {
            huge.push(1_000 + i as i64);
        }
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"event_timestamps": huge}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = EventWindowScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => {
                assert!(msg.contains("too large"), "msg={msg}");
            }
            other => panic!("got {other:?}"),
        }
    }

    fn decode_f64(extra: &BTreeMap<String, RawArray>, key: &str) -> Vec<f64> {
        let arr = extra.get(key).unwrap_or_else(|| panic!("{key}"));
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
}

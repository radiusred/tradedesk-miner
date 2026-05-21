//! `DayOfWeekScan` — SEAS-02 (Plan 04-09 Task 2).
//!
//! 7-bucket day-of-week return profile. Reuses the shared
//! [`crate::scan::seas::bucketing::bucket_stats`] helper introduced in Task 1.
//! Differs from [`super::hour_of_day`] only in the bucket-key derivation —
//! `weekday().num_days_from_monday()` (0=Mon..6=Sun per RESEARCH §Section 2).
//!
//! ## D4-09 contract
//!
//! - `id = "seas.bucket.day_of_week"`, `version = 1`.
//! - `arity = ScanArity::Single`.
//! - `param_schema`: optional `min_obs_per_bucket: integer >= 1` (default 5).
//! - `effect.metric = "day_of_week_max_abs_t_stat"`, `effect.value` = max-abs
//!   t-stat across the 7 buckets, `effect.extra.{buckets, means, stds, counts,
//!   t_stats, iqrs}` parallel arrays of length 7.
//! - `raw.series.{returns, timestamps_ms}`.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, EffectSize, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::seas::bucketing::bucket_stats;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// SEAS-02 — 7-bucket day-of-week return profile scan.
pub struct DayOfWeekScan;

const SCAN_ID: &str = "seas.bucket.day_of_week";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "day_of_week_max_abs_t_stat";
const NUM_BUCKETS: usize = 7;
const DEFAULT_MIN_OBS_PER_BUCKET: i64 = 5;

impl Scan for DayOfWeekScan {
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
                "min_obs_per_bucket": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Buckets with fewer observations than this threshold emit NaN for mean / std / t-stat / iqr; defaults to 5."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["buckets", "counts", "iqrs", "means", "stds", "t_stats"],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "Phase 5 (Plan 05-01 / D5-03) added `effect_size: None` + Phase 5 (D5-05) added `repro: None` to the Effect / ResultFinding struct literals, nudging the run body from 100 to 101 lines. Splitting the body would obscure the linear scan-build-emit flow without reducing complexity."
    )]
    /// Phase 5 (Plan 05-03 / D5-04 / HYG-03) — opt-in to bootstrap CI.
    fn supports_bootstrap(&self) -> bool {
        true
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
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        let returns = log_returns(&ctx.bars.close);
        let n = returns.len();
        if n < 2 {
            return Err(ScanError::Kernel(format!(
                "seas.bucket.day_of_week: need at least 2 returns; got n={n}"
            )));
        }

        let min_obs = resolve_min_obs(req)?;

        // Bucket key: weekday().num_days_from_monday() of the bar that produced
        // the return — `ts_open_utc[1..]` aligns with log_returns output.
        let ts_for_returns: Vec<chrono::DateTime<chrono::Utc>> =
            ctx.bars.ts_open_utc.iter().skip(1).copied().collect();
        debug_assert_eq!(ts_for_returns.len(), n);
        let bucket_keys = kernel::weekday_keys(&ts_for_returns);

        let r = bucket_stats(&returns, &bucket_keys, NUM_BUCKETS, min_obs);

        let value = max_abs_finite(&r.t_stats);

        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        #[allow(
            clippy::cast_precision_loss,
            reason = "NUM_BUCKETS is 7; trivially fits in f64's 52-bit mantissa"
        )]
        let bucket_indices: Vec<f64> = (0..NUM_BUCKETS).map(|i| i as f64).collect();
        #[allow(
            clippy::cast_precision_loss,
            reason = "counts are bounded by the bar count; realistic OHLCV slices fit in f64's 52-bit mantissa"
        )]
        let counts_f: Vec<f64> = r.counts.iter().map(|c| *c as f64).collect();
        extra.insert("buckets".into(), f64_slice_to_raw_array(&bucket_indices));
        extra.insert("means".into(), f64_slice_to_raw_array(&r.means));
        extra.insert("stds".into(), f64_slice_to_raw_array(&r.stds));
        extra.insert("counts".into(), f64_slice_to_raw_array(&counts_f));
        extra.insert("t_stats".into(), f64_slice_to_raw_array(&r.t_stats));
        extra.insert("iqrs".into(), f64_slice_to_raw_array(&r.iqrs));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize -> u64 lossless on 64-bit targets (Phase 1 invariant)"
            )]
            n: Some(n as u64),
            ci95: None,
            effect_size: Some(EffectSize {
                kind: "max_abs_t_stat".to_string(),
                value,
            }),
            extra,
        };

        let timestamps_ms: Vec<f64> = ctx
            .bars
            .ts_open_utc
            .iter()
            .skip(1)
            .map(|dt| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "epoch-ms fits in f64 mantissa for realistic timestamps"
                )]
                let v = dt.timestamp_millis() as f64;
                v
            })
            .collect();
        debug_assert_eq!(timestamps_ms.len(), n);

        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert("returns".into(), f64_slice_to_raw_array(&returns));
        series.insert(
            "timestamps_ms".into(),
            f64_slice_to_raw_array(&timestamps_ms),
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

fn resolve_min_obs(req: &ScanRequest) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("min_obs_per_bucket");
    let v: i64 = match raw {
        Some(v) => v.as_i64().ok_or_else(|| {
            ScanError::Kernel(format!("min_obs_per_bucket must be an integer; got {v}"))
        })?,
        None => DEFAULT_MIN_OBS_PER_BUCKET,
    };
    if v < 1 {
        return Err(ScanError::Kernel(format!(
            "min_obs_per_bucket must be >= 1; got {v}"
        )));
    }
    let us = usize::try_from(v).map_err(|_| {
        ScanError::Kernel(format!("min_obs_per_bucket out of range for usize: {v}"))
    })?;
    Ok(us)
}

fn max_abs_finite(xs: &[f64]) -> f64 {
    xs.iter()
        .filter(|x| x.is_finite())
        .fold(0.0_f64, |acc, x| acc.max(x.abs()))
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

    /// LCG-seeded bar frame of 96 daily bars starting at the supplied date.
    #[allow(clippy::cast_possible_truncation)]
    fn lcg_daily_bar_frame(n: usize, seed: u64, start: DateTime<Utc>) -> BarFrame {
        let mut s = seed as u32;
        let mut closes = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            closes.push(1.0 + frac);
        }
        let ts_open: Vec<DateTime<Utc>> =
            (0..n).map(|i| start + Duration::days(i as i64)).collect();
        let ts_close: Vec<DateTime<Utc>> = ts_open.iter().map(|t| *t + Duration::days(1)).collect();
        let opens = closes.clone();
        let highs: Vec<f64> = closes.iter().map(|c| c + 0.001).collect();
        let lows: Vec<f64> = closes.iter().map(|c| c - 0.001).collect();
        let vols = vec![1.0; n];
        BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf1d,
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
        let end = Utc.with_ymd_and_hms(2024, 4, 1, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: SCAN_ID.into(),
            version: SCAN_VERSION,
            instruments: vec![InstrumentSpec {
                symbol: "EURUSD".into(),
                side: Side::Bid,
            }],
            timeframe: Timeframe::Tf1d,
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
    fn day_of_week_id_and_version() {
        let s = DayOfWeekScan;
        assert_eq!(s.id(), "seas.bucket.day_of_week");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn day_of_week_arity_is_single() {
        let s = DayOfWeekScan;
        assert_eq!(s.arity(), ScanArity::Single);
    }

    #[test]
    fn day_of_week_param_schema() {
        let s = DayOfWeekScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["properties"]["min_obs_per_bucket"]["type"],
            "integer"
        );
        assert_eq!(schema["properties"]["min_obs_per_bucket"]["minimum"], 1);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn day_of_week_emits_one_result() {
        // 28-day series at daily timeframe.
        let bars = lcg_daily_bar_frame(28, 7, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DayOfWeekScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink(&sink);
        assert_eq!(findings.len(), 1, "exactly one envelope");
    }

    #[test]
    fn day_of_week_result_envelope_shape() {
        let bars = lcg_daily_bar_frame(28, 8, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DayOfWeekScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "seas.bucket.day_of_week@1");
        assert_eq!(r.effect.metric, "day_of_week_max_abs_t_stat");
        // 28 closes -> 27 returns.
        assert_eq!(r.effect.n, Some(27));
        // 7-bucket arrays.
        for key in ["buckets", "means", "stds", "counts", "t_stats", "iqrs"] {
            let arr = r.effect.extra.get(key).unwrap_or_else(|| panic!("{key}"));
            assert_eq!(arr.shape, vec![7], "{key} must be length-7");
        }
        // buckets values are [0..6].
        let buckets = decode_f64(&r.effect.extra, "buckets");
        assert_eq!(buckets, (0..7).map(|i| i as f64).collect::<Vec<_>>());
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    /// 2024-01-01 (Monday) is bucket 0; 2024-01-07 (Sunday) is bucket 6.
    /// Build a 14-day series so we see all 7 buckets at least twice. Returns
    /// align with bars 1..=13 -> the second Monday (bar 7 in 0-indexed) is the
    /// 7th return.
    #[test]
    fn day_of_week_bucket_assignment_uses_monday_as_zero() {
        // 2024-01-01 is a Monday. Build 14 daily bars; the 7 returns from
        // bars[1..=7] cover Tue (1), Wed (2), Thu (3), Fri (4), Sat (5),
        // Sun (6), Mon (0). All 7 weekday buckets see at least one observation.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..14).map(|i| start + Duration::days(i)).collect();
        let close: Vec<f64> = (1..=14).map(|i| 1.0 + i as f64 * 0.01).collect();
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf1d,
            ts_open_utc: ts.clone(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::days(1)).collect(),
            open: close.clone(),
            high: close.iter().map(|c| c + 0.001).collect(),
            low: close.iter().map(|c| c - 0.001).collect(),
            close: close.clone(),
            tick_volume: vec![1.0; 14],
        };
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        DayOfWeekScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let counts = decode_counts(&r.effect.extra);
        // 13 returns: bars[1..=13]. Their weekdays from start=2024-01-01 (Mon)
        // are: Tue(1), Wed(2), Thu(3), Fri(4), Sat(5), Sun(6), Mon(0), Tue(1),
        // Wed(2), Thu(3), Fri(4), Sat(5), Sun(6).
        // Counts: Mon=1, Tue=2, Wed=2, Thu=2, Fri=2, Sat=2, Sun=2.
        assert_eq!(counts[0], 1, "Monday count");
        assert_eq!(counts[1], 2, "Tuesday count");
        assert_eq!(counts[2], 2);
        assert_eq!(counts[3], 2);
        assert_eq!(counts[4], 2);
        assert_eq!(counts[5], 2);
        assert_eq!(counts[6], 2);
    }

    #[test]
    fn day_of_week_n_zero_emits_scan_error() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf1d,
            ts_open_utc: vec![start],
            ts_close_utc: vec![start + Duration::days(1)],
            open: vec![1.0],
            high: vec![1.001],
            low: vec![0.999],
            close: vec![1.0],
            tick_volume: vec![1.0],
        };
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = DayOfWeekScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)), "got {err:?}");
    }

    #[test]
    fn day_of_week_cancellation() {
        let bars = lcg_daily_bar_frame(28, 9, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        DayOfWeekScan.run(&ctx, &req, &mut sink).expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    fn decode_counts(extra: &BTreeMap<String, RawArray>) -> Vec<u64> {
        decode_f64(extra, "counts")
            .iter()
            .map(|x| *x as u64)
            .collect()
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

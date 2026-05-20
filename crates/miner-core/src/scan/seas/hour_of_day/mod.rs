//! `HourOfDayScan` — SEAS-01 (Plan 04-09 Task 1).
//!
//! 24-bucket hour-of-day return profile. The scan groups log-returns by the UTC
//! hour of the bar-close timestamp that produced them, computes per-bucket
//! mean / std (ddof=1) / count / t-stat / IQR via the shared
//! [`crate::scan::seas::bucketing::bucket_stats`] helper, and emits ONE
//! `Finding::Result` whose `effect.value` is the max-abs t-stat across the 24
//! buckets (RESEARCH §1.3).
//!
//! ## D4-09 contract
//!
//! - `id = "seas.bucket.hour_of_day"`, `version = 1`.
//! - `arity = ScanArity::Single` (SEAS family).
//! - `param_schema`: optional `min_obs_per_bucket: integer >= 1`, default `5`.
//! - `effect.metric = "hour_of_day_max_abs_t_stat"`, `effect.value` = max-abs
//!   t-stat across the 24 buckets, `effect.extra.{buckets, means, stds, counts,
//!   t_stats, iqrs}` parallel arrays of length 24.
//! - `raw.series.{returns, timestamps_ms}` for downstream re-test by the quant
//!   agent.
//!
//! ## Pattern analog
//!
//! `crate::scan::ljung_box` — verbatim envelope construction with the per-scan
//! delta in `effect.metric`, `effect.value`, and `effect.extra` keys (PATTERNS
//! Pattern A + Pattern D).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::seas::bucketing::bucket_stats;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// SEAS-01 — 24-bucket hour-of-day return profile scan.
pub struct HourOfDayScan;

const SCAN_ID: &str = "seas.bucket.hour_of_day";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "hour_of_day_max_abs_t_stat";
const NUM_BUCKETS: usize = 24;
const DEFAULT_MIN_OBS_PER_BUCKET: i64 = 5;

impl Scan for HourOfDayScan {
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
    fn supports_bootstrap(&self) -> bool { true }

    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        // Cancel-poll at entry (D3-22 — Pattern 4 site 1).
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        let returns = log_returns(&ctx.bars.close);
        let n = returns.len();
        if n < 2 {
            return Err(ScanError::Kernel(format!(
                "seas.bucket.hour_of_day: need at least 2 returns; got n={n}"
            )));
        }

        let min_obs = resolve_min_obs(req)?;

        // Bucket keys derived from the bar-close timestamp of each return —
        // i.e. ts[i + 1] (the bar whose close ended return i). `log_returns`
        // skips the first bar so ts[1..] is the aligned timestamp slice.
        let ts_for_returns: Vec<chrono::DateTime<chrono::Utc>> =
            ctx.bars.ts_open_utc.iter().skip(1).copied().collect();
        debug_assert_eq!(ts_for_returns.len(), n);
        let bucket_keys = kernel::hour_keys(&ts_for_returns);

        let r = bucket_stats(&returns, &bucket_keys, NUM_BUCKETS, min_obs);

        // Effect.value = max-abs t-stat across buckets (NaN-aware: NaN entries
        // are ignored — fold over `is_finite()` filter).
        let value = max_abs_finite(&r.t_stats);

        // Build `effect.extra` (Pattern D vector-output).
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        #[allow(
            clippy::cast_precision_loss,
            reason = "NUM_BUCKETS is 24; trivially fits in f64's 52-bit mantissa"
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
            effect_size: None,
            extra,
        };

        // raw.series: returns + per-return bar-open timestamps (epoch-ms). The
        // timestamps are the bar-open of bar `t+1` (the bar that produced the
        // return) — matches the SEAS-01 bucket-assignment convention.
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

        // D4-03: leg-labelled sources (length = 1 for SEAS Single-arity).
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

/// Resolve `min_obs_per_bucket` from `req.resolved_params`. Defaults to
/// [`DEFAULT_MIN_OBS_PER_BUCKET`] (5). Validates `>= 1`; returns
/// `ScanError::Kernel(_)` on type / range violation.
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

/// Max absolute value over the supplied slice, ignoring `NaN` entries. Returns
/// `0.0` when no finite entry exists (the bucket profile is degenerate — all
/// buckets either empty or zero-variance).
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

    /// Build a deterministic `BarFrame` of `n` 15m bars starting at the
    /// supplied datetime. Uses the same LCG-seeded scheme as Phase 3 so closes
    /// are non-constant (denom != 0 for returns).
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
    fn hour_of_day_id_and_version() {
        let s = HourOfDayScan;
        assert_eq!(s.id(), "seas.bucket.hour_of_day");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn hour_of_day_arity_is_single() {
        let s = HourOfDayScan;
        assert_eq!(s.arity(), ScanArity::Single);
    }

    #[test]
    fn hour_of_day_param_schema() {
        let s = HourOfDayScan;
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
    fn hour_of_day_emits_one_result() {
        // 7 days * 24h * 4 bars/h at 15m = 672 bars.
        let bars = lcg_bar_frame(672, 1, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        HourOfDayScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink(&sink);
        assert_eq!(findings.len(), 1, "exactly one envelope");
        assert!(matches!(findings[0], Finding::Result(_)));
    }

    #[test]
    fn hour_of_day_result_envelope_shape() {
        let bars = lcg_bar_frame(672, 2, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        HourOfDayScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // PINNED field names (Pattern A).
        assert_eq!(r.scan_id_at_version, "seas.bucket.hour_of_day@1");
        assert_eq!(r.effect.metric, "hour_of_day_max_abs_t_stat");
        // 672 bars -> 671 returns.
        assert_eq!(r.effect.n, Some(671));
        assert!(r.effect.p_value.is_none());
        assert!(r.effect.ci95.is_none());
        // effect.extra keys + length-24 vectors.
        for key in ["buckets", "means", "stds", "counts", "t_stats", "iqrs"] {
            let arr = r.effect.extra.get(key).unwrap_or_else(|| panic!("{key}"));
            assert_eq!(arr.shape, vec![24], "{key} must be length-24");
        }
        // sources length 1 (Single arity).
        assert_eq!(r.data_slice.sources.len(), 1);
        // raw series.
        let raw = r.raw.as_ref().expect("raw present");
        assert!(raw.series.contains_key("returns"));
        assert!(raw.series.contains_key("timestamps_ms"));
    }

    /// Plan 04-09 Task 1 behavior — bucket-assignment uses `ts_open_utc.hour()`
    /// of the bar that produced the return. With a hand-built `BarFrame` of 5
    /// bars at 00:00, 00:15, 00:30, 00:45, 01:00, `log_returns` produces 4
    /// returns aligned with bars 1..=4: hours [0, 0, 0, 1]. After running the
    /// scan, counts[0] = 3, counts[1] = 1.
    #[test]
    fn hour_of_day_bucket_assignment_correct() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..5)
            .map(|i| start + Duration::minutes(15 * i as i64))
            .collect();
        let close = vec![1.0_f64, 1.1, 1.2, 1.15, 1.18];
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
            tick_volume: vec![1.0; 5],
        };
        let mut sink = VecSink::new();
        // min_obs = 1 to keep buckets visible.
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        HourOfDayScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // Decode counts.
        let counts = decode_counts(&r.effect.extra);
        assert_eq!(counts.len(), 24);
        assert_eq!(counts[0], 3, "hour 0 should have 3 returns");
        assert_eq!(counts[1], 1, "hour 1 should have 1 return");
        // All other hours empty.
        for h in 2..24 {
            assert_eq!(counts[h], 0, "hour {h} should be empty");
        }
    }

    /// Sparse buckets: `min_obs_per_bucket = 5` and a hour with 3 observations
    /// emit `NaN` for that bucket's t-stat (Plan Task 1 behavior).
    #[test]
    fn hour_of_day_sparse_buckets_handled() {
        // Build 12 bars all on hour 0 -> 11 returns all hour 0, count = 11.
        // Hour 1 has count 0 -> NaN. With min_obs = 5, hour 0 stays valid.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..12).map(|i| start + Duration::minutes(i)).collect();
        let close: Vec<f64> = (1..=12).map(|i| 1.0 + i as f64 * 0.001).collect();
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts.clone(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(1)).collect(),
            open: close.clone(),
            high: close.iter().map(|c| c + 0.001).collect(),
            low: close.iter().map(|c| c - 0.001).collect(),
            close: close.clone(),
            tick_volume: vec![1.0; 12],
        };
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        HourOfDayScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let t_stats = decode_f64(&r.effect.extra, "t_stats");
        // Bucket 1 has 0 observations -> NaN.
        assert!(t_stats[1].is_nan(), "empty bucket -> NaN");
        // Bucket 0 has 11 observations >= 5 -> finite (or 0.0 if degenerate).
        assert!(
            !t_stats[0].is_nan(),
            "bucket with 11 obs must be non-NaN; got {}",
            t_stats[0]
        );
    }

    #[test]
    fn hour_of_day_n_zero_emits_scan_error() {
        // 1 bar -> 0 returns -> error.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: vec![start],
            ts_close_utc: vec![start + Duration::minutes(15)],
            open: vec![1.0],
            high: vec![1.001],
            low: vec![0.999],
            close: vec![1.0],
            tick_volume: vec![1.0],
        };
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = HourOfDayScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)), "got {err:?}");
    }

    #[test]
    fn hour_of_day_cancellation() {
        let bars = lcg_bar_frame(672, 3, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        HourOfDayScan.run(&ctx, &req, &mut sink).expect("cancel ok");
        assert!(sink.0.is_empty(), "no envelope written on cancel-at-entry");
    }

    /// Test the zero-mean t-stat behaviour — a hand-built series whose returns
    /// in a single hour have mean exactly 0.0 produces `t_stat` == 0 for that
    /// bucket.
    #[test]
    fn hour_of_day_t_stat_zero_mean_bucket() {
        // Build a 4-bar series where the 3 returns from bars 1..=3 are
        // symmetric about 0 in log-space. Closes: e^a, e^{a+1}, e^{a+0}, e^{a-1}
        // Returns: 1, -1, -1. Hmm, mean -1/3 - not zero. Try a symmetric set:
        // closes: 1, 2, 1, 2. log_returns: ln 2, ln 0.5, ln 2. Wait, ln(0.5) = -ln(2).
        // That's [ln2, -ln2, ln2] -> mean = ln2/3 -- not zero.
        // Try closes: 1, 2, 1 -> returns [ln2, -ln2] -> mean = 0, std = sqrt(2*ln2^2/1) = sqrt(2)*|ln2|.
        // n=2, so bucket needs min_obs_per_bucket <= 2.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..3)
            .map(|i| start + Duration::minutes(15 * i as i64))
            .collect();
        let close = vec![1.0_f64, 2.0, 1.0];
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
            tick_volume: vec![1.0; 3],
        };
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 2}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        HourOfDayScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let t_stats = decode_f64(&r.effect.extra, "t_stats");
        // Bucket 0 has both returns; mean is zero, std non-zero -> t_stat == 0.
        assert!(
            t_stats[0].abs() < 1e-12,
            "bucket 0 t_stat must be ~0 (mean is 0); got {}",
            t_stats[0]
        );
    }

    /// Decode a counts vector (stored as f64) from `effect.extra["counts"]`.
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

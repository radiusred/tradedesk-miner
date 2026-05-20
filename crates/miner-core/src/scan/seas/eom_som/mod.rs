//! `EomSomScan` — SEAS-04 (Plan 04-10 Task 1).
//!
//! Trading-day-of-month bucketed return profile. With the default `cutoff_n=3`
//! the scan produces 6 buckets — EOM-3, EOM-2, EOM-1 (last 3 trading days)
//! followed by SOM-1, SOM-2, SOM-3 (first 3 trading days). Returns from bars
//! in the middle of the month are excluded.
//!
//! ## D4-09 contract
//!
//! - `id = "seas.bucket.eom_som"`, `version = 1`.
//! - `arity = ScanArity::Single`.
//! - `param_schema`: optional `cutoff_n: integer 1..=10` (default 3),
//!   optional `min_obs_per_bucket: integer >= 1` (default 5).
//! - `effect.metric = "eom_som_max_abs_t_stat"`, `effect.value` = max-abs
//!   finite t-stat across the `2 * cutoff_n` buckets.
//! - `effect.extra.{bucket_labels, counts, cutoff_n, iqrs, means, stds,
//!   t_stats}`. `bucket_labels` is a UTF-8 JSON-encoded array of
//!   `"EOM-N".."EOM-1","SOM-1".."SOM-N"` strings (encoded the same way the
//!   SEAS-03 session bucket-labels are wired by Plan 04-09).
//! - `raw.series.{returns, timestamps_ms}`.
//!
//! ## Calendar
//!
//! Trading days come from [`crate::calendar::Calendar`] (Phase 2 D2-08). The
//! v1 default is [`Calendar::fx_major`] — Fri-22:00 UTC to Sun-22:00 UTC
//! closed plus Christmas Day and New Year's Day.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::calendar::Calendar;
use crate::findings::{
    Base64Bytes, DataSlice, Dtype, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding,
    Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::seas::bucketing::bucket_stats;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// SEAS-04 — trading-day-of-month return profile scan.
pub struct EomSomScan;

const SCAN_ID: &str = "seas.bucket.eom_som";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "eom_som_max_abs_t_stat";
const DEFAULT_CUTOFF_N: i64 = 3;
const MAX_CUTOFF_N: i64 = 10;
const DEFAULT_MIN_OBS_PER_BUCKET: i64 = 5;

impl Scan for EomSomScan {
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
                "cutoff_n": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_CUTOFF_N,
                    "description": "Number of trading days at each month edge to bucket. Default 3 -> 6 buckets EOM-3..EOM-1, SOM-1..SOM-3."
                },
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
            effect_extra_keys: &[
                "bucket_labels",
                "counts",
                "cutoff_n",
                "iqrs",
                "means",
                "stds",
                "t_stats",
            ],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "envelope construction + bucket-key derivation + label encoding live together per Pattern A; splitting would obscure flow"
    )]
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        // Cancel-poll cadence: every 4096 iterations is sufficient for the
        // typical bar count (1m..6y / 15m ~= 1.4M bars) without hot-loop
        // overhead. Hoisted to function head per clippy::items_after_statements.
        const CANCEL_POLL_CADENCE: usize = 4096;

        // Cancel-poll at entry.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        let returns = log_returns(&ctx.bars.close);
        let n = returns.len();
        if n < 2 {
            return Err(ScanError::Kernel(format!(
                "seas.bucket.eom_som: need at least 2 returns; got n={n}"
            )));
        }

        let cutoff_n = resolve_cutoff_n(req)?;
        let min_obs = resolve_min_obs(req)?;
        let num_buckets = 2 * cutoff_n;

        // Anchor timestamps for each return (the bar whose close produced
        // the return — ts_open_utc[1..]).
        let ts_for_returns: Vec<chrono::DateTime<chrono::Utc>> =
            ctx.bars.ts_open_utc.iter().skip(1).copied().collect();
        debug_assert_eq!(ts_for_returns.len(), n);

        // The trading calendar is fixed to the FX-major default for v1; a
        // future Reader-supplied per-symbol calendar would flow through here
        // (Phase 3+ deferral).
        let calendar = Calendar::fx_major();

        // Build parallel (values, bucket_keys) — only including returns whose
        // bar lands in an EOM/SOM window. Cancel-poll inside the assignment
        // loop (Pattern 4 site 2). CANCEL_POLL_CADENCE is hoisted to the
        // function head above.
        let mut values: Vec<f64> = Vec::with_capacity(n);
        let mut keys: Vec<usize> = Vec::with_capacity(n);
        for (i, ts) in ts_for_returns.iter().enumerate() {
            if i % CANCEL_POLL_CADENCE == 0 && ctx.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            if let Some(b) = kernel::trading_day_of_month_bucket(*ts, cutoff_n, &calendar) {
                debug_assert!(b < num_buckets, "bucket {b} >= {num_buckets}");
                values.push(returns[i]);
                keys.push(b);
            }
        }

        let r = bucket_stats(&values, &keys, num_buckets, min_obs);

        let value = max_abs_finite(&r.t_stats);

        // Build effect.extra.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        #[allow(
            clippy::cast_precision_loss,
            reason = "counts bounded by bar count; fits f64 mantissa"
        )]
        let counts_f: Vec<f64> = r.counts.iter().map(|c| *c as f64).collect();
        extra.insert("means".into(), f64_slice_to_raw_array(&r.means));
        extra.insert("stds".into(), f64_slice_to_raw_array(&r.stds));
        extra.insert("counts".into(), f64_slice_to_raw_array(&counts_f));
        extra.insert("t_stats".into(), f64_slice_to_raw_array(&r.t_stats));
        extra.insert("iqrs".into(), f64_slice_to_raw_array(&r.iqrs));
        #[allow(
            clippy::cast_precision_loss,
            reason = "cutoff_n bounded by MAX_CUTOFF_N (10)"
        )]
        let cutoff_arr: Vec<f64> = vec![cutoff_n as f64];
        extra.insert("cutoff_n".into(), f64_slice_to_raw_array(&cutoff_arr));

        // bucket_labels — emitted as UTF-8 JSON-encoded array bytes per the
        // SEAS-03 session-labels convention (Plan 04-09 Pattern D).
        let labels: Vec<String> = (0..cutoff_n)
            .map(|i| {
                // index 0..cutoff_n -> EOM-N, EOM-(N-1), ..., EOM-1
                let n_back = cutoff_n - i;
                format!("EOM-{n_back}")
            })
            .chain((0..cutoff_n).map(|i| format!("SOM-{}", i + 1)))
            .collect();
        let labels_bytes =
            serde_json::to_vec(&labels).map_err(|e| ScanError::Kernel(e.to_string()))?;
        let labels_len = u64::try_from(labels_bytes.len())
            .map_err(|_| ScanError::Kernel("bucket_labels: byte length exceeds u64".into()))?;
        extra.insert(
            "bucket_labels".into(),
            RawArray {
                data: Base64Bytes(labels_bytes),
                shape: vec![labels_len],
                dtype: Dtype::F64,
            },
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

        let timestamps_ms: Vec<f64> = ctx
            .bars
            .ts_open_utc
            .iter()
            .skip(1)
            .map(|dt| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "epoch-ms fits f64 mantissa for realistic timestamps"
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_cutoff_n(req: &ScanRequest) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("cutoff_n");
    let v: i64 = match raw {
        Some(v) => v
            .as_i64()
            .ok_or_else(|| ScanError::Kernel(format!("cutoff_n must be an integer; got {v}")))?,
        None => DEFAULT_CUTOFF_N,
    };
    if !(1..=MAX_CUTOFF_N).contains(&v) {
        return Err(ScanError::Kernel(format!(
            "cutoff_n must be in 1..={MAX_CUTOFF_N}; got {v}"
        )));
    }
    let us = usize::try_from(v)
        .map_err(|_| ScanError::Kernel(format!("cutoff_n out of range for usize: {v}")))?;
    Ok(us)
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

    /// Daily bar frame from `start` of length `n` with LCG-seeded closes.
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
        let end = Utc.with_ymd_and_hms(2024, 7, 1, 0, 0, 0).unwrap();
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
    fn eom_som_id_and_version() {
        let s = EomSomScan;
        assert_eq!(s.id(), "seas.bucket.eom_som");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn eom_som_arity_is_single() {
        let s = EomSomScan;
        assert_eq!(s.arity(), ScanArity::Single);
    }

    #[test]
    fn eom_som_param_schema() {
        let s = EomSomScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["cutoff_n"]["type"], "integer");
        assert_eq!(schema["properties"]["cutoff_n"]["minimum"], 1);
        assert_eq!(schema["properties"]["cutoff_n"]["maximum"], MAX_CUTOFF_N);
        assert_eq!(schema["additionalProperties"], false);
    }

    /// Default `cutoff_n=3` -> 6 buckets (2 * 3).
    #[test]
    fn eom_som_default_cutoff_3_produces_6_buckets() {
        // 6 months of daily bars (~180 days) gives every month enough trading
        // days to fully populate the EOM/SOM windows.
        let bars = lcg_daily_bar_frame(180, 17, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EomSomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // 6-bucket parallel arrays.
        for key in ["means", "stds", "counts", "t_stats", "iqrs"] {
            let arr = r.effect.extra.get(key).unwrap_or_else(|| panic!("{key}"));
            assert_eq!(arr.shape, vec![6], "{key} must be length 6");
        }
        // bucket_labels JSON decodes to 6 strings.
        let labels_bytes = &r.effect.extra["bucket_labels"].data.0;
        let labels: Vec<String> = serde_json::from_slice(labels_bytes).expect("bucket_labels JSON");
        assert_eq!(
            labels,
            vec!["EOM-3", "EOM-2", "EOM-1", "SOM-1", "SOM-2", "SOM-3"]
        );
    }

    /// `cutoff_n=5` -> 10 buckets.
    #[test]
    fn eom_som_cutoff_5_produces_10_buckets() {
        let bars = lcg_daily_bar_frame(180, 19, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"cutoff_n": 5, "min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EomSomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        for key in ["means", "stds", "counts", "t_stats", "iqrs"] {
            assert_eq!(
                r.effect.extra.get(key).unwrap().shape,
                vec![10],
                "{key} must be length 10"
            );
        }
    }

    #[test]
    fn eom_som_invalid_cutoff_zero() {
        let bars = lcg_daily_bar_frame(60, 1, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"cutoff_n": 0}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = EomSomScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject cutoff=0");
        assert!(matches!(err, ScanError::Kernel(_)), "got {err:?}");
    }

    #[test]
    fn eom_som_invalid_cutoff_too_large() {
        let bars = lcg_daily_bar_frame(60, 2, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"cutoff_n": 11}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = EomSomScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject cutoff=11");
        assert!(matches!(err, ScanError::Kernel(_)), "got {err:?}");
    }

    /// Build a bar frame anchored at Jan 1 2024; verify that Jan 29 / 30 / 31
    /// land in EOM-3 / EOM-2 / EOM-1 and Jan 2 / 3 / 4 in SOM-1 / SOM-2 /
    /// SOM-3 (`cutoff_n=3` default). We assert via counts: build a single-
    /// month bar frame (Jan 2024) and check the bucket counts.
    #[test]
    fn eom_som_bucket_assignment_jan_2024() {
        // 31 daily bars Jan 1..=Jan 31. log_returns produces 30 returns
        // aligned with bars 1..=30 -> dates Jan 2..=Jan 31. Of these,
        // 1, 6, 7, 13, 14, 20, 21, 27, 28 are non-trading days; we expect
        // SOM-1 = Jan 2, SOM-2 = Jan 3, SOM-3 = Jan 4 (each = 1 return) and
        // EOM-3 = Jan 29, EOM-2 = Jan 30, EOM-1 = Jan 31 (each = 1 return).
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..31).map(|i| start + Duration::days(i)).collect();
        let close: Vec<f64> = (1..=31).map(|i| 1.0 + i as f64 * 0.01).collect();
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
            tick_volume: vec![1.0; 31],
        };
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EomSomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let counts = decode_counts(&r.effect.extra);
        // [EOM-3, EOM-2, EOM-1, SOM-1, SOM-2, SOM-3] = [Jan 29, 30, 31, 2, 3, 4]
        // each gets 1 return.
        assert_eq!(counts, vec![1, 1, 1, 1, 1, 1]);
    }

    /// A mid-month bar (Jan 15 2024) contributes to NO bucket; the bar's
    /// return is omitted entirely. Build a series of 3 bars where ONLY the
    /// middle bar (return) is mid-month and assert counts is all zero.
    #[test]
    fn eom_som_middle_of_month_excluded() {
        // 3 bars: Jan 14, Jan 15, Jan 16 (all mid-month). 2 returns at
        // Jan 15 and Jan 16, both excluded.
        let ts = vec![
            Utc.with_ymd_and_hms(2024, 1, 14, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2024, 1, 15, 0, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2024, 1, 16, 0, 0, 0).unwrap(),
        ];
        let close = vec![1.0_f64, 1.1, 1.2];
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
            tick_volume: vec![1.0; 3],
        };
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EomSomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let counts = decode_counts(&r.effect.extra);
        // Both mid-month returns are excluded -> all 6 buckets are empty.
        assert_eq!(counts, vec![0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn eom_som_emits_one_result() {
        let bars = lcg_daily_bar_frame(120, 3, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EomSomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn eom_som_result_envelope_shape() {
        let bars = lcg_daily_bar_frame(120, 4, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        EomSomScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "seas.bucket.eom_som@1");
        assert_eq!(r.effect.metric, "eom_som_max_abs_t_stat");
        assert_eq!(r.effect.n, Some(119));
        for key in [
            "bucket_labels",
            "counts",
            "cutoff_n",
            "iqrs",
            "means",
            "stds",
            "t_stats",
        ] {
            assert!(
                r.effect.extra.contains_key(key),
                "effect.extra[{key}] missing"
            );
        }
        // sources len 1.
        assert_eq!(r.data_slice.sources.len(), 1);
        // raw series.
        let raw = r.raw.as_ref().expect("raw");
        assert!(raw.series.contains_key("returns"));
        assert!(raw.series.contains_key("timestamps_ms"));
    }

    #[test]
    fn eom_som_cancellation() {
        let bars = lcg_daily_bar_frame(60, 5, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        EomSomScan.run(&ctx, &req, &mut sink).expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn eom_som_n_zero_emits_scan_error() {
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
        let err = EomSomScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
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

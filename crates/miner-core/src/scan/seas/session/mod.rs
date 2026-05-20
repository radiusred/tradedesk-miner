//! `SessionScan` — SEAS-03 (Plan 04-09 Task 3).
//!
//! 4-bucket trading-session return profile with configurable UTC boundaries.
//! Default sessions per RESEARCH §1.8: Asia (22-07, wraps midnight), London
//! (07-16), NY (12-21), Overlap (12-16). Sessions are **independent** buckets
//! (a bar at 13:00 UTC falls in London, NY, AND Overlap simultaneously per
//! RESEARCH §1.8).
//!
//! ## D4-09 contract
//!
//! - `id = "seas.bucket.session"`, `version = 1`.
//! - `arity = ScanArity::Single`.
//! - `param_schema`: optional `sessions` array of `{name, start_utc_hour,
//!   end_utc_hour}` objects (default = [`kernel::FX_MAJOR_DEFAULTS`]);
//!   optional `min_obs_per_bucket: integer >= 1` (default 5). The number of
//!   `sessions` is bounded above by [`MAX_SESSIONS`] per T-04-09-02.
//! - `effect.metric = "session_max_abs_t_stat"`, `effect.value` = max-abs
//!   t-stat across the configured sessions, `effect.extra` carries
//!   `bucket_labels` (UTF-8 JSON-encoded session-name string array bytes per
//!   T-04-09-03), `means`, `stds`, `counts`, `t_stats`, `iqrs`, and
//!   `session_boundaries_utc` (JSON-encoded array of `{name, start, end}`
//!   objects bytes).
//! - `raw.series.{returns, timestamps_ms}`.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Timelike;

use crate::findings::{
    Base64Bytes, DataSlice, Dtype, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding,
    Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::seas::bucketing::bucket_stats_from_groups;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

pub use kernel::{FX_MAJOR_DEFAULTS, SessionDef};

/// SEAS-03 — 4-bucket trading-session return profile scan.
pub struct SessionScan;

const SCAN_ID: &str = "seas.bucket.session";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "session_max_abs_t_stat";
const DEFAULT_MIN_OBS_PER_BUCKET: i64 = 5;
/// T-04-09-02 mitigation — DOS via 1000-entry sessions array. 100 is well
/// beyond any realistic use case (the FX-major default is 4; expert users
/// rarely exceed a dozen).
pub const MAX_SESSIONS: usize = 100;

/// A single owned session definition (parsed from --params or seeded from
/// defaults). The `name` field is `String` here (vs `&'static str` on
/// [`SessionDef`]) so user-supplied JSON can hand us labels we don't know at
/// compile time.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OwnedSessionDef {
    name: String,
    start_utc_h: u32,
    end_utc_h: u32,
}

impl Scan for SessionScan {
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
                },
                "sessions": {
                    "type": "array",
                    "maxItems": MAX_SESSIONS,
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "minLength": 1 },
                            "start_utc_hour": { "type": "integer", "minimum": 0, "maximum": 23 },
                            "end_utc_hour": { "type": "integer", "minimum": 0, "maximum": 23 }
                        },
                        "required": ["name", "start_utc_hour", "end_utc_hour"],
                        "additionalProperties": false
                    },
                    "description": "Trading-session definitions. Default per RESEARCH §1.8 (Asia/London/NY/Overlap). Each session is an INDEPENDENT bucket — overlapping windows mean a bar is counted in every matching session."
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
                "iqrs",
                "means",
                "session_boundaries_utc",
                "stds",
                "t_stats",
            ],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "envelope construction + many-to-many bucket assignment + label/boundary encoding live together in this single function per Pattern A; splitting would obscure the call-site flow"
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
                "seas.bucket.session: need at least 2 returns; got n={n}"
            )));
        }

        let min_obs = resolve_min_obs(req)?;
        let sessions = resolve_sessions(req)?;
        let num_buckets = sessions.len();

        // Many-to-many bucket assignment. For each return, iterate over every
        // session and append the return to every session whose interval
        // contains the bar-close UTC hour.
        let ts_for_returns: Vec<chrono::DateTime<chrono::Utc>> =
            ctx.bars.ts_open_utc.iter().skip(1).copied().collect();
        debug_assert_eq!(ts_for_returns.len(), n);

        let mut per_bucket: Vec<Vec<f64>> = (0..num_buckets).map(|_| Vec::new()).collect();
        for (i, ts) in ts_for_returns.iter().enumerate() {
            let hour = ts.hour();
            for (b, sess) in sessions.iter().enumerate() {
                if kernel::hour_in_session(hour, sess.start_utc_h, sess.end_utc_h) {
                    per_bucket[b].push(returns[i]);
                }
            }
        }

        let r = bucket_stats_from_groups(&mut per_bucket, min_obs);

        let value = max_abs_finite(&r.t_stats);

        // Build effect.extra (Pattern D vector-output + UTF-8 raw-array labels
        // per T-04-09-03).
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        #[allow(
            clippy::cast_precision_loss,
            reason = "counts are bounded by the bar count; realistic OHLCV slices fit in f64's 52-bit mantissa"
        )]
        let counts_f: Vec<f64> = r.counts.iter().map(|c| *c as f64).collect();
        extra.insert("means".into(), f64_slice_to_raw_array(&r.means));
        extra.insert("stds".into(), f64_slice_to_raw_array(&r.stds));
        extra.insert("counts".into(), f64_slice_to_raw_array(&counts_f));
        extra.insert("t_stats".into(), f64_slice_to_raw_array(&r.t_stats));
        extra.insert("iqrs".into(), f64_slice_to_raw_array(&r.iqrs));

        // T-04-09-03 — bucket_labels emitted as a raw array of UTF-8 bytes
        // (a JSON-encoded array of strings). Consumers decode the bytes as
        // UTF-8 then parse as JSON to recover the session names.
        let labels_json: Vec<&str> = sessions.iter().map(|s| s.name.as_str()).collect();
        let labels_bytes =
            serde_json::to_vec(&labels_json).map_err(|e| ScanError::Kernel(e.to_string()))?;
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

        // session_boundaries_utc — JSON-encoded array of {name, start_utc_hour,
        // end_utc_hour} objects so the consumer can re-derive bucket
        // membership for arbitrary timestamps.
        let boundaries_json: Vec<serde_json::Value> = sessions
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "start_utc_hour": s.start_utc_h,
                    "end_utc_hour": s.end_utc_h,
                })
            })
            .collect();
        let boundaries_bytes =
            serde_json::to_vec(&boundaries_json).map_err(|e| ScanError::Kernel(e.to_string()))?;
        let boundaries_len = u64::try_from(boundaries_bytes.len()).map_err(|_| {
            ScanError::Kernel("session_boundaries_utc: byte length exceeds u64".into())
        })?;
        extra.insert(
            "session_boundaries_utc".into(),
            RawArray {
                data: Base64Bytes(boundaries_bytes),
                shape: vec![boundaries_len],
                dtype: Dtype::F64,
            },
        );

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize -> u64 lossless on 64-bit targets"
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
            produced_at_utc: chrono::Utc::now(),
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

/// Resolve the `sessions` parameter array. Returns the FX-major defaults when
/// absent, or parses the user-supplied JSON array. Validates 0..=23 hour
/// bounds and rejects empty arrays. Caps at [`MAX_SESSIONS`] per T-04-09-02.
fn resolve_sessions(req: &ScanRequest) -> Result<Vec<OwnedSessionDef>, ScanError> {
    let raw = req.resolved_params.get("sessions");
    let arr = match raw {
        None => {
            // Default: FX-major sessions.
            return Ok(FX_MAJOR_DEFAULTS
                .iter()
                .map(|d| OwnedSessionDef {
                    name: d.name.to_string(),
                    start_utc_h: d.start_utc_h,
                    end_utc_h: d.end_utc_h,
                })
                .collect());
        }
        Some(v) => v
            .as_array()
            .ok_or_else(|| ScanError::Kernel(format!("sessions must be a JSON array; got {v}")))?,
    };
    if arr.is_empty() {
        return Err(ScanError::Kernel(
            "sessions must be non-empty; got []".into(),
        ));
    }
    if arr.len() > MAX_SESSIONS {
        return Err(ScanError::Kernel(format!(
            "sessions array too large: {} > {MAX_SESSIONS} (T-04-09-02 mitigation)",
            arr.len()
        )));
    }
    let mut out = Vec::with_capacity(arr.len());
    for (i, entry) in arr.iter().enumerate() {
        let obj = entry
            .as_object()
            .ok_or_else(|| ScanError::Kernel(format!("sessions[{i}] must be an object")))?;
        let name = obj
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                ScanError::Kernel(format!("sessions[{i}].name must be a non-empty string"))
            })?;
        if name.is_empty() {
            return Err(ScanError::Kernel(format!(
                "sessions[{i}].name must be non-empty"
            )));
        }
        let start_raw = obj
            .get("start_utc_hour")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| {
                ScanError::Kernel(format!(
                    "sessions[{i}].start_utc_hour must be an integer 0..=23"
                ))
            })?;
        let end_raw = obj
            .get("end_utc_hour")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| {
                ScanError::Kernel(format!(
                    "sessions[{i}].end_utc_hour must be an integer 0..=23"
                ))
            })?;
        if !(0..=23).contains(&start_raw) || !(0..=23).contains(&end_raw) {
            return Err(ScanError::Kernel(format!(
                "sessions[{i}] UTC hour bounds must be in 0..=23; got start={start_raw}, end={end_raw}"
            )));
        }
        out.push(OwnedSessionDef {
            name: name.to_string(),
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            start_utc_h: start_raw as u32,
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            end_utc_h: end_raw as u32,
        });
    }
    Ok(out)
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
    use chrono::{DateTime, Duration, TimeZone, Utc};
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
    fn session_id_and_version() {
        let s = SessionScan;
        assert_eq!(s.id(), "seas.bucket.session");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn session_arity_is_single() {
        let s = SessionScan;
        assert_eq!(s.arity(), ScanArity::Single);
    }

    #[test]
    fn session_param_schema() {
        let s = SessionScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(
            schema["properties"]["min_obs_per_bucket"]["type"],
            "integer"
        );
        assert_eq!(schema["properties"]["sessions"]["type"], "array");
        assert_eq!(schema["properties"]["sessions"]["maxItems"], MAX_SESSIONS);
    }

    /// Plan 04-09 Task 3 — defaults reflect FX-major sessions per RESEARCH §1.8.
    #[test]
    fn session_default_sessions_fx_major() {
        // 7 days * 24h * 4 bars/h = 672 bars at 15m.
        let bars = lcg_bar_frame(672, 1, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SessionScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // 4 sessions in defaults.
        let counts = decode_f64(&r.effect.extra, "counts");
        assert_eq!(counts.len(), 4, "FX-major defaults are 4 sessions");
        // Decode bucket_labels JSON.
        let labels_bytes = &r.effect.extra["bucket_labels"].data.0;
        let labels: Vec<String> =
            serde_json::from_slice(labels_bytes).expect("bucket_labels is JSON array");
        assert_eq!(labels, vec!["asia", "london", "ny", "overlap"]);
        // session_boundaries_utc carries the 4 default boundaries.
        let boundaries_bytes = &r.effect.extra["session_boundaries_utc"].data.0;
        let boundaries: Vec<serde_json::Value> =
            serde_json::from_slice(boundaries_bytes).expect("session_boundaries_utc is JSON");
        assert_eq!(boundaries.len(), 4);
        assert_eq!(boundaries[0]["name"], "asia");
        assert_eq!(boundaries[0]["start_utc_hour"], 22);
        assert_eq!(boundaries[0]["end_utc_hour"], 7);
        assert_eq!(boundaries[1]["name"], "london");
        assert_eq!(boundaries[1]["start_utc_hour"], 7);
        assert_eq!(boundaries[1]["end_utc_hour"], 16);
        assert_eq!(boundaries[2]["name"], "ny");
        assert_eq!(boundaries[2]["start_utc_hour"], 12);
        assert_eq!(boundaries[2]["end_utc_hour"], 21);
        assert_eq!(boundaries[3]["name"], "overlap");
        assert_eq!(boundaries[3]["start_utc_hour"], 12);
        assert_eq!(boundaries[3]["end_utc_hour"], 16);
    }

    /// Plan 04-09 Task 3 — `--params sessions` override produces a 2-bucket
    /// result.
    #[test]
    fn session_param_override() {
        let bars = lcg_bar_frame(672, 2, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({
            "sessions": [
                {"name": "morning", "start_utc_hour": 6, "end_utc_hour": 12},
                {"name": "afternoon", "start_utc_hour": 12, "end_utc_hour": 18},
            ],
            "min_obs_per_bucket": 1
        }));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SessionScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let counts = decode_f64(&r.effect.extra, "counts");
        assert_eq!(counts.len(), 2);
        let labels_bytes = &r.effect.extra["bucket_labels"].data.0;
        let labels: Vec<String> = serde_json::from_slice(labels_bytes).unwrap();
        assert_eq!(labels, vec!["morning", "afternoon"]);
    }

    /// Plan 04-09 Task 3 — overlap handling. A bar at 13:00 UTC must fall in
    /// London (07-16), NY (12-21), and Overlap (12-16). Build a 2-bar series
    /// at 13:00 and 13:15; the single return is included in those 3 buckets
    /// AND excluded from Asia (22-07).
    #[test]
    fn session_overlap_handling() {
        // 3 bars at 13:00, 13:15, 13:30 -> 2 returns (n >= 2). Both at hour
        // 13 which is in London (07-16), NY (12-21), Overlap (12-16) but
        // NOT Asia (22-07).
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 13, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = vec![
            start,
            start + Duration::minutes(15),
            start + Duration::minutes(30),
        ];
        let close = vec![1.0_f64, 1.05, 1.10];
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
        // Use min_obs = 1 so 1-or-2 obs buckets still emit stats.
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SessionScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let counts = decode_f64(&r.effect.extra, "counts");
        // 2 returns, both at hour 13.
        // Asia (idx 0) -> 0; London (idx 1) -> 2; NY (idx 2) -> 2;
        // Overlap (idx 3) -> 2.
        assert_eq!(counts[0] as u64, 0, "asia at 13:00 -> 0");
        assert_eq!(counts[1] as u64, 2, "london at 13:00 -> 2");
        assert_eq!(counts[2] as u64, 2, "ny at 13:00 -> 2");
        assert_eq!(counts[3] as u64, 2, "overlap at 13:00 -> 2");
    }

    /// Plan 04-09 Task 3 — Asia 22-07 wraps midnight. A bar at 23:00 UTC AND a
    /// bar at 03:00 UTC both land in Asia.
    #[test]
    fn session_asia_wraps_midnight() {
        // Build 5 bars: 22:00, 22:15, 23:00 (next bar produces a return),
        // 03:00 next day, 03:15. Return hours: 22, 23, 3, 3. All 4 returns
        // should fall in Asia. None in London (07-16).
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 22, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = vec![
            start,                                              // 22:00
            start + Duration::minutes(15),                      // 22:15
            start + Duration::hours(1),                         // 23:00
            start + Duration::hours(5),                         // 03:00 next day
            start + Duration::hours(5) + Duration::minutes(15), // 03:15
        ];
        let close = vec![1.0_f64, 1.05, 1.1, 1.15, 1.2];
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
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SessionScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let counts = decode_f64(&r.effect.extra, "counts");
        // 4 returns total. Their hours: bar[1]=22:15 -> 22; bar[2]=23:00 -> 23;
        // bar[3]=03:00 -> 3; bar[4]=03:15 -> 3. All 4 in Asia. London / NY /
        // Overlap = 0.
        assert_eq!(counts[0] as u64, 4, "Asia at 22, 23, 03, 03 -> 4");
        assert_eq!(counts[1] as u64, 0, "London should be 0");
        assert_eq!(counts[2] as u64, 0, "NY should be 0");
        assert_eq!(counts[3] as u64, 0, "Overlap should be 0");
    }

    #[test]
    fn session_emits_one_result() {
        let bars = lcg_bar_frame(672, 3, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SessionScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn session_result_envelope_shape() {
        let bars = lcg_bar_frame(672, 4, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        SessionScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "seas.bucket.session@1");
        assert_eq!(r.effect.metric, "session_max_abs_t_stat");
        assert_eq!(r.effect.n, Some(671));
        for key in [
            "bucket_labels",
            "counts",
            "iqrs",
            "means",
            "session_boundaries_utc",
            "stds",
            "t_stats",
        ] {
            assert!(r.effect.extra.contains_key(key), "key {key} missing");
        }
        // Numeric arrays are length 4 (default FX-major sessions).
        for key in ["counts", "iqrs", "means", "stds", "t_stats"] {
            let arr = r.effect.extra.get(key).unwrap();
            assert_eq!(arr.shape, vec![4], "{key} must be length 4");
        }
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    #[test]
    fn session_cancellation() {
        let bars = lcg_bar_frame(672, 5, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        SessionScan.run(&ctx, &req, &mut sink).expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn session_n_zero_emits_scan_error() {
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
        let err = SessionScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    /// T-04-09-02 — sessions array too large is rejected.
    #[test]
    fn session_rejects_oversized_sessions_array() {
        let mut huge = Vec::new();
        for i in 0..=MAX_SESSIONS {
            huge.push(serde_json::json!({
                "name": format!("s{i}"),
                "start_utc_hour": 0,
                "end_utc_hour": 1
            }));
        }
        let bars = lcg_bar_frame(64, 6, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"sessions": huge}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = SessionScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => {
                assert!(msg.contains("too large"), "unexpected msg: {msg}");
            }
            other => panic!("expected Kernel error; got {other:?}"),
        }
    }

    fn decode_f64(extra: &BTreeMap<String, RawArray>, key: &str) -> Vec<f64> {
        let arr = extra.get(key).unwrap_or_else(|| panic!("{key}"));
        let bytes = &arr.data.0;
        assert_eq!(bytes.len() % 8, 0, "{key} byte length not multiple of 8");
        let mut out = Vec::with_capacity(bytes.len() / 8);
        for chunk in bytes.chunks_exact(8) {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(chunk);
            out.push(f64::from_le_bytes(buf));
        }
        out
    }
}

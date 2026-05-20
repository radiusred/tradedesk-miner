//! `AnovaKruskalScan` — SEAS-05 (Plan 04-10 Task 2).
//!
//! Meta-scan computing one-way ANOVA F-statistic + Kruskal-Wallis H-statistic
//! across return buckets derived from one of the other SEAS bucketing
//! schemes. The caller selects the bucketing method via the required
//! `params.buckets_via` enum.
//!
//! ## D4-09 contract
//!
//! - `id = "seas.test.anova_kruskal"`, `version = 1`.
//! - `arity = ScanArity::Single`.
//! - `param_schema`: required `buckets_via: enum` with `"hour_of_day" |
//!   "day_of_week" | "session" | "eom_som"`; optional `min_obs_per_group:
//!   integer >= 1` (default 5).
//! - `effect.metric = "anova_f_statistic"`, `effect.value = F`,
//!   `effect.p_value = ANOVA p-value`.
//! - `effect.extra = {anova_p_value, kw_p_value, kw_stat, group_count,
//!   total_n}`.
//! - `raw.series = {returns, timestamps_ms}`.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::{Datelike, Timelike, Utc};

use crate::calendar::Calendar;
use crate::findings::{
    DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// SEAS-05 — ANOVA + Kruskal-Wallis bucket-comparison meta-scan.
pub struct AnovaKruskalScan;

const SCAN_ID: &str = "seas.test.anova_kruskal";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "anova_f_statistic";
const DEFAULT_MIN_OBS_PER_GROUP: i64 = 5;

/// Supported bucketing methods. Mirrors the SEAS scan ids the meta-scan can
/// borrow from (Plan 04-09 + Plan 04-10 Task 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BucketsVia {
    HourOfDay,
    DayOfWeek,
    Session,
    EomSom,
}

impl BucketsVia {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "hour_of_day" => Some(BucketsVia::HourOfDay),
            "day_of_week" => Some(BucketsVia::DayOfWeek),
            "session" => Some(BucketsVia::Session),
            "eom_som" => Some(BucketsVia::EomSom),
            _ => None,
        }
    }
}

impl Scan for AnovaKruskalScan {
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
                "buckets_via": {
                    "type": "string",
                    "enum": ["hour_of_day", "day_of_week", "session", "eom_som"],
                    "description": "Bucketing scheme to derive groups before running ANOVA + Kruskal-Wallis."
                },
                "min_obs_per_group": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Groups with fewer observations than this threshold are dropped before the test runs; defaults to 5."
                }
            },
            "required": ["buckets_via"],
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[
                "anova_p_value",
                "group_count",
                "kw_p_value",
                "kw_stat",
                "total_n",
            ],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

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
                "seas.test.anova_kruskal: need at least 2 returns; got n={n}"
            )));
        }

        let buckets_via = resolve_buckets_via(req)?;
        let min_obs = resolve_min_obs_per_group(req)?;

        let ts_for_returns: Vec<chrono::DateTime<chrono::Utc>> =
            ctx.bars.ts_open_utc.iter().skip(1).copied().collect();
        debug_assert_eq!(ts_for_returns.len(), n);

        // Derive groups by the chosen bucketing scheme.
        let groups = derive_groups(buckets_via, &returns, &ts_for_returns);

        // Filter groups by min_obs.
        let filtered: Vec<Vec<f64>> =
            groups.into_iter().filter(|g| g.len() >= min_obs).collect();
        if filtered.len() < 2 {
            return Err(ScanError::Kernel(format!(
                "seas.test.anova_kruskal: need >= 2 non-empty groups after min_obs={min_obs} filter; got {}",
                filtered.len()
            )));
        }

        let anova = kernel::one_way_anova(&filtered);
        let kw = kernel::kruskal_wallis(&filtered);

        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "anova_p_value".into(),
            f64_slice_to_raw_array(&[anova.p_value]),
        );
        extra.insert("kw_stat".into(), f64_slice_to_raw_array(&[kw.h_stat]));
        extra.insert("kw_p_value".into(), f64_slice_to_raw_array(&[kw.p_value]));
        #[allow(
            clippy::cast_precision_loss,
            reason = "group_count + total_n are small counts; fit f64 mantissa"
        )]
        let group_count_arr: Vec<f64> = vec![anova.k as f64];
        #[allow(
            clippy::cast_precision_loss,
            reason = "total_n bounded by bar count; fits f64 mantissa"
        )]
        let total_n_arr: Vec<f64> = vec![anova.total_n as f64];
        extra.insert(
            "group_count".into(),
            f64_slice_to_raw_array(&group_count_arr),
        );
        extra.insert("total_n".into(), f64_slice_to_raw_array(&total_n_arr));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: anova.f_stat,
            p_value: Some(anova.p_value),
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize -> u64 lossless on 64-bit"
            )]
            n: Some(anova.total_n as u64),
            ci95: None,
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
        };

        sink.write_envelope(&Finding::Result(result))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_buckets_via(req: &ScanRequest) -> Result<BucketsVia, ScanError> {
    let raw = req
        .resolved_params
        .get("buckets_via")
        .ok_or_else(|| ScanError::Kernel("buckets_via is required".into()))?;
    let s = raw
        .as_str()
        .ok_or_else(|| ScanError::Kernel(format!("buckets_via must be a string; got {raw}")))?;
    BucketsVia::from_str(s).ok_or_else(|| {
        ScanError::Kernel(format!(
            "buckets_via must be one of [hour_of_day, day_of_week, session, eom_som]; got '{s}'"
        ))
    })
}

fn resolve_min_obs_per_group(req: &ScanRequest) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("min_obs_per_group");
    let v: i64 = match raw {
        Some(v) => v.as_i64().ok_or_else(|| {
            ScanError::Kernel(format!("min_obs_per_group must be an integer; got {v}"))
        })?,
        None => DEFAULT_MIN_OBS_PER_GROUP,
    };
    if v < 1 {
        return Err(ScanError::Kernel(format!(
            "min_obs_per_group must be >= 1; got {v}"
        )));
    }
    let us = usize::try_from(v).map_err(|_| {
        ScanError::Kernel(format!("min_obs_per_group out of range for usize: {v}"))
    })?;
    Ok(us)
}

/// Group returns by the chosen bucketing scheme. Each scheme produces a
/// vector of value-groups whose length equals the bucketing arity (24 for
/// `hour_of_day`, 7 for `day_of_week`, 4 for session FX-major defaults, 6 for
/// `eom_som` default `cutoff_n=3`). Empty groups are preserved here; the caller
/// applies the `min_obs_per_group` filter.
fn derive_groups(
    via: BucketsVia,
    returns: &[f64],
    ts: &[chrono::DateTime<chrono::Utc>],
) -> Vec<Vec<f64>> {
    debug_assert_eq!(returns.len(), ts.len());
    match via {
        BucketsVia::HourOfDay => {
            let mut groups: Vec<Vec<f64>> = (0..24).map(|_| Vec::new()).collect();
            for (i, t) in ts.iter().enumerate() {
                groups[t.hour() as usize].push(returns[i]);
            }
            groups
        }
        BucketsVia::DayOfWeek => {
            let mut groups: Vec<Vec<f64>> = (0..7).map(|_| Vec::new()).collect();
            for (i, t) in ts.iter().enumerate() {
                let k = t.weekday().num_days_from_monday() as usize;
                groups[k].push(returns[i]);
            }
            groups
        }
        BucketsVia::Session => {
            // FX-major defaults per RESEARCH §1.8: Asia 22-07, London 07-16,
            // NY 12-21, Overlap 12-16. Sessions are INDEPENDENT — a bar at
            // 13:00 UTC contributes to London + NY + Overlap simultaneously.
            const SESSIONS: &[(u32, u32)] = &[(22, 7), (7, 16), (12, 21), (12, 16)];
            let mut groups: Vec<Vec<f64>> = (0..SESSIONS.len()).map(|_| Vec::new()).collect();
            for (i, t) in ts.iter().enumerate() {
                let h = t.hour();
                for (b, (s, e)) in SESSIONS.iter().enumerate() {
                    let in_session = if s <= e {
                        h >= *s && h < *e
                    } else {
                        h >= *s || h < *e
                    };
                    if in_session {
                        groups[b].push(returns[i]);
                    }
                }
            }
            groups
        }
        BucketsVia::EomSom => {
            // Default cutoff_n=3 -> 6 buckets EOM-3..EOM-1, SOM-1..SOM-3.
            let cutoff = 3_usize;
            let num_buckets = 2 * cutoff;
            let calendar = Calendar::fx_major();
            let mut groups: Vec<Vec<f64>> = (0..num_buckets).map(|_| Vec::new()).collect();
            for (i, t) in ts.iter().enumerate() {
                if let Some(b) =
                    crate::scan::seas::eom_som::kernel::trading_day_of_month_bucket(
                        *t, cutoff, &calendar,
                    )
                {
                    groups[b].push(returns[i]);
                }
            }
            groups
        }
    }
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

    fn make_ctx<'a>(bars: &'a BarFrame, cancel: Arc<AtomicBool>) -> ScanCtx<'a> {
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
    fn anova_kw_id_and_version() {
        let s = AnovaKruskalScan;
        assert_eq!(s.id(), "seas.test.anova_kruskal");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn anova_kw_arity_is_single() {
        let s = AnovaKruskalScan;
        assert_eq!(s.arity(), ScanArity::Single);
    }

    #[test]
    fn anova_kw_param_schema() {
        let s = AnovaKruskalScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["buckets_via"]["type"], "string");
        // Required.
        let req: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(req.contains(&"buckets_via"));
        // Enum
        let opts: Vec<&str> = schema["properties"]["buckets_via"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(opts, vec!["hour_of_day", "day_of_week", "session", "eom_som"]);
    }

    #[test]
    fn anova_kw_invalid_buckets_via() {
        let bars = lcg_bar_frame(672, 1, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"buckets_via": "bogus"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = AnovaKruskalScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)), "got {err:?}");
    }

    #[test]
    fn anova_kw_missing_buckets_via() {
        let bars = lcg_bar_frame(672, 2, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = AnovaKruskalScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)), "got {err:?}");
    }

    #[test]
    fn anova_kw_emits_one_result() {
        let bars = lcg_bar_frame(672, 3, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(
            serde_json::json!({"buckets_via": "hour_of_day", "min_obs_per_group": 5}),
        );
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        AnovaKruskalScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn anova_kw_result_envelope_shape() {
        let bars = lcg_bar_frame(672, 4, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(
            serde_json::json!({"buckets_via": "hour_of_day", "min_obs_per_group": 5}),
        );
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        AnovaKruskalScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "seas.test.anova_kruskal@1");
        assert_eq!(r.effect.metric, "anova_f_statistic");
        assert!(r.effect.p_value.is_some());
        for key in [
            "anova_p_value",
            "group_count",
            "kw_p_value",
            "kw_stat",
            "total_n",
        ] {
            assert!(
                r.effect.extra.contains_key(key),
                "effect.extra[{key}] missing"
            );
        }
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    #[test]
    fn anova_kw_cancellation() {
        let bars = lcg_bar_frame(672, 5, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"buckets_via": "hour_of_day"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        AnovaKruskalScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn anova_kw_n_zero_emits_scan_error() {
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
        let req = sample_request(serde_json::json!({"buckets_via": "hour_of_day"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = AnovaKruskalScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    /// If `buckets_via` produces fewer than 2 non-empty groups after the
    /// `min_obs` filter, the test is degenerate → `ScanError::Kernel`. Build a
    /// 3-bar series all on hour 0 with `min_obs=1` → all returns in bucket 0;
    /// only 1 non-empty group.
    #[test]
    fn anova_kw_single_group_emits_scan_error() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..4).map(|i| start + Duration::minutes(i)).collect();
        let close = vec![1.0_f64, 1.1, 1.2, 1.3];
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
            tick_volume: vec![1.0; 4],
        };
        let mut sink = VecSink::new();
        let req = sample_request(
            serde_json::json!({"buckets_via": "hour_of_day", "min_obs_per_group": 1}),
        );
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = AnovaKruskalScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => {
                assert!(
                    msg.contains("need >= 2 non-empty groups"),
                    "msg={msg}"
                );
            }
            other => panic!("got {other:?}"),
        }
    }
}

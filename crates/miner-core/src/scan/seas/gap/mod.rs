//! `OvernightGapScan` — SEAS overnight / session-gap statistics (RAD-3840).
//!
//! Scout ranking RAD-3548 ★★★★ (Sharpe 2.38 SPX; CO-OC across FX). Identifies
//! session/overnight gaps (close→next-open jump) and reports the gap-size
//! distribution plus **gap-fill probability** conditioned on direction × size
//! bucket. Most valuable on equity-index / commodity CFDs; FX trades ~24×5 so
//! true overnight gaps are sparse — when that happens the finding trips the
//! `sparse_gaps` flag rather than emitting weak candidates (RAD-3840 AC-3).
//!
//! ## D4-02 contract
//!
//! - `id = "seas.gap.overnight"`, `version = 1`, `arity = ScanArity::Single`.
//! - `param_schema` (all optional): `boundary_gap_minutes` (session/day-boundary
//!   definition — inter-bar minute delta above which a gap is recognised;
//!   default `1.5 × timeframe`), `size_bucket_edges` (ascending positive edges
//!   over `|relative gap|`; default `[5e-4, 1e-3, 2e-3]`), `min_gap_threshold`
//!   (minimum `|relative gap|`; default 0), `resolution_hint` (advisory bar
//!   resolution string), `fill_lookahead_bars` (default 48), `min_obs_per_bucket`
//!   (default 5), `hold_floor_bars` (default 12), `sparse_gap_min_count`
//!   (default 20).
//! - `effect.metric = "overnight_gap_fill_rate"`, `effect.value` = overall fill
//!   rate (filled / total; 0 when no gaps). `effect.extra` carries `gap_count`,
//!   `gap_size_quantiles` (+ `gap_size_quantile_probs`), `size_bucket_edges`,
//!   `bucket_labels`, the direction×bucket count / fill-count / fill-prob
//!   matrices (`up_*` / `down_*`), `median_bars_to_fill`, and the
//!   `hold_floor_caveat` / `sparse_gaps` flags (1.0 = true).
//! - `raw.series.{gap_sizes, gap_directions, gap_filled, gap_bars_to_fill,
//!   timestamps_ms}` — one entry per detected gap event.
//!
//! Caveats surfaced as flags (per the research): `hold_floor_caveat` trips when
//! the median bars-to-fill is below the 12-bar arena floor (a gap-fill trade
//! holds too few bars at this resolution — build at a finer resolution);
//! `sparse_gaps` trips when fewer than `sparse_gap_min_count` gaps were seen.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use crate::findings::{
    Base64Bytes, DataSlice, Dtype, Effect, EffectSize, Finding, FindingSink, Raw, RawArray,
    ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

use kernel::GapConfig;

/// SEAS overnight / session-gap statistics scan.
pub struct OvernightGapScan;

const SCAN_ID: &str = "seas.gap.overnight";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "overnight_gap_fill_rate";

const DEFAULT_MIN_OBS_PER_BUCKET: i64 = 5;
const DEFAULT_FILL_LOOKAHEAD_BARS: i64 = 48;
const DEFAULT_HOLD_FLOOR_BARS: i64 = 12;
const DEFAULT_SPARSE_GAP_MIN_COUNT: i64 = 20;
const DEFAULT_SIZE_BUCKET_EDGES: [f64; 3] = [0.0005, 0.001, 0.002];
/// DOS guard — a pathological `size_bucket_edges` array. 64 is far beyond any
/// realistic bucket scheme (the default is 3 edges / 4 buckets).
const MAX_SIZE_BUCKET_EDGES: usize = 64;
/// Quantile probabilities reported for the `|gap size|` distribution.
const GAP_SIZE_QUANTILE_PROBS: [f64; 5] = [0.1, 0.25, 0.5, 0.75, 0.9];
/// Advisory bar-resolution hints accepted by `resolution_hint`.
const RESOLUTION_HINTS: [&str; 5] = ["5m", "10m", "15m", "1h", "1d"];

impl Scan for OvernightGapScan {
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
                "boundary_gap_minutes": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Session/day-boundary definition: an inter-bar open-timestamp delta (minutes) STRICTLY GREATER than this marks a gap boundary. Defaults to 1.5 x the timeframe."
                },
                "size_bucket_edges": {
                    "type": "array",
                    "maxItems": MAX_SIZE_BUCKET_EDGES,
                    "items": { "type": "number", "exclusiveMinimum": 0 },
                    "description": "Ascending, strictly-positive edges over |relative gap| (close->next-open jump / prior close). num_buckets = edges + 1. Default [5e-4, 1e-3, 2e-3]."
                },
                "min_gap_threshold": {
                    "type": "number",
                    "minimum": 0,
                    "description": "Minimum |relative gap| for a boundary to count as a gap event; defaults to 0 (any non-zero jump)."
                },
                "resolution_hint": {
                    "type": "string",
                    "enum": ["5m", "10m", "15m", "1h", "1d"],
                    "description": "Advisory bar resolution. Overnight-gap trades hold ~2 bars; the scan is most useful at 15-min / 30-min bars. Advisory only — the data-driven hold_floor_caveat reflects the actual median bars-to-fill."
                },
                "fill_lookahead_bars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Forward window (bars, inclusive of the post-gap bar) over which gap-fill is evaluated; defaults to 48."
                },
                "min_obs_per_bucket": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Direction x size-bucket cells with fewer gaps than this emit NaN fill-probability; defaults to 5."
                },
                "hold_floor_bars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Arena minimum-hold floor (bars). hold_floor_caveat trips when the median bars-to-fill is below this; defaults to 12."
                },
                "sparse_gap_min_count": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "sparse_gaps trips when fewer than this many gaps were detected; defaults to 20."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[
                "bucket_labels",
                "down_counts",
                "down_fill_counts",
                "down_fill_prob",
                "gap_count",
                "gap_size_quantile_probs",
                "gap_size_quantiles",
                "hold_floor_caveat",
                "median_bars_to_fill",
                "size_bucket_edges",
                "sparse_gaps",
                "up_counts",
                "up_fill_counts",
                "up_fill_prob",
            ],
            raw_series_keys: &[
                "gap_bars_to_fill",
                "gap_directions",
                "gap_filled",
                "gap_sizes",
                "timestamps_ms",
            ],
        }
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::similar_names,
        reason = "Scan::run is the linear param-resolve + detect + envelope-build path (Pattern A); count/index -> f64/u64 casts are bounded by the bar count and lossless on 64-bit targets; up_*/down_* parallel arrays are intentionally similarly named"
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

        let n_bars = ctx.bars.close.len();
        if n_bars < 2 {
            return Err(ScanError::Kernel(format!(
                "seas.gap.overnight: need at least 2 bars; got n={n_bars}"
            )));
        }

        // ---- resolve params -------------------------------------------------
        validate_resolution_hint(req)?;
        let edges = resolve_edges(req)?;
        let min_gap_threshold = resolve_min_gap_threshold(req)?;
        let boundary_gap_minutes = resolve_boundary_minutes(req)?;
        let fill_lookahead_bars =
            resolve_pos_int(req, "fill_lookahead_bars", DEFAULT_FILL_LOOKAHEAD_BARS)?;
        let min_obs = resolve_pos_int(req, "min_obs_per_bucket", DEFAULT_MIN_OBS_PER_BUCKET)?;
        let hold_floor_bars = resolve_pos_int(req, "hold_floor_bars", DEFAULT_HOLD_FLOOR_BARS)?;
        let sparse_gap_min_count =
            resolve_pos_int(req, "sparse_gap_min_count", DEFAULT_SPARSE_GAP_MIN_COUNT)?;

        let cfg = GapConfig {
            boundary_gap_minutes,
            min_gap_threshold,
            size_bucket_edges: edges.clone(),
            fill_lookahead_bars,
        };
        let num_buckets = cfg.num_buckets();

        // ---- detect ---------------------------------------------------------
        let events = kernel::detect_gaps(
            &ctx.bars.ts_open_utc,
            &ctx.bars.open,
            &ctx.bars.high,
            &ctx.bars.low,
            &ctx.bars.close,
            &cfg,
        );

        // ---- aggregate (direction x size bucket) ----------------------------
        let mut up_counts = vec![0_u64; num_buckets];
        let mut down_counts = vec![0_u64; num_buckets];
        let mut up_fill = vec![0_u64; num_buckets];
        let mut down_fill = vec![0_u64; num_buckets];
        for e in &events {
            let b = e.bucket.min(num_buckets - 1);
            if e.direction > 0 {
                up_counts[b] += 1;
                if e.filled() {
                    up_fill[b] += 1;
                }
            } else {
                down_counts[b] += 1;
                if e.filled() {
                    down_fill[b] += 1;
                }
            }
        }
        let up_prob = fill_prob(&up_counts, &up_fill, min_obs);
        let down_prob = fill_prob(&down_counts, &down_fill, min_obs);

        // ---- |gap size| distribution ----------------------------------------
        let mut abs_sizes: Vec<f64> = events.iter().map(|e| e.size.abs()).collect();
        abs_sizes.sort_by(f64::total_cmp);
        let quantiles: Vec<f64> = GAP_SIZE_QUANTILE_PROBS
            .iter()
            .map(|&p| {
                if abs_sizes.is_empty() {
                    f64::NAN
                } else {
                    kernel::linear_quantile(&abs_sizes, p)
                }
            })
            .collect();

        // ---- hold-floor caveat (data-driven median bars-to-fill) ------------
        let mut fill_bars: Vec<f64> = events
            .iter()
            .filter_map(|e| e.bars_to_fill.map(|x| x as f64))
            .collect();
        fill_bars.sort_by(f64::total_cmp);
        let median_bars_to_fill = if fill_bars.is_empty() {
            f64::NAN
        } else {
            kernel::linear_quantile(&fill_bars, 0.5)
        };

        // ---- scalar summary + flags -----------------------------------------
        let gap_total = events.len();
        let filled_total = events.iter().filter(|e| e.filled()).count();
        let value = if gap_total == 0 {
            0.0
        } else {
            filled_total as f64 / gap_total as f64
        };
        let sparse_gaps = gap_total < sparse_gap_min_count;
        let hold_floor_caveat =
            median_bars_to_fill.is_finite() && median_bars_to_fill < hold_floor_bars as f64;

        // ---- effect.extra ---------------------------------------------------
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("bucket_labels".into(), encode_labels(&edges)?);
        extra.insert(
            "down_counts".into(),
            f64_slice_to_raw_array(&u64_to_f64(&down_counts)),
        );
        extra.insert(
            "down_fill_counts".into(),
            f64_slice_to_raw_array(&u64_to_f64(&down_fill)),
        );
        extra.insert("down_fill_prob".into(), f64_slice_to_raw_array(&down_prob));
        extra.insert(
            "gap_count".into(),
            f64_slice_to_raw_array(&[gap_total as f64]),
        );
        extra.insert(
            "gap_size_quantile_probs".into(),
            f64_slice_to_raw_array(&GAP_SIZE_QUANTILE_PROBS),
        );
        extra.insert(
            "gap_size_quantiles".into(),
            f64_slice_to_raw_array(&quantiles),
        );
        extra.insert(
            "hold_floor_caveat".into(),
            f64_slice_to_raw_array(&[bool_f64(hold_floor_caveat)]),
        );
        extra.insert(
            "median_bars_to_fill".into(),
            f64_slice_to_raw_array(&[median_bars_to_fill]),
        );
        extra.insert("size_bucket_edges".into(), f64_slice_to_raw_array(&edges));
        extra.insert(
            "sparse_gaps".into(),
            f64_slice_to_raw_array(&[bool_f64(sparse_gaps)]),
        );
        extra.insert(
            "up_counts".into(),
            f64_slice_to_raw_array(&u64_to_f64(&up_counts)),
        );
        extra.insert(
            "up_fill_counts".into(),
            f64_slice_to_raw_array(&u64_to_f64(&up_fill)),
        );
        extra.insert("up_fill_prob".into(), f64_slice_to_raw_array(&up_prob));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value,
            p_value: None,
            n: Some(gap_total as u64),
            ci95: None,
            effect_size: Some(EffectSize {
                kind: "fill_rate".to_string(),
                value,
            }),
            extra,
        };

        // ---- raw.series (one entry per gap event) ---------------------------
        let gap_sizes: Vec<f64> = events.iter().map(|e| e.size).collect();
        let gap_directions: Vec<f64> = events.iter().map(|e| f64::from(e.direction)).collect();
        let gap_filled: Vec<f64> = events.iter().map(|e| bool_f64(e.filled())).collect();
        let gap_bars_to_fill: Vec<f64> = events
            .iter()
            .map(|e| e.bars_to_fill.map_or(f64::NAN, |x| x as f64))
            .collect();
        let timestamps_ms: Vec<f64> = events.iter().map(|e| e.ts_open_ms as f64).collect();

        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert(
            "gap_bars_to_fill".into(),
            f64_slice_to_raw_array(&gap_bars_to_fill),
        );
        series.insert(
            "gap_directions".into(),
            f64_slice_to_raw_array(&gap_directions),
        );
        series.insert("gap_filled".into(), f64_slice_to_raw_array(&gap_filled));
        series.insert("gap_sizes".into(), f64_slice_to_raw_array(&gap_sizes));
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

#[allow(
    clippy::cast_precision_loss,
    reason = "counts are bounded by the gap count; realistic gap counts fit in f64's 52-bit mantissa"
)]
fn u64_to_f64(v: &[u64]) -> Vec<f64> {
    v.iter().map(|&x| x as f64).collect()
}

fn bool_f64(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// Per-bucket fill probability `fills / counts`; `NaN` when a cell has fewer
/// than `min_obs` gaps.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "counts are bounded by the gap count; u64 -> usize / f64 are lossless on 64-bit targets for realistic counts"
)]
fn fill_prob(counts: &[u64], fills: &[u64], min_obs: usize) -> Vec<f64> {
    counts
        .iter()
        .zip(fills.iter())
        .map(|(&c, &f)| {
            if (c as usize) < min_obs {
                f64::NAN
            } else {
                f as f64 / c as f64
            }
        })
        .collect()
}

/// Build the human-readable size-bucket labels and encode them as a UTF-8
/// JSON-array byte `RawArray` (T-04-09-03 convention — consumers decode the
/// bytes as UTF-8 then parse as JSON).
fn encode_labels(edges: &[f64]) -> Result<RawArray, ScanError> {
    let labels = bucket_labels(edges);
    let bytes = serde_json::to_vec(&labels).map_err(|e| ScanError::Kernel(e.to_string()))?;
    let len = u64::try_from(bytes.len())
        .map_err(|_| ScanError::Kernel("bucket_labels: byte length exceeds u64".into()))?;
    Ok(RawArray {
        data: Base64Bytes(bytes),
        shape: vec![len],
        dtype: Dtype::F64,
    })
}

fn bucket_labels(edges: &[f64]) -> Vec<String> {
    if edges.is_empty() {
        return vec!["all".to_string()];
    }
    let mut out = Vec::with_capacity(edges.len() + 1);
    out.push(format!("<{}", edges[0]));
    for w in edges.windows(2) {
        out.push(format!("{}..{}", w[0], w[1]));
    }
    out.push(format!(">={}", edges[edges.len() - 1]));
    out
}

fn validate_resolution_hint(req: &ScanRequest) -> Result<(), ScanError> {
    if let Some(v) = req.resolved_params.get("resolution_hint") {
        let s = v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!("resolution_hint must be a string; got {v}"))
        })?;
        if !RESOLUTION_HINTS.contains(&s) {
            return Err(ScanError::Kernel(format!(
                "resolution_hint must be one of {RESOLUTION_HINTS:?}; got {s:?}"
            )));
        }
    }
    Ok(())
}

fn resolve_edges(req: &ScanRequest) -> Result<Vec<f64>, ScanError> {
    let raw = req.resolved_params.get("size_bucket_edges");
    let arr = match raw {
        None => return Ok(DEFAULT_SIZE_BUCKET_EDGES.to_vec()),
        Some(v) => v.as_array().ok_or_else(|| {
            ScanError::Kernel(format!("size_bucket_edges must be a JSON array; got {v}"))
        })?,
    };
    if arr.len() > MAX_SIZE_BUCKET_EDGES {
        return Err(ScanError::Kernel(format!(
            "size_bucket_edges too large: {} > {MAX_SIZE_BUCKET_EDGES}",
            arr.len()
        )));
    }
    let mut out: Vec<f64> = Vec::with_capacity(arr.len());
    for (i, e) in arr.iter().enumerate() {
        let x = e
            .as_f64()
            .ok_or_else(|| ScanError::Kernel(format!("size_bucket_edges[{i}] must be a number")))?;
        if !x.is_finite() || x <= 0.0 {
            return Err(ScanError::Kernel(format!(
                "size_bucket_edges[{i}] must be finite and > 0; got {x}"
            )));
        }
        if let Some(&prev) = out.last() {
            if x <= prev {
                return Err(ScanError::Kernel(format!(
                    "size_bucket_edges must be strictly ascending; {x} <= {prev} at index {i}"
                )));
            }
        }
        out.push(x);
    }
    Ok(out)
}

fn resolve_min_gap_threshold(req: &ScanRequest) -> Result<f64, ScanError> {
    match req.resolved_params.get("min_gap_threshold") {
        None => Ok(0.0),
        Some(v) => {
            let x = v.as_f64().ok_or_else(|| {
                ScanError::Kernel(format!("min_gap_threshold must be a number; got {v}"))
            })?;
            if !x.is_finite() || x < 0.0 {
                return Err(ScanError::Kernel(format!(
                    "min_gap_threshold must be finite and >= 0; got {x}"
                )));
            }
            Ok(x)
        }
    }
}

fn resolve_boundary_minutes(req: &ScanRequest) -> Result<i64, ScanError> {
    if let Some(v) = req.resolved_params.get("boundary_gap_minutes") {
        let x = v.as_i64().ok_or_else(|| {
            ScanError::Kernel(format!("boundary_gap_minutes must be an integer; got {v}"))
        })?;
        if x < 1 {
            return Err(ScanError::Kernel(format!(
                "boundary_gap_minutes must be >= 1; got {x}"
            )));
        }
        Ok(x)
    } else {
        // Default: 1.5 x the timeframe (integer floor). A within-session bar
        // delta equals the timeframe (<= this), while any skipped bar /
        // overnight / weekend break exceeds it.
        let tf_min = req.timeframe.duration().num_minutes();
        Ok(tf_min * 3 / 2)
    }
}

fn resolve_pos_int(req: &ScanRequest, key: &str, default: i64) -> Result<usize, ScanError> {
    let v: i64 = match req.resolved_params.get(key) {
        Some(v) => v
            .as_i64()
            .ok_or_else(|| ScanError::Kernel(format!("{key} must be an integer; got {v}")))?,
        None => default,
    };
    if v < 1 {
        return Err(ScanError::Kernel(format!("{key} must be >= 1; got {v}")));
    }
    usize::try_from(v).map_err(|_| ScanError::Kernel(format!("{key} out of range for usize: {v}")))
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

    /// 6 sessions x 2 bars (15m) with engineered overnight gaps. Each session's
    /// second bar closes at 100.0 so every gap is measured against 100.0; the
    /// gap-open bar's high/low controls whether it fills on bar 1.
    #[allow(clippy::too_many_lines)]
    fn engineered_gap_frame() -> BarFrame {
        let mut ts: Vec<DateTime<Utc>> = Vec::new();
        for day in 1..=6 {
            let d = Utc.with_ymd_and_hms(2024, 1, day, 0, 0, 0).unwrap();
            ts.push(d);
            ts.push(d + Duration::minutes(15));
        }
        // (open, high, low, close) per bar. Index 0,2,4,.. are session A bars.
        let bars: [(f64, f64, f64, f64); 12] = [
            (100.0, 100.05, 99.95, 100.0),    // s0.A
            (100.0, 100.05, 99.95, 100.0),    // s0.B
            (100.15, 100.20, 99.99, 100.15),  // s1.A up 0.0015 -> fills
            (100.0, 100.05, 99.95, 100.0),    // s1.B
            (100.30, 100.40, 100.20, 100.30), // s2.A up 0.003 -> NOT filled
            (100.0, 100.05, 99.95, 100.0),    // s2.B
            (99.85, 100.01, 99.80, 99.85),    // s3.A down 0.0015 -> fills
            (100.0, 100.05, 99.95, 100.0),    // s3.B
            (99.70, 100.02, 99.60, 99.70),    // s4.A down 0.003 -> fills
            (100.0, 100.05, 99.95, 100.0),    // s4.B
            (100.07, 100.10, 99.95, 100.07),  // s5.A up 0.0007 -> fills
            (100.0, 100.05, 99.95, 100.0),    // s5.B
        ];
        BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts.clone(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
            open: bars.iter().map(|b| b.0).collect(),
            high: bars.iter().map(|b| b.1).collect(),
            low: bars.iter().map(|b| b.2).collect(),
            close: bars.iter().map(|b| b.3).collect(),
            tick_volume: vec![1.0; 12],
        }
    }

    fn sample_request(params: serde_json::Value) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 8, 0, 0, 0).unwrap();
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

    #[test]
    fn id_version_arity() {
        let s = OvernightGapScan;
        assert_eq!(s.id(), "seas.gap.overnight");
        assert_eq!(s.version(), 1);
        assert_eq!(s.arity(), ScanArity::Single);
    }

    #[test]
    fn param_schema_shape() {
        let schema = OvernightGapScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["size_bucket_edges"]["type"], "array");
        assert_eq!(schema["properties"]["min_gap_threshold"]["type"], "number");
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn finding_fields_keys_are_emitted() {
        let bars = engineered_gap_frame();
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({
            "min_obs_per_bucket": 1, "fill_lookahead_bars": 1, "sparse_gap_min_count": 3
        }));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OvernightGapScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let shape = OvernightGapScan.finding_fields();
        for key in shape.effect_extra_keys {
            assert!(r.effect.extra.contains_key(*key), "extra[{key}] missing");
        }
        let raw = r.raw.as_ref().expect("raw present");
        for key in shape.raw_series_keys {
            assert!(raw.series.contains_key(*key), "series[{key}] missing");
        }
    }

    /// RAD-3840 AC-2 — engineered open-gaps recover the distribution and a
    /// fill-probability matching the construction.
    #[test]
    fn engineered_gaps_recover_distribution_and_fill_prob() {
        let bars = engineered_gap_frame();
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({
            "min_obs_per_bucket": 1, "fill_lookahead_bars": 1, "sparse_gap_min_count": 3
        }));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OvernightGapScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        assert_eq!(findings.len(), 1);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "seas.gap.overnight@1");
        assert_eq!(r.effect.metric, "overnight_gap_fill_rate");
        // 5 gaps, 4 filled -> fill rate 0.8.
        assert_eq!(r.effect.n, Some(5));
        assert!((r.effect.value - 0.8).abs() < 1e-12);
        assert_eq!(decode_f64(&r.effect.extra, "gap_count"), vec![5.0]);
        // Direction x bucket counts (4 buckets: <5e-4, 5e-4..1e-3, 1e-3..2e-3, >=2e-3).
        assert_eq!(
            decode_f64(&r.effect.extra, "up_counts"),
            vec![0.0, 1.0, 1.0, 1.0]
        );
        assert_eq!(
            decode_f64(&r.effect.extra, "down_counts"),
            vec![0.0, 0.0, 1.0, 1.0]
        );
        assert_eq!(
            decode_f64(&r.effect.extra, "up_fill_counts"),
            vec![0.0, 1.0, 1.0, 0.0]
        );
        assert_eq!(
            decode_f64(&r.effect.extra, "down_fill_counts"),
            vec![0.0, 0.0, 1.0, 1.0]
        );
        // up bucket3 had 1 gap, 0 fills -> prob 0.0; bucket0 had 0 -> NaN.
        let up_prob = decode_f64(&r.effect.extra, "up_fill_prob");
        assert!(up_prob[0].is_nan());
        assert!((up_prob[1] - 1.0).abs() < 1e-12);
        assert!((up_prob[2] - 1.0).abs() < 1e-12);
        assert!((up_prob[3] - 0.0).abs() < 1e-12);
        // Caveats: median bars-to-fill is 1 (< 12) -> hold_floor caveat; 5 gaps
        // >= sparse threshold 3 -> NOT sparse.
        assert_eq!(
            decode_f64(&r.effect.extra, "median_bars_to_fill"),
            vec![1.0]
        );
        assert_eq!(decode_f64(&r.effect.extra, "hold_floor_caveat"), vec![1.0]);
        assert_eq!(decode_f64(&r.effect.extra, "sparse_gaps"), vec![0.0]);
        // Raw series carry one entry per gap.
        let raw = r.raw.as_ref().expect("raw");
        assert_eq!(
            decode_f64(&raw.series, "gap_directions"),
            vec![1.0, 1.0, -1.0, -1.0, 1.0]
        );
        assert_eq!(
            decode_f64(&raw.series, "gap_filled"),
            vec![1.0, 0.0, 1.0, 1.0, 1.0]
        );
    }

    /// RAD-3840 AC-3 — a continuous (gapless) series emits one curated finding
    /// with the sparse-gaps flag and zero gaps; never a spurious gap.
    #[test]
    fn continuous_series_flags_sparse_no_spurious_gaps() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let n = 96; // 24h of 15m bars, all contiguous.
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| start + Duration::minutes(15 * i as i64))
            .collect();
        let close: Vec<f64> = (0..n).map(|i| 100.0 + (i as f64) * 0.01).collect();
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts.clone(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
            open: close.clone(),
            high: close.iter().map(|c| c + 0.05).collect(),
            low: close.iter().map(|c| c - 0.05).collect(),
            close: close.clone(),
            tick_volume: vec![1.0; n as usize],
        };
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"min_obs_per_bucket": 1}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        OvernightGapScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink(&sink);
        assert_eq!(findings.len(), 1, "exactly one curated finding");
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.effect.n, Some(0));
        assert_eq!(decode_f64(&r.effect.extra, "gap_count"), vec![0.0]);
        assert_eq!(decode_f64(&r.effect.extra, "sparse_gaps"), vec![1.0]);
        // value is finite (0.0) so the envelope round-trips through serde.
        assert!((r.effect.value - 0.0).abs() < 1e-12);
        for key in ["up_counts", "down_counts"] {
            assert!(decode_f64(&r.effect.extra, key).iter().all(|c| *c == 0.0));
        }
    }

    #[test]
    fn cancellation_emits_nothing() {
        let bars = engineered_gap_frame();
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        OvernightGapScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn one_bar_rejected() {
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
        let err = OvernightGapScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn rejects_non_ascending_edges() {
        let bars = engineered_gap_frame();
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"size_bucket_edges": [0.002, 0.001]}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = OvernightGapScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject");
        match err {
            ScanError::Kernel(m) => assert!(m.contains("ascending"), "msg: {m}"),
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_resolution_hint() {
        let bars = engineered_gap_frame();
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"resolution_hint": "3m"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = OvernightGapScan
            .run(&ctx, &req, &mut sink)
            .expect_err("reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn bucket_labels_default() {
        let labels = bucket_labels(&DEFAULT_SIZE_BUCKET_EDGES);
        assert_eq!(
            labels,
            vec!["<0.0005", "0.0005..0.001", "0.001..0.002", ">=0.002"]
        );
    }
}

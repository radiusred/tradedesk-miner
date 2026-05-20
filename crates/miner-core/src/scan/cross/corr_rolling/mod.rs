//! Rolling Pearson + Spearman correlation scans (CROSS-02).
//!
//! Two Pair-arity rolling scans sharing a single module (they share the
//! inner-join + window-iteration scaffold; only the per-window kernel call
//! differs).
//!
//! - `cross.corr.pearson_rolling@1` — rolling Pearson r over aligned log
//!   returns. Reference: hand-rolled rolling numpy.corrcoef. Tolerance 1e-10.
//! - `cross.corr.spearman_rolling@1` — rolling Spearman ρ with
//!   scipy.stats.spearmanr default `method = "average"` tie correction.
//!   Tolerance 1e-8.
//!
//! `effect.value` = last-window correlation (RESEARCH.md §1.3 trader-intuition
//! canonical pick for rolling Pair); `effect.extra` carries `{values,
//! window_starts_ms, window_length, threshold_crossings, threshold}`;
//! `raw.series` carries `{returns_a, returns_b, timestamps_ms_aligned}`
//! (D4-03 leg-labelled keys).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::primitives::time_alignment::inner_join;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// Pair-arity rolling Pearson correlation scan (CROSS-02 Pearson).
pub struct PearsonRollingScan;

/// Pair-arity rolling Spearman correlation scan (CROSS-02 Spearman).
pub struct SpearmanRollingScan;

const PEARSON_SCAN_ID: &str = "cross.corr.pearson_rolling";
const SPEARMAN_SCAN_ID: &str = "cross.corr.spearman_rolling";
const SCAN_VERSION: u32 = 1;
const PEARSON_EFFECT_METRIC: &str = "pearson_corr_last";
const SPEARMAN_EFFECT_METRIC: &str = "spearman_corr_last";

/// Default threshold for the `threshold_crossings` flag vector (RESEARCH.md
/// §Section 2 default). Users may override via `--params threshold=<f>`.
const DEFAULT_THRESHOLD: f64 = 0.7;

/// Selector for which correlation kernel the shared `run_corr_rolling` body
/// dispatches.
#[derive(Debug, Clone, Copy)]
enum CorrKind {
    Pearson,
    Spearman,
}

/// Shared `param_schema` body for both Pearson + Spearman variants
/// (window required ≥3, threshold optional default 0.7).
fn corr_param_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "window": {
                "type": "integer",
                "minimum": 3,
                "description": "Rolling-window size (number of aligned bars). Must be >= 3."
            },
            "threshold": {
                "type": "number",
                "description": "Absolute-value threshold for the threshold_crossings flag vector. Default 0.7."
            }
        },
        "required": ["window"],
        "additionalProperties": false
    })
}

/// Shared `finding_fields` shape for both variants.
///
/// Note (Rule 3 deviation from plan spec): the plan's `<behavior>` listed
/// `timestamps_ms_aligned` as the joint-timestamps raw-series key, but the
/// existing `Raw::new` D-03 invariant (`findings/mod.rs:146`) rejects any
/// `Raw` whose `series` lacks the literal key `"timestamps_ms"`. The
/// CROSS-scan body therefore emits the aligned joint-timestamps vector
/// under the canonical key `timestamps_ms` (same data, D-03 compliant) —
/// since this raw block ONLY carries aligned data, the key is unambiguous
/// without the `_aligned` suffix. Documented in SUMMARY.md as a wire-form
/// pin.
const FINDING_SHAPE: ScanFindingShape = ScanFindingShape {
    effect_extra_keys: &[
        "values",
        "window_starts_ms",
        "window_length",
        "threshold_crossings",
        "threshold",
    ],
    raw_series_keys: &["returns_a", "returns_b", "timestamps_ms"],
};

impl Scan for PearsonRollingScan {
    fn id(&self) -> &'static str {
        PEARSON_SCAN_ID
    }
    fn version(&self) -> u32 {
        SCAN_VERSION
    }
    fn arity(&self) -> ScanArity {
        ScanArity::Pair
    }
    fn param_schema(&self) -> serde_json::Value {
        corr_param_schema()
    }
    fn finding_fields(&self) -> ScanFindingShape {
        FINDING_SHAPE
    }
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        run_corr_rolling(
            CorrKind::Pearson,
            PEARSON_SCAN_ID,
            PEARSON_EFFECT_METRIC,
            ctx,
            req,
            sink,
        )
    }
}

impl Scan for SpearmanRollingScan {
    fn id(&self) -> &'static str {
        SPEARMAN_SCAN_ID
    }
    fn version(&self) -> u32 {
        SCAN_VERSION
    }
    fn arity(&self) -> ScanArity {
        ScanArity::Pair
    }
    fn param_schema(&self) -> serde_json::Value {
        corr_param_schema()
    }
    fn finding_fields(&self) -> ScanFindingShape {
        FINDING_SHAPE
    }
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        run_corr_rolling(
            CorrKind::Spearman,
            SPEARMAN_SCAN_ID,
            SPEARMAN_EFFECT_METRIC,
            ctx,
            req,
            sink,
        )
    }
}

/// Shared run body (Pattern C two-leg + Pattern D vector-output). The
/// `kind` selector picks Pearson vs Spearman at the per-window kernel call;
/// every other step is identical.
#[allow(clippy::too_many_lines)]
fn run_corr_rolling(
    kind: CorrKind,
    scan_id: &str,
    effect_metric: &str,
    ctx: &ScanCtx<'_>,
    req: &ScanRequest,
    sink: &mut dyn FindingSink,
) -> Result<(), ScanError> {
    // 1. Cancel-at-entry.
    if ctx.cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    // 2. Pair-arity borrow check.
    let (bars_a, bars_b) = ctx
        .bars_pair()
        .ok_or_else(|| ScanError::Kernel("expected Pair arity (ctx.bars_pair is None)".into()))?;

    // 3. Inner-join (CROSS-01 primitive — consumed exactly once per
    //    invocation, D4-04).
    let aligned = inner_join(bars_a, bars_b);
    if aligned.timestamps_ms.is_empty() {
        return Err(ScanError::Kernel(
            "inner join produced zero overlap between legs".into(),
        ));
    }

    // 4. Per-leg log returns AFTER alignment (ANOM-01 primitive). Both
    //    returns slices have length aligned_n - 1.
    let returns_a = log_returns(&aligned.close_a);
    let returns_b = log_returns(&aligned.close_b);
    let returns_n = returns_a.len();
    debug_assert_eq!(returns_n, returns_b.len());
    if returns_n < 3 {
        return Err(ScanError::Kernel(format!(
            "rolling correlation needs at least 3 aligned returns; got {returns_n}"
        )));
    }

    // 5. Resolve params (window required ≥3; threshold optional default 0.7).
    let window = resolve_window(req, returns_n)?;
    let threshold = resolve_threshold(req)?;

    // 6. Cancel-poll before the per-window sweep.
    if ctx.cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    // 7. Run the kernel.
    let values: Vec<f64> = match kind {
        CorrKind::Pearson => kernel::rolling_pearson(&returns_a, &returns_b, window),
        CorrKind::Spearman => kernel::rolling_spearman(&returns_a, &returns_b, window),
    };
    debug_assert_eq!(values.len(), returns_n - window + 1);

    // 8. NaN detection (zero-variance window in either leg) -> ScanError.
    if let Some(idx) = values.iter().position(|v| v.is_nan()) {
        return Err(ScanError::Kernel(format!(
            "zero-variance window at index {idx}: correlation undefined"
        )));
    }

    // 9. Cancel-poll between kernel and envelope construction.
    if ctx.cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    // 10. window_starts_ms: aligned timestamp at the START of each window.
    //     The returns array is one shorter than aligned.timestamps_ms; the
    //     return at position t corresponds to the bar at aligned index t+1.
    //     A window covering returns positions [i..i+window) starts at
    //     aligned timestamp index i+1.
    let mut window_starts_ms: Vec<f64> = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        // i+1 is safe: returns_n = aligned_n - 1; aligned_n = returns_n + 1
        // = values.len() + window; so i+1 <= values.len() <= aligned_n - 1.
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
        )]
        let v = aligned.timestamps_ms[i + 1] as f64;
        window_starts_ms.push(v);
    }

    // 11. threshold_crossings flag vector: 1.0 where |values[i]| > threshold,
    //     0.0 otherwise. Encoded as f64 so it fits the single-Dtype RawArray
    //     wire form (findings/base64_bytes.rs:75 only declares F64 in v1).
    let threshold_crossings: Vec<f64> = values
        .iter()
        .map(|v| if v.abs() > threshold { 1.0 } else { 0.0 })
        .collect();

    // 12. Effect.extra arrays.
    let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
    extra.insert("values".into(), f64_slice_to_raw_array(&values));
    extra.insert(
        "window_starts_ms".into(),
        f64_slice_to_raw_array(&window_starts_ms),
    );
    extra.insert(
        "window_length".into(),
        f64_slice_to_raw_array(&[window_as_f64(window)]),
    );
    extra.insert(
        "threshold_crossings".into(),
        f64_slice_to_raw_array(&threshold_crossings),
    );
    extra.insert("threshold".into(), f64_slice_to_raw_array(&[threshold]));

    // 13. effect.value = last-window correlation (RESEARCH §1.3 canonical).
    let last_value = *values.last().expect("values non-empty by construction");

    let effect = Effect {
        metric: effect_metric.to_string(),
        value: last_value,
        p_value: None,
        #[allow(
            clippy::cast_possible_truncation,
            reason = "usize <= u64 on all supported targets (Phase 1 is 64-bit only)"
        )]
        n: Some(values.len() as u64),
        ci95: None,
        effect_size: None,
        extra,
    };

    // 14. raw.series — leg-labelled keys + aligned timestamps (Pattern C
    //     D4-03). The aligned timestamps slice carries the joint epoch-ms
    //     index returned by inner_join; per-return timestamps_ms_aligned
    //     drops the first entry (the leading bar that has no return).
    let timestamps_ms_aligned: Vec<f64> = aligned
        .timestamps_ms
        .iter()
        .skip(1)
        .map(|ms| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
            )]
            let v = *ms as f64;
            v
        })
        .collect();
    debug_assert_eq!(timestamps_ms_aligned.len(), returns_n);

    // D-03 invariant: Raw::new requires a `timestamps_ms` key. Plan's
    // leg-labelled `timestamps_ms_aligned` would violate the invariant —
    // we use the canonical key under the same semantic (these ARE the
    // aligned joint timestamps, one per return entry).
    let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
    series.insert("returns_a".into(), f64_slice_to_raw_array(&returns_a));
    series.insert("returns_b".into(), f64_slice_to_raw_array(&returns_b));
    series.insert(
        "timestamps_ms".into(),
        f64_slice_to_raw_array(&timestamps_ms_aligned),
    );
    let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;

    // 15. D4-03: two-leg sources Vec. Iterates `req.instruments` in order;
    //     the per-leg source_id reads from each leg's BarFrame so the
    //     wire-form provenance survives cross-reader scans (not relevant
    //     v1 — single reader — but the field shape is forward-compatible).
    let per_leg_source_ids = (bars_a.source_id.clone(), bars_b.source_id.clone());
    let mut sources: Vec<Source> = Vec::with_capacity(2);
    if let (Some(spec_a), Some(spec_b)) = (req.instruments.first(), req.instruments.get(1)) {
        sources.push(Source {
            source_id: per_leg_source_ids.0,
            symbol: spec_a.symbol.clone(),
            side: spec_a.side.as_str().to_string(),
            timeframe: req.timeframe.as_str().to_string(),
        });
        sources.push(Source {
            source_id: per_leg_source_ids.1,
            symbol: spec_b.symbol.clone(),
            side: spec_b.side.as_str().to_string(),
            timeframe: req.timeframe.as_str().to_string(),
        });
    } else {
        return Err(ScanError::Kernel(format!(
            "{scan_id}: expected 2 instruments; got {}",
            req.instruments.len()
        )));
    }

    let result = ResultFinding {
        schema_version: 1,
        scan_id_at_version: format!("{scan_id}@{SCAN_VERSION}"),
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

/// Resolve the `window` parameter — required integer ≥ 3 and ≤ `returns_n`.
fn resolve_window(req: &ScanRequest, returns_n: usize) -> Result<usize, ScanError> {
    let raw = req
        .resolved_params
        .get("window")
        .ok_or_else(|| ScanError::Kernel("rolling correlation: window param required".into()))?;
    let window_i64 = raw
        .as_i64()
        .ok_or_else(|| ScanError::Kernel(format!("window must be an integer; got {raw}")))?;
    if window_i64 < 3 {
        return Err(ScanError::Kernel(format!(
            "window must be >= 3; got {window_i64}"
        )));
    }
    let window_us = usize::try_from(window_i64)
        .map_err(|_| ScanError::Kernel(format!("window out of range for usize: {window_i64}")))?;
    if window_us > returns_n {
        return Err(ScanError::Kernel(format!(
            "window must be <= aligned_n (= {returns_n} returns); got window={window_us}"
        )));
    }
    Ok(window_us)
}

/// Resolve the optional `threshold` parameter (default 0.7).
fn resolve_threshold(req: &ScanRequest) -> Result<f64, ScanError> {
    match req.resolved_params.get("threshold") {
        None => Ok(DEFAULT_THRESHOLD),
        Some(v) => v
            .as_f64()
            .ok_or_else(|| ScanError::Kernel(format!("threshold must be a number; got {v}"))),
    }
}

/// Convert a usize window to f64 for the `effect.extra["window_length"]`
/// 1-element array. Bounded above by `returns_n`; fits easily in f64.
#[allow(
    clippy::cast_precision_loss,
    reason = "window is bounded by returns_n; realistic values are << 2^52"
)]
fn window_as_f64(window: usize) -> f64 {
    window as f64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::cast_lossless, clippy::float_cmp)]
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

    // -----------------------------------------------------------------------
    // Fixtures — two-leg synthetic BarFrames with shared timestamps so the
    // inner-join is full-overlap.
    // -----------------------------------------------------------------------

    fn blake3_hex_zero() -> Blake3Hex {
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

    /// Build a synthetic `BarFrame` from a slice of (ts, close) pairs.
    fn build_bars(symbol: &str, ts: &[DateTime<Utc>], closes: &[f64]) -> BarFrame {
        assert_eq!(ts.len(), closes.len());
        BarFrame {
            source_id: "dukascopy".into(),
            symbol: symbol.into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts.to_vec(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
            open: closes.to_vec(),
            high: closes.iter().map(|c| c + 0.001).collect(),
            low: closes.iter().map(|c| c - 0.001).collect(),
            close: closes.to_vec(),
            tick_volume: vec![1.0; ts.len()],
        }
    }

    /// Two-leg fixture sharing timestamps. Closes follow an LCG so log
    /// returns are non-trivial.
    fn two_leg_fixture(n: usize, seed_a: u64, seed_b: u64) -> (BarFrame, BarFrame) {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        let closes_a = lcg_closes(n, seed_a);
        let closes_b = lcg_closes(n, seed_b);
        (
            build_bars("EURUSD", &ts, &closes_a),
            build_bars("GBPUSD", &ts, &closes_b),
        )
    }

    #[allow(clippy::cast_possible_truncation)]
    fn lcg_closes(n: usize, seed: u64) -> Vec<f64> {
        let mut s = seed as u32;
        let mut closes = Vec::with_capacity(n);
        for _ in 0..n {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frac = f64::from(s) / f64::from(u32::MAX);
            closes.push(1.0 + frac);
        }
        closes
    }

    fn sample_request_with_params(params: serde_json::Value, scan_id: &str) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: scan_id.into(),
            version: SCAN_VERSION,
            instruments: vec![
                InstrumentSpec {
                    symbol: "EURUSD".into(),
                    side: Side::Bid,
                },
                InstrumentSpec {
                    symbol: "GBPUSD".into(),
                    side: Side::Bid,
                },
            ],
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

    fn make_ctx<'a>(
        bars_a: &'a BarFrame,
        bars_b: &'a BarFrame,
        cancel: Arc<AtomicBool>,
    ) -> ScanCtx<'a> {
        ScanCtx {
            bars: bars_a,
            bars_pair: Some((bars_a, bars_b)),
            gap_manifest: None,
            run_id: RunId::new(),
            code_revision: "abc1234",
            cancel,
            sleep_after_first_finding_ms: None,
        }
    }

    fn parse_findings(sink: &VecSink) -> Vec<Finding> {
        sink.0
            .split(|b| *b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<Finding>(line).expect("parse"))
            .collect()
    }

    // -----------------------------------------------------------------------
    // PearsonRollingScan — Behavior Tests
    // -----------------------------------------------------------------------

    #[test]
    fn pearson_rolling_id_and_version() {
        let s = PearsonRollingScan;
        assert_eq!(s.id(), "cross.corr.pearson_rolling");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn pearson_rolling_arity_is_pair() {
        assert_eq!(PearsonRollingScan.arity(), ScanArity::Pair);
        assert_eq!(PearsonRollingScan.arity().expected_len(), 2);
    }

    #[test]
    fn pearson_rolling_param_schema() {
        let schema = PearsonRollingScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["window"]["type"], "integer");
        assert_eq!(schema["properties"]["window"]["minimum"], 3);
        assert_eq!(schema["properties"]["threshold"]["type"], "number");
        let required = schema["required"].as_array().expect("required is array");
        assert!(required.iter().any(|v| v == "window"));
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn spearman_rolling_id_and_version() {
        let s = SpearmanRollingScan;
        assert_eq!(s.id(), "cross.corr.spearman_rolling");
        assert_eq!(s.version(), 1);
        assert_eq!(s.arity(), ScanArity::Pair);
    }

    #[test]
    fn corr_rolling_invalid_window_too_small() {
        let (a, b) = two_leg_fixture(20, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 2}),
            "cross.corr.pearson_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = PearsonRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject window < 3");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn corr_rolling_invalid_window_exceeds_data() {
        let (a, b) = two_leg_fixture(10, 1, 2);
        // returns_n = 9 (aligned bars - 1); window 50 > 9 -> reject.
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 50}),
            "cross.corr.pearson_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = PearsonRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject window > returns_n");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn pearson_rolling_emits_one_result() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 5}),
            "cross.corr.pearson_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        PearsonRollingScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn pearson_rolling_result_envelope_shape() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 5}),
            "cross.corr.pearson_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        PearsonRollingScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // Envelope identity.
        assert_eq!(r.scan_id_at_version, "cross.corr.pearson_rolling@1");
        assert_eq!(r.effect.metric, "pearson_corr_last");
        // effect.extra keys (BTreeMap is sorted alphabetically).
        let keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "threshold",
                "threshold_crossings",
                "values",
                "window_length",
                "window_starts_ms",
            ]
        );
        // effect.value == last values entry.
        let values_arr = &r.effect.extra["values"];
        assert!(!values_arr.shape.is_empty());
        // effect.n == values.len() as u64.
        let values_n = values_arr.shape[0];
        assert_eq!(r.effect.n, Some(values_n));
        // raw.series keys (D4-03; D-03 rules `timestamps_ms` as the canonical
        // joint-timestamps key — see FINDING_SHAPE doc-comment for rationale).
        let raw = r.raw.as_ref().expect("raw present");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns_a", "returns_b", "timestamps_ms"]);
    }

    #[test]
    fn pearson_rolling_data_slice_sources_len_two() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 5}),
            "cross.corr.pearson_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        PearsonRollingScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // D4-03: Pair-arity scan emits sources Vec of length 2.
        assert_eq!(r.data_slice.sources.len(), 2);
        assert_eq!(r.data_slice.sources[0].symbol, "EURUSD");
        assert_eq!(r.data_slice.sources[1].symbol, "GBPUSD");
        assert_eq!(r.data_slice.sources[0].side, "bid");
        assert_eq!(r.data_slice.sources[1].side, "bid");
    }

    #[test]
    fn pearson_rolling_threshold_crossings() {
        // Build a synthetic perfect-correlation pair so every window's r is
        // ~1.0 -> threshold_crossings vector is all 1.0 at threshold 0.7.
        let n = 12;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        // b = 2*a + offset so log returns are perfectly correlated.
        let closes_a: Vec<f64> = (1..=n).map(|i| 1.0 + 0.01 * i as f64).collect();
        let closes_b: Vec<f64> = (1..=n).map(|i| 2.0 + 0.02 * i as f64).collect();
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 3, "threshold": 0.7}),
            "cross.corr.pearson_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        PearsonRollingScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // Decode threshold_crossings — every entry should be 1.0 (|r| > 0.7
        // for every window because both legs scale linearly).
        let tc = decode_f64(&r.effect.extra["threshold_crossings"]);
        for (i, v) in tc.iter().enumerate() {
            assert_eq!(*v, 1.0, "tc[{i}] = {v}; expected 1.0 (|r| > 0.7)");
        }
        // threshold scalar carries the resolved param value.
        let thr = decode_f64(&r.effect.extra["threshold"]);
        assert_eq!(thr.len(), 1);
        assert_eq!(thr[0], 0.7);
    }

    #[test]
    fn pearson_rolling_zero_variance_leg_emits_scan_error() {
        // Leg A is constant -> log returns are all 0 -> zero variance over
        // any window -> kernel returns NaN -> ScanError::Kernel.
        let n = 20;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        let closes_a = vec![1.0; n]; // constant
        let closes_b: Vec<f64> = (0..n).map(|i| 1.0 + 0.01 * i as f64).collect();
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 5}),
            "cross.corr.pearson_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = PearsonRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("zero variance must reject");
        match err {
            ScanError::Kernel(msg) => {
                assert!(msg.contains("zero-variance"), "msg: {msg}");
            }
            other => panic!("expected Kernel error; got {other:?}"),
        }
    }

    #[test]
    fn pearson_rolling_cancellation() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 5}),
            "cross.corr.pearson_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(true)));
        PearsonRollingScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel returns Ok");
        assert!(sink.0.is_empty(), "no envelope on cancel-at-entry");
    }

    #[test]
    fn spearman_rolling_handles_ties() {
        // Two-leg fixture; manually craft a leg with a tied pair to exercise
        // the rank-with-ties path.
        let n = 12;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        // Leg A monotone; leg B has a flat segment so a tied window appears
        // in the log-return space.
        let closes_a: Vec<f64> = (1..=n).map(|i| 1.0 + 0.01 * i as f64).collect();
        let mut closes_b: Vec<f64> = (1..=n).map(|i| 1.0 + 0.02 * i as f64).collect();
        // Force a tie in returns_b: closes_b[5] = closes_b[4] -> log-return at
        // position 4 = 0; also closes_b[6] = closes_b[5] -> another zero.
        closes_b[5] = closes_b[4];
        closes_b[6] = closes_b[5];
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(
            serde_json::json!({"window": 4}),
            "cross.corr.spearman_rolling",
        );
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        SpearmanRollingScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "cross.corr.spearman_rolling@1");
        // Decode values and verify every window's Spearman ρ is finite
        // (the rank machinery must handle ties without producing NaN).
        let values = decode_f64(&r.effect.extra["values"]);
        for v in values {
            assert!(v.is_finite(), "expected finite Spearman value; got {v}");
        }
    }

    /// Decode an `effect.extra[key]` `RawArray` to a Vec<f64>.
    fn decode_f64(arr: &RawArray) -> Vec<f64> {
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

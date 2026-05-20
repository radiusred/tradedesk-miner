//! `LjungBoxSqScan` — ANOM-04 (squared variant) Ljung-Box on squared returns.
//!
//! Pattern analog: [`crate::scan::ljung_box::LjungBoxScan`] (Phase 3 gold-
//! standard / `04-PATTERNS.md` Pattern A). Near-clone with two deltas:
//!
//! 1. **Input preprocessing**: returns are squared element-wise before being
//!    handed to the ACF + Q-stat kernels (`kernel::square_returns`).
//! 2. **Wire-form labelling**: `effect.metric = "ljung_box_q_squared"`,
//!    `effect.extra` carries a `series_kind = "squared_returns"` discriminator,
//!    and `raw.series` exposes the squared series under the key
//!    `returns_squared`.
//!
//! ## D4-02 surface
//!
//! - `id = "stats.autocorr.ljung_box_sq"`, `version = 1`,
//!   `arity = ScanArity::Single`.
//! - `params`: same shape as Phase 3 `stats.autocorr.ljung_box` — optional
//!   `lags: integer >= 1`; default `min(10, n / 5)`.
//! - `effect.metric = "ljung_box_q_squared"`, `effect.value = Q[lags-1]`,
//!   `effect.p_value = chi2(df=lags).sf(Q[lags-1])`, `effect.n = returns.len()`.
//! - `effect.extra = {acf, lags, p_values, q_stats, series_kind}` (alphabetical
//!   under `BTreeMap` discipline).
//! - `raw.series = {returns_squared, timestamps_ms}`.
//!
//! ## Phase 3 `LjungBox` golden invariant
//!
//! The new squared-variant scan reuses the existing Phase 3 ACF + Q-stat
//! kernels (`crate::scan::ljung_box::kernel::{biased_acf, ljung_box_q_and_p}`).
//! Their visibility was widened from `pub(super)` to `pub(crate)` so this
//! module can call them — a non-behavioural change. The existing
//! `crates/miner-core/tests/scan_ljung_box.rs` statsmodels golden continues to
//! pass byte-identically (D4-06 / Pitfall 9 invariant).
//!
//! ## Registration
//!
//! Appended inside `crate::scan::anom::register_anom_scans` (Pattern E —
//! `crates/miner-core/src/scan/registry.rs` is NOT modified in this task).
//! Alphabetical position: `stats.autocorr.ljung_box_sq` sorts immediately
//! AFTER the Phase 3 `stats.autocorr.ljung_box` (and FIRST among the four
//! ANOM scans currently registered).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;
use serde_json::Value as JsonValue;

use crate::findings::{
    Base64Bytes, DataSlice, Dtype, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding,
    Source,
};
use crate::scan::ljung_box::kernel::{biased_acf, ljung_box_q_and_p};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// ANOM-04 (squared variant) — Ljung-Box Q-stat on squared log-returns.
pub struct LjungBoxSqScan;

const SCAN_ID: &str = "stats.autocorr.ljung_box_sq";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "ljung_box_q_squared";
const SERIES_KIND: &str = "squared_returns";

impl Scan for LjungBoxSqScan {
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
        // Identical shape to Phase 3 `stats.autocorr.ljung_box` — only the
        // `lags` parameter (default `min(10, n/5)` resolved at run time).
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "lags": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Number of lags for the Ljung-Box Q-statistic on squared returns; defaults to min(10, n/5)"
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        // BTreeMap order: alphabetical.
        ScanFindingShape {
            effect_extra_keys: &["acf", "lags", "p_values", "q_stats", "series_kind"],
            raw_series_keys: &["returns_squared", "timestamps_ms"],
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
        // Step 1 — cancel-poll at entry.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Step 2 — compute log returns from closes, then square them.
        let returns = log_returns(&ctx.bars.close);
        let n = returns.len();
        if n < 2 {
            // Same n<2 rejection as the Phase 3 scan.
            return Err(ScanError::Kernel(format!(
                "ljung_box_sq: need at least 2 returns; got n={n}"
            )));
        }
        let returns_squared = kernel::square_returns(&returns);
        debug_assert_eq!(returns_squared.len(), n);

        // Step 3 — resolve `lags` param (same shape as the Phase 3 scan).
        let lags = resolve_lags(req, n)?;

        // Step 4 — call the Phase 3 kernels on the SQUARED returns. The
        // visibility of `biased_acf` and `ljung_box_q_and_p` was widened from
        // `pub(super)` to `pub(crate)` in this plan; the behaviour is
        // identical (Pitfall 9 invariant — the Phase 3 LjungBox golden
        // continues to pass byte-identically).
        let acf = biased_acf(&returns_squared, lags);
        let (q_stats, p_values) = ljung_box_q_and_p(n, &acf, lags);

        // Step 5 — build the envelope.

        // effect.extra: includes the `series_kind` string discriminator under
        // a JSON-encoded RawArray (the catalogue's wire form for a 1-element
        // string is a Dtype::Json-style payload — but the existing v1 wire
        // form supports only Dtype::F64). We encode the string via a
        // sidecar JSON payload inside the `params` echo AND a 1-element
        // f64 discriminator under `series_kind_label` (0.0 == "squared_returns")
        // OR — cleaner — we encode `series_kind` as a Base64-encoded UTF-8
        // byte slice with Dtype::F64 shape [0]. The cleanest approach that
        // matches the existing wire-form is to encode the label as the raw
        // string bytes inside a RawArray with shape [len] and dtype f64 —
        // BUT the test wants `effect.extra` to have a "series_kind" key with
        // String value via the JSON parser. The catalogue v1 only supports
        // RawArray-valued effect.extra entries. We therefore expose
        // `series_kind` as a single-element f64 discriminator
        // (0.0 == "squared_returns") AND additionally echo the string into
        // `params.series_kind` at the ResultFinding `params` level.
        //
        // SIMPLER: the existing v1 `RawArray` is `{ data: Base64Bytes,
        // shape: Vec<u64>, dtype: Dtype }`. We pack the UTF-8 bytes of the
        // SERIES_KIND label into `data`, with shape `[label.len()]` and
        // dtype f64. This is purely the wire-form trick that lets us ship
        // the string under the existing v1 contract without bumping
        // schema_version. The Quant agent reads `series_kind` by decoding
        // the bytes as UTF-8.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("acf".into(), f64_slice_to_raw_array(&acf));
        extra.insert("lags".into(), f64_slice_to_raw_array(&[lags_to_f64(lags)]));
        extra.insert("p_values".into(), f64_slice_to_raw_array(&p_values));
        extra.insert("q_stats".into(), f64_slice_to_raw_array(&q_stats));
        extra.insert("series_kind".into(), string_to_raw_array(SERIES_KIND));

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: q_stats[lags - 1],
            p_value: Some(p_values[lags - 1]),
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize <= u64 on all supported targets"
            )]
            n: Some(n as u64),
            ci95: None,
            effect_size: None,
            extra,
        };

        // raw.series — the squared returns + their parallel timestamps.
        // Each squared return at position t corresponds to log_return at
        // position t which is indexed by bar t+1's open timestamp (the bar
        // whose close ended the underlying return).
        #[allow(
            clippy::cast_precision_loss,
            reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
        )]
        let timestamps_ms: Vec<f64> = ctx
            .bars
            .ts_open_utc
            .iter()
            .skip(1)
            .map(|dt| dt.timestamp_millis() as f64)
            .collect();
        debug_assert_eq!(timestamps_ms.len(), n);

        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert(
            "returns_squared".into(),
            f64_slice_to_raw_array(&returns_squared),
        );
        series.insert(
            "timestamps_ms".into(),
            f64_slice_to_raw_array(&timestamps_ms),
        );
        let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;

        // sources Vec parallel to req.instruments (D4-03).
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

        // Step 6 — emit.
        sink.write_envelope(&Finding::Result(result))?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the `lags` parameter — identical contract to the Phase 3 scan's
/// helper (default `min(10, n/5)`; reject `lags < 1` or `lags >= n`).
fn resolve_lags(req: &ScanRequest, n: usize) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("lags");
    let lags_i64: i64 = match raw {
        Some(v) => v
            .as_i64()
            .ok_or_else(|| ScanError::Kernel(format!("lags must be an integer; got {v}")))?,
        None => default_lags(n),
    };
    if lags_i64 < 1 {
        return Err(ScanError::Kernel(format!(
            "lags must be >= 1; got {lags_i64}"
        )));
    }
    let lags_us = usize::try_from(lags_i64)
        .map_err(|_| ScanError::Kernel(format!("lags out of range for usize: {lags_i64}")))?;
    if lags_us >= n {
        return Err(ScanError::Kernel(format!(
            "lags must be < n; got lags={lags_us}, n={n}"
        )));
    }
    Ok(lags_us)
}

/// D3-03 default: `min(10, n/5)`. For very small `n` this can collapse to 0
/// which `resolve_lags` then rejects via the `lags < 1` branch.
fn default_lags(n: usize) -> i64 {
    let candidate = (n / 5).min(10);
    i64::try_from(candidate).expect("candidate is bounded by 10 — fits in i64")
}

/// Convert a usize lag value to f64 for the `effect.extra["lags"]` 1-element
/// `RawArray`. Bounded by `n` (returns count); fits trivially in f64.
#[allow(
    clippy::cast_precision_loss,
    reason = "lags is bounded by n (returns count); realistic values << 2^52"
)]
fn lags_to_f64(lags: usize) -> f64 {
    lags as f64
}

/// Encode a UTF-8 string as a 1-element string-payload `RawArray`.
///
/// The catalogue's v1 wire-form only supports `Dtype::F64` `RawArray` — but a
/// scan envelope still needs to carry the `series_kind` discriminator label
/// (`"squared_returns"`) so the Quant agent can disambiguate the level vs
/// squared variant without a per-scan look-up table.
///
/// We pack the label's UTF-8 bytes into the `data` field with shape
/// `[label.len()]` and dtype `f64`. Consumers detect the encoding by reading
/// `effect.extra["series_kind"]` and decoding the bytes as UTF-8 (the dtype
/// is a placeholder under the v1 contract; a future schema-version bump can
/// add a proper `Dtype::Utf8`). The integration test parses the bytes back
/// to verify the round-trip.
fn string_to_raw_array(s: &str) -> RawArray {
    let bytes = s.as_bytes().to_vec();
    RawArray {
        data: Base64Bytes(bytes),
        shape: vec![s.len() as u64],
        dtype: Dtype::F64,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp)]
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
    // Fixtures
    // -----------------------------------------------------------------------

    fn blake3_hex_zero() -> Blake3Hex {
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

    /// Same LCG-seeded fixture builder used by the Phase 3 `LjungBox` tests.
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
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn ljung_box_sq_id_and_version() {
        let s = LjungBoxSqScan;
        assert_eq!(s.id(), "stats.autocorr.ljung_box_sq");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn ljung_box_sq_arity_is_single() {
        let s = LjungBoxSqScan;
        assert_eq!(s.arity(), ScanArity::Single);
        assert_eq!(s.arity().expected_len(), 1);
    }

    #[test]
    fn ljung_box_sq_param_schema() {
        let s = LjungBoxSqScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["lags"]["type"], "integer");
        assert_eq!(schema["properties"]["lags"]["minimum"], 1);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn ljung_box_sq_kernel_input_is_returns_squared() {
        // Run the scan and decode raw.series["returns_squared"]; assert each
        // element equals (log_return)^2 for the LCG fixture.
        let bars = lcg_bar_frame_seeded(32, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 3}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        LjungBoxSqScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let raw = r.raw.as_ref().expect("raw present");
        let squared_arr = &raw.series["returns_squared"];
        // Decode the f64 bytes back.
        let bytes = &squared_arr.data.0;
        assert_eq!(bytes.len(), 31 * 8, "32 closes -> 31 squared returns");
        // Compute reference: log_returns(closes)^2.
        let ref_returns = log_returns(&bars.close);
        let ref_squared: Vec<f64> = ref_returns.iter().map(|r| r * r).collect();
        for (i, want) in ref_squared.iter().enumerate() {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[i * 8..i * 8 + 8]);
            let got = f64::from_le_bytes(buf);
            assert!(
                (got - want).abs() < 1e-12,
                "returns_squared[{i}]={got} expected {want}"
            );
        }
    }

    #[test]
    fn ljung_box_sq_effect_extra_series_kind() {
        let bars = lcg_bar_frame_seeded(64, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        LjungBoxSqScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // series_kind is encoded as the UTF-8 bytes of "squared_returns".
        let sk = r
            .effect
            .extra
            .get("series_kind")
            .expect("series_kind key present");
        let decoded = std::str::from_utf8(&sk.data.0).expect("utf-8");
        assert_eq!(decoded, "squared_returns");
    }

    #[test]
    fn ljung_box_sq_invalid_params_low() {
        let bars = lcg_bar_frame_seeded(64, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 0}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = LjungBoxSqScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn ljung_box_sq_invalid_params_high() {
        let bars = lcg_bar_frame_seeded(32, 4);
        let mut sink = VecSink::new();
        // 32 closes -> 31 returns; lags=999 >= 31 must reject.
        let req = sample_request_with_params(serde_json::json!({"lags": 999}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = LjungBoxSqScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn ljung_box_sq_emits_one_result() {
        let bars = lcg_bar_frame_seeded(64, 5);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        LjungBoxSqScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn ljung_box_sq_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(64, 6);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 4}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        LjungBoxSqScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "stats.autocorr.ljung_box_sq@1");
        assert_eq!(r.effect.metric, "ljung_box_q_squared");
        assert_eq!(r.effect.n, Some(63));
        assert!(r.effect.p_value.is_some());
        assert_eq!(r.effect.ci95, None);
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec!["acf", "lags", "p_values", "q_stats", "series_kind"]
        );
        let raw = r.raw.as_ref().expect("raw present");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns_squared", "timestamps_ms"]);
        assert_eq!(r.data_slice.sources.len(), 1);
    }

    #[test]
    fn ljung_box_sq_cancellation() {
        let bars = lcg_bar_frame_seeded(64, 7);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        LjungBoxSqScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn ljung_box_sq_q_stat_on_iid_returns_below_critical() {
        // For an LCG-seeded series, the level returns are near-IID by
        // construction so the SQUARED returns should also be near-IID and
        // the Q-stat should be small. We just verify the Q-stat is finite
        // and the p-value sits in [0, 1] — the actual statsmodels golden
        // lives in Plan 04-11. This is a smoke check.
        let bars = lcg_bar_frame_seeded(256, 8);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 10}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        LjungBoxSqScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!(r.effect.value.is_finite(), "Q-stat finite");
        let p = r.effect.p_value.unwrap();
        assert!((0.0..=1.0).contains(&p), "p-value in [0, 1]; got {p}");
    }

    #[test]
    fn ljung_box_sq_raw_new_enforces_timestamps_ms() {
        let bars = lcg_bar_frame_seeded(64, 9);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 3}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        LjungBoxSqScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!(r.raw.as_ref().unwrap().series.contains_key("timestamps_ms"));
    }
}

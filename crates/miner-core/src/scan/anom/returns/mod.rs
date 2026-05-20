//! `ReturnsProfileScan` — ANOM-01 callable scan dispatching log / simple /
//! intraday / overnight returns by `params.variant`.
//!
//! Pattern analog: `crate::scan::ljung_box::LjungBoxScan` (Phase 3 gold-standard
//! per `04-PATTERNS.md` Pattern A).
//!
//! ## D4-05 surface
//!
//! - `id = "stats.returns.profile"`, `version = 1`, `arity = ScanArity::Single`.
//! - One callable scan with `params.variant ∈ {"log","simple","intraday",
//!   "overnight"}` (default `"log"`) per RESEARCH §1.4 (D4-05).
//! - `effect.metric = "returns_{variant}_mean"` (dynamic per-invocation),
//!   `effect.value = arithmetic mean of returns`, `effect.n = returns.len()`.
//! - `effect.extra = {returns_vector, variant_label, mean, std, n}` per §Section 2.
//! - `raw.series = {returns, timestamps_ms}`.
//!
//! ## Registration (per-family pattern E)
//!
//! Registered by appending `r.register(Box::new(ReturnsProfileScan));` to
//! `crate::scan::anom::register_anom_scans` — `crates/miner-core/src/scan/registry.rs`
//! is NOT modified in Plan 04-03.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

use kernel::ReturnsVariant;

/// ANOM-01 — log/simple/intraday/overnight return profile scan.
///
/// Stateless unit struct following the Phase 3 `LjungBoxScan` pattern (no
/// fields; one trait impl).
pub struct ReturnsProfileScan;

/// Wire-form scan id — D4-05 / RESEARCH §1.4 / Plan 04-03.
const SCAN_ID: &str = "stats.returns.profile";

/// Scan version. Bumps on output-shape change.
const SCAN_VERSION: u32 = 1;

impl Scan for ReturnsProfileScan {
    fn id(&self) -> &'static str {
        SCAN_ID
    }

    fn version(&self) -> u32 {
        SCAN_VERSION
    }

    /// ANOM scans are always single-leg per D4-02.
    fn arity(&self) -> ScanArity {
        ScanArity::Single
    }

    fn param_schema(&self) -> serde_json::Value {
        // Hand-rolled per Pattern A. The `variant` param is an enum of four
        // string values with default `"log"`. `additionalProperties: false`
        // matches LjungBoxScan's strict schema.
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "variant": {
                    "type": "string",
                    "enum": ["log", "simple", "intraday", "overnight"],
                    "default": "log",
                    "description": "Return variant — dispatches to primitives::returns::{log,simple,intraday,overnight}_returns."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        // D3-04 — compile-time constant key set per Pattern A.
        ScanFindingShape {
            effect_extra_keys: &["mean", "n", "returns_vector", "std", "variant_label"],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        // Step 1 — cancel-poll at entry (RESEARCH Pattern 4 site 1 / D3-22).
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Step 2 — resolve variant param.
        let variant = resolve_variant(req)?;

        // Step 3 — N=0 / N=1 input guard. Returns need >= 2 closes; intraday /
        // overnight may produce fewer (post-partition) but the input itself
        // must have >= 2 bars or the scan emits ScanError::Kernel with the
        // InsufficientData label.
        let n_closes = ctx.bars.close.len();
        if n_closes < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.returns.profile: need at least 2 closes; got n={n_closes} (InsufficientData)"
            )));
        }

        // Step 4 — compute returns via the kernel dispatch.
        let returns = kernel::dispatch_variant(&ctx.bars.close, &ctx.bars.ts_open_utc, variant);
        let n = returns.len();

        // intraday / overnight may produce < 2 returns if every bar boundary
        // is on the same UTC date (intraday) or never crosses one
        // (overnight). Surface as InsufficientData so the consumer can re-run
        // with a wider window.
        if n < 2 {
            return Err(ScanError::Kernel(format!(
                "stats.returns.profile: variant={} produced n={n} returns (need >= 2); InsufficientData",
                variant.as_str()
            )));
        }

        let (mean, std) = kernel::mean_and_std(&returns);

        // Step 5 — build the envelope. `effect.metric` is dynamic per variant
        // per RESEARCH §2 ("returns_log_mean" for variant=log).
        let metric = format!("returns_{}_mean", variant.as_str());

        // effect.extra arrays (BTreeMap ordering deterministic per OUT-03).
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("mean".into(), f64_slice_to_raw_array(&[mean]));
        extra.insert("n".into(), f64_slice_to_raw_array(&[n_to_f64(n)]));
        extra.insert("returns_vector".into(), f64_slice_to_raw_array(&returns));
        extra.insert("std".into(), f64_slice_to_raw_array(&[std]));
        // variant_label is a string-typed array. The wire form encodes it via
        // a 1-element RawArray whose bytes are the UTF-8 bytes of the variant
        // string padded to 8 bytes (so shape=[1], dtype=F64). NO — that's
        // wrong: the variant label is a metadata string, not an f64 array.
        // The scan ships the label inside the resolved_params echo at the
        // ResultFinding level (`r.params.variant`) AND a 1-element f64-encoded
        // discriminator (0=log, 1=simple, 2=intraday, 3=overnight) here so
        // consumers that only parse effect.extra can recover the variant.
        let variant_id = variant_discriminant_f64(variant);
        extra.insert(
            "variant_label".into(),
            f64_slice_to_raw_array(&[variant_id]),
        );

        let effect = Effect {
            metric,
            value: mean,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize <= u64 on all supported targets (Phase 1 is 64-bit only)"
            )]
            n: Some(n as u64),
            ci95: None,
            effect_size: None,
            extra,
        };

        // Step 6 — raw.series. The `timestamps_ms` invariant is enforced by
        // Raw::new (D-03). Per-return timestamp is the bar-open epoch-ms of
        // bar t+1 (the bar whose close ended return t) — same convention as
        // LjungBoxScan, but the row count depends on the variant:
        //
        // - Log / Simple: n_returns == n_closes - 1; timestamps_ms is
        //   ts_open_utc[1..].len() (skip bar 0).
        // - Intraday / Overnight: n_returns may be < n_closes - 1; the
        //   timestamps_ms vector tracks the partitioned subset. To keep the
        //   wire form simple AND consistent across variants we ship one
        //   timestamp per emitted return — for log/simple this is
        //   ts_open_utc[1..]; for intraday/overnight it's the per-emission
        //   timestamps. To avoid recomputing the partition we recompute the
        //   timestamps inline mirroring the kernel's logic.
        let timestamps_ms = compute_timestamps_ms(&ctx.bars.close, &ctx.bars.ts_open_utc, variant);
        debug_assert_eq!(
            timestamps_ms.len(),
            n,
            "returns_vector and timestamps_ms must be parallel"
        );

        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert("returns".into(), f64_slice_to_raw_array(&returns));
        series.insert(
            "timestamps_ms".into(),
            f64_slice_to_raw_array(&timestamps_ms),
        );
        let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;

        // Phase 4 (D4-03): leg-labelled sources vector (length 1 for Single
        // arity).
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

        // Step 7 — emit (Pitfall 5: NEVER call sink.flush from inside a scan).
        sink.write_envelope(&Finding::Result(result))?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve `req.resolved_params["variant"]` -> `ReturnsVariant`. Defaults to
/// `Log` when absent. Rejects non-string / unknown-string values with
/// `ScanError::Kernel(_)` so the engine surfaces a typed `Finding::ScanError`.
fn resolve_variant(req: &ScanRequest) -> Result<ReturnsVariant, ScanError> {
    let raw = req.resolved_params.get("variant");
    let label = match raw {
        Some(v) => v.as_str().ok_or_else(|| {
            ScanError::Kernel(format!(
                "stats.returns.profile: variant must be log|simple|intraday|overnight; got {v}"
            ))
        })?,
        None => "log",
    };
    match label {
        "log" => Ok(ReturnsVariant::Log),
        "simple" => Ok(ReturnsVariant::Simple),
        "intraday" => Ok(ReturnsVariant::Intraday),
        "overnight" => Ok(ReturnsVariant::Overnight),
        other => Err(ScanError::Kernel(format!(
            "stats.returns.profile: variant must be log|simple|intraday|overnight; got {other:?}"
        ))),
    }
}

/// Discriminator for `effect.extra.variant_label`. f64-encoded so the
/// `RawArray` dtype stays `F64` (the v1 catalogue offers only `F64`). Decoders
/// look up the variant by integer index:
///   0 = log
///   1 = simple
///   2 = intraday
///   3 = overnight
#[inline]
fn variant_discriminant_f64(v: ReturnsVariant) -> f64 {
    match v {
        ReturnsVariant::Log => 0.0,
        ReturnsVariant::Simple => 1.0,
        ReturnsVariant::Intraday => 2.0,
        ReturnsVariant::Overnight => 3.0,
    }
}

/// Compute the per-return `timestamps_ms` vector mirroring the kernel's
/// partition logic. Mirrors the four `primitives::returns::*` kernels'
/// emit-conditions; keeps the test harness simple (one source of truth for
/// the parallel vector).
#[inline]
#[allow(
    clippy::cast_precision_loss,
    reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
)]
fn compute_timestamps_ms(
    closes: &[f64],
    ts: &[chrono::DateTime<chrono::Utc>],
    variant: ReturnsVariant,
) -> Vec<f64> {
    debug_assert_eq!(closes.len(), ts.len());
    if closes.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(closes.len() - 1);
    for i in 1..closes.len() {
        use chrono::Datelike;
        let same_day = ts[i - 1].num_days_from_ce() == ts[i].num_days_from_ce();
        let emit = match variant {
            ReturnsVariant::Log | ReturnsVariant::Simple => true,
            ReturnsVariant::Intraday => same_day,
            ReturnsVariant::Overnight => !same_day,
        };
        if emit {
            out.push(ts[i].timestamp_millis() as f64);
        }
    }
    out
}

/// usize -> f64 for `effect.extra.n` 1-element array. Bounded above by bar
/// count which fits trivially in f64's 52-bit mantissa.
#[allow(
    clippy::cast_precision_loss,
    reason = "n is the returns count; bar counts << 2^52"
)]
#[inline]
fn n_to_f64(n: usize) -> f64 {
    n as f64
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

    // -----------------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------------

    fn blake3_hex_zero() -> Blake3Hex {
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

    /// LCG-seeded deterministic `BarFrame` builder (Numerical Recipes
    /// constants; same as `ljung_box::tests::ar1_bar_frame_seeded`). Drops
    /// the `_ar1` prefix here because the closes are uniform pseudo-random
    /// in (1.0, 2.0) — not AR(1). The name pinned the lineage in Phase 3
    /// tests.
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
                let i_i64 = i64::try_from(i).expect("test n fits in i64");
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
    // 1. id + version
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_id_and_version() {
        let s = ReturnsProfileScan;
        assert_eq!(s.id(), "stats.returns.profile");
        assert_eq!(s.version(), 1);
    }

    // -----------------------------------------------------------------------
    // 2. arity
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_arity_is_single() {
        let s = ReturnsProfileScan;
        assert_eq!(s.arity(), ScanArity::Single);
        assert_eq!(s.arity().expected_len(), 1);
    }

    // -----------------------------------------------------------------------
    // 3. param_schema
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_param_schema() {
        let s = ReturnsProfileScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        let variant = &schema["properties"]["variant"];
        assert_eq!(variant["type"], "string");
        assert_eq!(variant["default"], "log");
        let enum_arr = variant["enum"].as_array().expect("enum array");
        let labels: Vec<&str> = enum_arr.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(labels, vec!["log", "simple", "intraday", "overnight"]);
        assert_eq!(schema["additionalProperties"], false);
    }

    // -----------------------------------------------------------------------
    // 4. default variant = log
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_default_variant_is_log() {
        let bars = lcg_bar_frame_seeded(32, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.effect.metric, "returns_log_mean");
        // variant_label discriminator = 0.0 for log.
        let label_bytes = &r.effect.extra["variant_label"].data.0;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&label_bytes[0..8]);
        assert_eq!(f64::from_le_bytes(buf), 0.0_f64);
    }

    // -----------------------------------------------------------------------
    // 5. explicit variant=simple
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_explicit_variant_simple() {
        let bars = lcg_bar_frame_seeded(32, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"variant": "simple"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.effect.metric, "returns_simple_mean");
        let label_bytes = &r.effect.extra["variant_label"].data.0;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&label_bytes[0..8]);
        assert_eq!(f64::from_le_bytes(buf), 1.0_f64, "simple discriminant");
    }

    // -----------------------------------------------------------------------
    // 6. invalid variant rejected
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_invalid_variant_rejected() {
        let bars = lcg_bar_frame_seeded(32, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"variant": "garbage"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => {
                assert!(
                    msg.contains("variant must be log|simple|intraday|overnight"),
                    "msg: {msg}"
                );
            }
            other => panic!("expected ScanError::Kernel; got {other:?}"),
        }
        assert!(sink.0.is_empty(), "no envelope on rejection");
    }

    // -----------------------------------------------------------------------
    // 7. emits exactly one Result
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_emits_one_result() {
        let bars = lcg_bar_frame_seeded(64, 4);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"variant": "log"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    // -----------------------------------------------------------------------
    // 8. result envelope shape
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_result_envelope_shape() {
        let bars = lcg_bar_frame_seeded(64, 5);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"variant": "log"}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // PINNED field names.
        assert_eq!(r.scan_id_at_version, "stats.returns.profile@1");
        assert_eq!(r.param_hash, req.param_hash.as_str().to_string());
        assert_eq!(r.code_revision, "abc1234");
        assert_eq!(r.schema_version, 1);
        assert_eq!(r.dsr, None);
        assert_eq!(r.fdr_q, None);
        // effect.* shape.
        assert_eq!(r.effect.metric, "returns_log_mean");
        // 64 closes -> 63 log returns.
        assert_eq!(r.effect.n, Some(63));
        assert_eq!(r.effect.p_value, None);
        assert_eq!(r.effect.ci95, None);
        // effect.extra keys exactly the declared set.
        let extra_keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            extra_keys,
            vec!["mean", "n", "returns_vector", "std", "variant_label"]
        );
        // raw.series keys.
        let raw = r.raw.as_ref().expect("raw present");
        let raw_keys: Vec<&str> = raw.series.keys().map(String::as_str).collect();
        assert_eq!(raw_keys, vec!["returns", "timestamps_ms"]);
        // D4-03 leg-labelled sources Vec length 1 for Single arity.
        assert_eq!(r.data_slice.sources.len(), 1);
        let leg = &r.data_slice.sources[0];
        assert_eq!(leg.source_id, "dukascopy");
        assert_eq!(leg.symbol, "EURUSD");
        assert_eq!(leg.side, "bid");
        assert_eq!(leg.timeframe, "15m");
    }

    // -----------------------------------------------------------------------
    // 9. Raw::new enforces timestamps_ms (D-03)
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_raw_new_enforces_timestamps_ms() {
        // Constructive test: a normal run produces a Raw block containing
        // `timestamps_ms` — the Raw::new contract surfaces if the key is
        // ever dropped.
        let bars = lcg_bar_frame_seeded(16, 6);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!(r.raw.as_ref().unwrap().series.contains_key("timestamps_ms"));
    }

    // -----------------------------------------------------------------------
    // 10. cancellation at entry
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_cancellation() {
        let bars = lcg_bar_frame_seeded(64, 7);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)));
        ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel returns Ok");
        assert!(sink.0.is_empty(), "no envelope on cancel-at-entry");
    }

    // -----------------------------------------------------------------------
    // 11. N=0 closes -> ScanError::Kernel InsufficientData
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_n_zero_emits_scan_error_insufficient_data() {
        // Build an empty BarFrame.
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: Vec::new(),
            ts_close_utc: Vec::new(),
            open: Vec::new(),
            high: Vec::new(),
            low: Vec::new(),
            close: Vec::new(),
            tick_volume: Vec::new(),
        };
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => assert!(
                msg.contains("InsufficientData"),
                "expected InsufficientData label; got {msg}"
            ),
            other => panic!("expected Kernel(InsufficientData); got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // 12. N=1 closes -> ScanError::Kernel InsufficientData
    // -----------------------------------------------------------------------

    #[test]
    fn returns_profile_n_one_emits_scan_error_insufficient_data() {
        let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let bars = BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: vec![t0],
            ts_close_utc: vec![t0 + Duration::minutes(15)],
            open: vec![1.0],
            high: vec![1.001],
            low: vec![0.999],
            close: vec![1.0],
            tick_volume: vec![1.0],
        };
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)));
        let err = ReturnsProfileScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        match err {
            ScanError::Kernel(msg) => assert!(
                msg.contains("InsufficientData"),
                "expected InsufficientData; got {msg}"
            ),
            other => panic!("expected Kernel(InsufficientData); got {other:?}"),
        }
    }
}

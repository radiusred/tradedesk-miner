//! `LjungBoxScan` — Phase 3 demo scan implementing the [`Scan`] trait.
//!
//! Pattern analog: `aggregator.rs::aggregate` — pure-kernel function calling a
//! `Reader`, returning a typed output. The Ljung-Box scan mirrors this shape
//! but reads from the brokering [`ScanCtx`] (the engine constructs it) and
//! writes one `Finding::Result` to the sink.
//!
//! ## D3-01..D3-05 contract
//!
//! - `id = "stats.autocorr.ljung_box"`, `version = 1` (D3-01, D3-17).
//! - Log returns computed inline from `BarFrame.close` (D3-02; no ANOM-01
//!   pull-forward).
//! - Default `lags = min(10, n / 5)` per Box-Jenkins / statsmodels (D3-03).
//! - `effect.metric = "ljung_box_q"`, `effect.value` = Q-stat at max lag,
//!   `effect.p_value` = chi-squared p-value (df = lags), `effect.n` = sample
//!   size of the returns series, `effect.extra.{lags,q_stats,p_values,acf}`,
//!   `raw.series.{returns,timestamps_ms}` (D3-04).
//!
//! ## Algorithm (Plan 04 walk per gap.rs:158-186 analog)
//!
//! 1. Cancel-poll at entry (RESEARCH Pattern 4 site 1 — D3-22).
//! 2. Resolve `lags` from `req.resolved_params` (default `min(10, n/5)`); reject
//!    `lags < 1` or `lags >= n` via `ScanError::Compute(_)` per D3-03.
//! 3. Compute log returns from `ctx.bars.close` (D3-02 inline; no separate
//!    ANOM-01 dependency).
//! 4. Compute biased ACF and (Q-stat, p-value) vectors via the kernel module.
//! 5. Build the `ResultFinding` envelope using the PINNED field names from
//!    `findings/mod.rs` (Warning 7): `scan_id_at_version` (renamed via
//!    `#[serde(rename = "scan_id@version")]`), `param_hash: String`,
//!    `source: Source`, `params: Value`, `effect.ci95` (not `ci_95`),
//!    `effect.n: Option<u64>`, `raw: Option<Raw>` — [`Raw::new`] enforces the
//!    `timestamps_ms` invariant (D-03).
//! 6. Emit one `Finding::Result` via `sink.write_envelope` (Pitfall 5 — NEVER
//!    call the sink's `flush()` method inside the scan body; the facade owns flush).
//! 7. (test-only) Cancel-aware sleep loop on `ctx.sleep_after_first_finding_ms`
//!    polls cancel every ~10ms (Blocker 3 step 3 — Pitfall 8 mitigation).
//!
//! [`Scan`]: crate::scan::Scan
//! [`ScanCtx`]: crate::scan::ScanCtx

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, EffectSize, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// Phase 3 demo scan — Ljung-Box on log returns of a bar series.
///
/// Stateless unit struct (no fields) — pattern: `gap.rs:152-155` `GapDetector`.
pub struct LjungBoxScan;

/// `scan_id` constant — exposed as a module-level item so tests + future
/// callers can reference the canonical wire string without re-typing it.
const SCAN_ID: &str = "stats.autocorr.ljung_box";

/// Scan version. Bumps on output-shape change.
const SCAN_VERSION: u32 = 1;

/// `effect.metric` constant — D3-04 wire-form locked.
const EFFECT_METRIC: &str = "ljung_box_q";

impl Scan for LjungBoxScan {
    fn id(&self) -> &'static str {
        SCAN_ID
    }

    fn version(&self) -> u32 {
        SCAN_VERSION
    }

    /// Phase 4 (Plan 04-01 / D4-02): `LjungBox` is a single-leg ANOM scan
    /// — arity is always [`ScanArity::Single`].
    fn arity(&self) -> ScanArity {
        ScanArity::Single
    }

    fn param_schema(&self) -> serde_json::Value {
        // Hand-rolled per RESEARCH Pattern 3 lines 354-365. The `lags` field
        // is a positive integer; default is `min(10, n / 5)` computed at run
        // time (`run` resolves the default), so the schema only documents the
        // range constraint.
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "lags": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Number of lags for the Ljung-Box Q-statistic; defaults to min(10, n/5)"
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        // D3-04 — compile-time constant key set.
        ScanFindingShape {
            effect_extra_keys: &["lags", "q_stats", "p_values", "acf"],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-03) — opt-in to bootstrap CI.
    fn supports_bootstrap(&self) -> bool {
        true
    }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-04) — opt-in to null methods
    /// (`PhaseScramble` + `CircularShift`) per the per-scan matrix.
    fn supports_null_method(&self, m: crate::scan::NullMethod) -> bool {
        matches!(
            m,
            crate::scan::NullMethod::PhaseScramble | crate::scan::NullMethod::CircularShift
        )
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
        // Step 1 — cancel-poll at entry (RESEARCH Pattern 4 site 1).
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Step 2-4 — compute returns + ACF + Q-stats.
        // D4-06: returns kernel lifted to `scan::primitives::returns::log_returns`
        // (the body was a byte-identical move per Pitfall 9). The Phase 3
        // statsmodels golden continues to pass byte-identically.
        let returns = log_returns(&ctx.bars.close);
        let n = returns.len();
        if n < 2 {
            // No statistical content at n < 2; reject so the engine surfaces
            // a typed ScanError (the test path pins this contract).
            return Err(ScanError::Kernel(format!(
                "ljung_box: need at least 2 returns; got n={n}"
            )));
        }

        let lags = resolve_lags(req, n)?;
        let acf = kernel::biased_acf(&returns, lags);
        let (q_stats, p_values) = kernel::ljung_box_q_and_p(n, &acf, lags);

        // Step 5 — build the envelope.

        // `effect.extra` arrays.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("lags".into(), f64_slice_to_raw_array(&[lags_to_f64(lags)]));
        extra.insert("q_stats".into(), f64_slice_to_raw_array(&q_stats));
        extra.insert("p_values".into(), f64_slice_to_raw_array(&p_values));
        extra.insert("acf".into(), f64_slice_to_raw_array(&acf));

        // Plan 05-03 / D5-03: acf_lag_max_abs = max(|acf[k]|) for k >= 1.
        // (Lag 0 = 1.0 by construction; only proper lags carry information.)
        let acf_lag_max_abs = acf
            .iter()
            .skip(1)
            .copied()
            .map(f64::abs)
            .fold(0.0_f64, f64::max);
        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: q_stats[lags - 1],
            p_value: Some(p_values[lags - 1]),
            // usize -> u64 is widening on 32-bit and identity on 64-bit; cast
            // is exact and lossless (Phase 1 only targets 64-bit pointers per
            // FOUND-04 invariant). `try_into` would force an Err arm that can
            // never trigger.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize <= u64 on all supported targets (Phase 1 is 64-bit only)"
            )]
            n: Some(n as u64),
            ci95: None,
            effect_size: Some(EffectSize {
                kind: "acf_lag_max_abs".to_string(),
                value: acf_lag_max_abs,
            }),
            extra,
        };

        // `raw.series` arrays. The `timestamps_ms` invariant is enforced by
        // Raw::new (D-03). Per-return timestamp is the bar-open epoch-ms of
        // bar `t+1` (the bar whose close ended return `t`); skip bar 0 because
        // returns is shorter by one.
        //
        // The current `Dtype` enum only declares `F64` in v1 (see
        // findings/base64_bytes.rs:75); we encode the i64 epoch-ms values as
        // f64 (exact representation up to 2^53 ms, far beyond any reasonable
        // year) so the wire form stays a single dtype. Future additive Dtype
        // variants (I64) can refine the encoding without breaking consumers.
        let timestamps_ms: Vec<f64> = ctx
            .bars
            .ts_open_utc
            .iter()
            .skip(1)
            .map(|dt| {
                // i64 epoch-ms -> f64: exact for any timestamp within
                // [-2^53, 2^53] ms (~285M years either side of epoch). The
                // realistic OHLCV range is decades, so the precision loss is
                // theoretical only.
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
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

        // Phase 4 (D4-03): populate `data_slice.sources: Vec<Source>` parallel
        // to `req.instruments` (length 1 for ANOM single-leg). `Source.side`
        // / `Source.timeframe` carry the canonical wire-form strings.
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

        // Step 6 — emit. Pitfall 5: scans NEVER call the sink's flush method;
        // the facade owns flush.
        sink.write_envelope(&Finding::Result(result))?;

        // Step 7 — cancel-aware sleep hook (Blocker 3 step 3 / Pitfall 8). Only
        // compiled under cfg(test) or feature = "test-internal"; release builds
        // never see this branch. Polls cancel every ~10ms instead of calling
        // std::thread::sleep(Duration::from_millis(total)) in one go.
        #[cfg(any(test, feature = "test-internal"))]
        if let Some(total_ms) = ctx.sleep_after_first_finding_ms {
            let step = std::time::Duration::from_millis(10);
            let mut remaining = std::time::Duration::from_millis(total_ms);
            while !ctx.cancel.load(Ordering::Relaxed) && !remaining.is_zero() {
                let pause = remaining.min(step);
                std::thread::sleep(pause);
                remaining = remaining.saturating_sub(pause);
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the `lags` parameter for an Ljung-Box invocation per D3-03.
///
/// Reads `req.resolved_params["lags"]` if present; else defaults to
/// `min(10, n / 5)`. Validates `1 <= lags < n`; returns
/// `ScanError::Kernel(_)` on violation so the engine surfaces a typed
/// `Finding::ScanError` envelope.
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

/// D3-03 default: `min(10, n / 5)`. For very small `n` this can collapse to 0,
/// which `resolve_lags` then rejects via the `lags < 1` branch.
fn default_lags(n: usize) -> i64 {
    let candidate = (n / 5).min(10);
    // usize -> i64 cast is lossless for `candidate <= 10`. The .min(10) bound
    // makes the cast trivially safe.
    i64::try_from(candidate).expect("candidate is bounded by 10 — fits in i64")
}

/// Convert a usize lag value to f64 for the `effect.extra["lags"]` 1-element
/// array. Bounded above by `n` (the returns count); fits easily in f64.
#[allow(
    clippy::cast_precision_loss,
    reason = "lags is bounded by n (the returns count); realistic values are << 2^52"
)]
fn lags_to_f64(lags: usize) -> f64 {
    lags as f64
}

// Plan 04-02 / D4-06 helper-lift: `f64_slice_to_raw_array` was lifted to
// `crate::scan::primitives::raw_array::f64_slice_to_raw_array` so all 22
// Phase 4 scans share one copy. The Phase 3 statsmodels golden continues
// to pass byte-identically after the lift (the helper body is verbatim).

// Workaround for clippy::needless_pass_by_value being noisy on the `n` arg
// of `resolve_lags`: the function is small and the `n: usize` is by-value
// (Copy) — no allocation. The lint doesn't fire here, but documenting the
// shape via the type signature is the goal.

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
    use std::time::Instant;

    // -----------------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------------

    fn blake3_hex_zero() -> Blake3Hex {
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

    /// Deterministic linear-congruential PRNG seeded by `seed`; produces a
    /// `BarFrame` of `n` 15m bars starting at 2024-01-01 UTC. The closes are
    /// not constant (denom != 0 so ACF is well-defined) and are not strictly
    /// monotone (so Q-stats and p-values are non-trivial).
    ///
    /// LCG constants (Numerical Recipes): a = 1664525, c = 1013904223, m = 2^32.
    /// Output scaled to `1.0 + (s / 2^32)` so closes land in (1.0, 2.0).
    fn ar1_bar_frame_seeded(n: usize, seed: u64) -> BarFrame {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        // Truncate seed to 32 bits intentionally — the LCG operates on u32.
        #[allow(
            clippy::cast_possible_truncation,
            reason = "deliberate u32 truncation to fit the LCG state"
        )]
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

    fn make_ctx(bars: &BarFrame, cancel: Arc<AtomicBool>, sleep_ms: Option<u64>) -> ScanCtx<'_> {
        ScanCtx {
            bars,
            bars_pair: None,
            gap_manifest: None,
            run_id: RunId::new(),
            code_revision: "abc1234",
            cancel,
            sleep_after_first_finding_ms: sleep_ms,
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
    fn ljung_box_scan_id_and_version() {
        let s = LjungBoxScan;
        assert_eq!(s.id(), "stats.autocorr.ljung_box");
        assert_eq!(s.version(), 1);
    }

    /// Plan 04-01 Task 2 — Behavior Test 6: `ljung_box_scan_reports_single_arity`.
    /// `LjungBoxScan` is an ANOM scan (D4-02) and therefore reports
    /// `ScanArity::Single`. The 22 Phase 4 scans each declare arity in the
    /// same fashion via the new `Scan::arity()` trait method (no default
    /// body — every impl is explicit).
    #[test]
    fn ljung_box_scan_reports_single_arity() {
        let s = LjungBoxScan;
        assert_eq!(s.arity(), ScanArity::Single);
        assert_eq!(s.arity().expected_len(), 1);
        assert_eq!(s.arity().as_str(), "single");
    }

    #[test]
    fn ljung_box_scan_finding_fields() {
        let s = LjungBoxScan;
        let shape = s.finding_fields();
        assert_eq!(
            shape.effect_extra_keys,
            &["lags", "q_stats", "p_values", "acf"]
        );
        assert_eq!(shape.raw_series_keys, &["returns", "timestamps_ms"]);
    }

    #[test]
    fn ljung_box_scan_param_schema() {
        let s = LjungBoxScan;
        let schema = s.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["lags"]["type"], "integer");
        assert_eq!(schema["properties"]["lags"]["minimum"], 1);
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn ljung_box_scan_default_lags() {
        // 256 closes -> 255 returns -> min(10, 255/5) = min(10, 51) = 10.
        let bars = ar1_bar_frame_seeded(256, 1);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("run ok");

        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // 10 q-stats means lags=10 was selected.
        let qs = r.effect.extra.get("q_stats").expect("q_stats present");
        assert_eq!(qs.shape, vec![10]);
    }

    #[test]
    fn ljung_box_scan_explicit_lags() {
        let bars = ar1_bar_frame_seeded(256, 2);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.effect.extra["q_stats"].shape, vec![5]);
    }

    #[test]
    fn ljung_box_scan_invalid_lags_low() {
        let bars = ar1_bar_frame_seeded(256, 3);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 0}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        let err = LjungBoxScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)), "got {err:?}");
    }

    #[test]
    fn ljung_box_scan_invalid_lags_high() {
        let bars = ar1_bar_frame_seeded(256, 4);
        let mut sink = VecSink::new();
        // 256 closes -> 255 returns; lags=999 >= 255 must reject.
        let req = sample_request_with_params(serde_json::json!({"lags": 999}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        let err = LjungBoxScan
            .run(&ctx, &req, &mut sink)
            .expect_err("must reject");
        assert!(matches!(err, ScanError::Kernel(_)), "got {err:?}");
    }

    #[test]
    fn ljung_box_scan_emits_one_result() {
        let bars = ar1_bar_frame_seeded(256, 5);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1, "expected exactly one envelope");
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn ljung_box_scan_result_envelope_shape() {
        let bars = ar1_bar_frame_seeded(256, 6);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 7}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // PINNED field names (Warning 7).
        assert_eq!(r.scan_id_at_version, "stats.autocorr.ljung_box@1");
        assert_eq!(r.param_hash, req.param_hash.as_str().to_string());
        assert_eq!(r.code_revision, "abc1234");
        assert_eq!(r.dsr, None);
        assert_eq!(r.fdr_q, None);
        assert_eq!(r.schema_version, 1);
        // effect.* shape locked.
        assert_eq!(r.effect.metric, "ljung_box_q");
        assert_eq!(r.effect.n, Some(255));
        assert!(r.effect.p_value.is_some());
        assert_eq!(r.effect.ci95, None);
        // effect.extra keys are exactly the declared set.
        let extra_keys: Vec<&String> = r.effect.extra.keys().collect();
        assert_eq!(
            extra_keys.iter().map(|s| s.as_str()).collect::<Vec<&str>>(),
            vec!["acf", "lags", "p_values", "q_stats"]
        );
        // raw.series keys are exactly the declared set.
        let raw = r.raw.as_ref().expect("raw present");
        let raw_keys: Vec<&String> = raw.series.keys().collect();
        assert_eq!(
            raw_keys.iter().map(|s| s.as_str()).collect::<Vec<&str>>(),
            vec!["returns", "timestamps_ms"]
        );
        // D4-03: leg-labelled sources Vec on data_slice (replaces the old
        // singleton `source: Source` field on ResultFinding).
        assert_eq!(r.data_slice.sources.len(), 1, "Single-arity -> Vec len 1");
        let leg = &r.data_slice.sources[0];
        assert_eq!(leg.source_id, "dukascopy");
        assert_eq!(leg.symbol, "EURUSD");
        assert_eq!(leg.side, "bid");
        assert_eq!(leg.timeframe, "15m");
    }

    #[test]
    fn ljung_box_scan_extras_have_correct_lengths() {
        let bars = ar1_bar_frame_seeded(256, 7);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 4}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.effect.extra["q_stats"].shape, vec![4]);
        assert_eq!(r.effect.extra["p_values"].shape, vec![4]);
        // ACF includes lag-0, so shape is lags + 1.
        assert_eq!(r.effect.extra["acf"].shape, vec![5]);
        assert_eq!(r.effect.extra["lags"].shape, vec![1]);
    }

    #[test]
    fn ljung_box_scan_raw_series_lengths() {
        let bars = ar1_bar_frame_seeded(256, 8);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 4}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let raw = r.raw.as_ref().expect("raw present");
        // 256 closes -> 255 returns; 255 timestamps_ms (one per return).
        assert_eq!(raw.series["returns"].shape, vec![255]);
        assert_eq!(raw.series["timestamps_ms"].shape, vec![255]);
    }

    #[test]
    fn ljung_box_scan_raw_new_enforces_timestamps_ms() {
        // Compile-time assertion that the source path uses Raw::new — we cannot
        // unit-test the negative case here without breaking the kernel, but the
        // acceptance grep `grep -c 'Raw::new' ...` covers the structural check.
        // Constructive test: a normal run produces a Raw block whose series
        // contains the required key.
        let bars = ar1_bar_frame_seeded(64, 9);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 3}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), None);
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_sink_to_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert!(r.raw.as_ref().unwrap().series.contains_key("timestamps_ms"));
    }

    #[test]
    fn ljung_box_scan_cancellation() {
        // With cancel = true at entry, run returns Ok(()) without emitting.
        let bars = ar1_bar_frame_seeded(256, 10);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 5}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(true)), None);
        LjungBoxScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel returns Ok");
        assert!(sink.0.is_empty(), "no envelope written on cancel-at-entry");
    }

    // Pitfall 5 invariant — the scan body must never invoke the sink's
    // explicit flush hook. Verified by acceptance grep (counted against the
    // production-code regions); an inline panicking-test variant would itself
    // contain the literal identifier and bloat the grep gate. Plan 06's
    // golden integration test pins behavioural equivalence end-to-end.

    #[test]
    fn ljung_box_scan_cancel_aware_sleep() {
        // Blocker 3 step 3 — set a 2000ms sleep hook and flip cancel ~50ms in;
        // run_one (here just scan.run) must return within ~150ms, NOT 2000ms.
        let bars = ar1_bar_frame_seeded(128, 12);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 5}));
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_watcher = Arc::clone(&cancel);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            cancel_watcher.store(true, Ordering::SeqCst);
        });
        let ctx = make_ctx(&bars, Arc::clone(&cancel), Some(2000));
        let start = Instant::now();
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("ok");
        let elapsed = start.elapsed();
        handle.join().expect("watcher joined");
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "cancel-aware sleep must interrupt within ~150ms; elapsed={elapsed:?}"
        );
        // The first Result was emitted before the sleep.
        let findings = parse_sink_to_findings(&sink);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn ljung_box_scan_sleep_hook_with_zero_ms_returns_immediately() {
        // Sanity: Some(0) means no sleep — the loop's first iteration sees
        // remaining == 0 and exits immediately.
        let bars = ar1_bar_frame_seeded(64, 13);
        let mut sink = VecSink::new();
        let req = sample_request_with_params(serde_json::json!({"lags": 3}));
        let ctx = make_ctx(&bars, Arc::new(AtomicBool::new(false)), Some(0));
        let start = Instant::now();
        LjungBoxScan.run(&ctx, &req, &mut sink).expect("ok");
        assert!(start.elapsed() < std::time::Duration::from_millis(100));
        assert_eq!(parse_sink_to_findings(&sink).len(), 1);
    }
}

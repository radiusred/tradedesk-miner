//! Engle-Granger two-step cointegration scan (CROSS-05).
//!
//! Single-shot Pair-arity scan computing the canonical pairs-trading
//! cointegration diagnostic via three sequential steps:
//!
//! 1. OLS regression `y_t = α + β x_t + ε_t` (per D4-09 / RESEARCH.md §1.7:
//!    `y = leg_a = req.instruments[0]`, `x = leg_b = req.instruments[1]`).
//! 2. ADF test on the residuals (constant-regression DF test in this
//!    mid-plan local kernel; Plan 04-05 / ANOM-05 supplies the canonical
//!    `adfuller` kernel with AIC lag selection that Plan 04-11 will
//!    reconcile to).
//! 3. OU half-life of the AR(1) residual: `τ = -ln(2) / ln(1 + ρ)` for
//!    `ρ ∈ (-1, 0)`; `f64::INFINITY` sentinel outside that range.
//!
//! Reference: `statsmodels.tsa.stattools.coint(y0, y1)` where `y0 = leg_a`
//! (regressand) and `y1 = leg_b` (regressor). The reported β is exactly
//! `β_y0_on_y1` per the statsmodels.coint signature — see 04-08-SUMMARY.md
//! for the explicit citation of this ordering.
//!
//! `effect.value` = β (hedge ratio per D4-09; RESEARCH.md §1.3 single-shot
//! canonical pick). `effect.p_value` = ADF-on-residuals p-value.
//! `effect.extra` carries `{adf_stat, hedge_ratio_alpha, ou_half_life,
//! residual_std, residuals}` (alphabetical, BTreeMap-friendly).
//! `raw.series` carries `{close_a, close_b, timestamps_ms}` — Engle-Granger
//! operates on LEVELS, not returns, so the raw block carries the closes
//! directly (with the canonical D-03 `timestamps_ms` key for the joint
//! aligned timestamps).
//!
//! ## `regression` parameter
//!
//! Mirrors `statsmodels.tsa.stattools.coint(trend=...)`. v1 supports
//! `"c"` (constant) — the canonical pairs-trading default. `"ct"`
//! (constant + linear trend) is accepted as a parameter for forward
//! compatibility but the local kernel downgrades it to `"c"` (the linear
//! detrend step is a no-op in this mid-plan stub). Plan 04-05 supplies
//! the full `"ct"` path through the canonical kernel.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, EffectSize, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::time_alignment::inner_join;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

pub use kernel::AdfRegression;

/// Pair-arity Engle-Granger two-step cointegration scan (CROSS-05).
pub struct EngleGrangerScan;

const SCAN_ID: &str = "cross.cointegration.engle_granger";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "engle_granger_hedge_ratio";

/// Minimum aligned-bar count required to fit the OLS regression and
/// produce a meaningful ADF stat + OU half-life. Below this the test is
/// underpowered and we reject up-stream. 30 matches the conservative pin
/// in the plan acceptance criteria.
///
/// `pub(crate)` so the rolling cointegration scan (`cross.cointegration.rolling`,
/// RAD-3626) reuses the SAME per-window floor rather than re-deriving it.
pub(crate) const MIN_ALIGNED_N: usize = 30;

const FINDING_SHAPE: ScanFindingShape = ScanFindingShape {
    effect_extra_keys: &[
        "adf_stat",
        "hedge_ratio_alpha",
        "ou_half_life",
        "residual_std",
        "residuals",
    ],
    // D-03 invariant: Raw::new requires a `timestamps_ms` key. Engle-Granger
    // carries the levels (close_a, close_b) — not returns — under the
    // joint aligned timestamps.
    raw_series_keys: &["close_a", "close_b", "timestamps_ms"],
};

fn engle_granger_param_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "regression": {
                "type": "string",
                "enum": ["c", "ct"],
                "default": "c",
                "description": "Trend specification for the embedded ADF test on residuals, matching statsmodels.tsa.stattools.coint(trend=...). v1 ships 'c' (constant); 'ct' is accepted for forward compatibility but downgraded to 'c' in the local kernel — Plan 04-05 / ANOM-05 supplies the full 'ct' path. See 04-08-SUMMARY.md."
            }
        },
        "additionalProperties": false
    })
}

impl Scan for EngleGrangerScan {
    fn id(&self) -> &'static str {
        SCAN_ID
    }
    fn version(&self) -> u32 {
        SCAN_VERSION
    }
    fn arity(&self) -> ScanArity {
        ScanArity::Pair
    }
    fn param_schema(&self) -> serde_json::Value {
        engle_granger_param_schema()
    }
    fn finding_fields(&self) -> ScanFindingShape {
        FINDING_SHAPE
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

    /// RAD-2397 — Engle-Granger is a single-shot whole-sample test: its
    /// `MIN_ALIGNED_N = 30` check evaluates the inner-joined, gap-removed
    /// series length. The engine must concatenate the per-sub-range frames
    /// emitted by the partitioner into ONE kernel call; otherwise the
    /// per-sub-range slices fall below the threshold and every dispatch
    /// short-circuits with `Engle-Granger needs >= 30 aligned bars; got N`.
    fn coalesce_subranges(&self) -> bool {
        true
    }

    #[allow(clippy::too_many_lines)]
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
        // 1. Cancel-at-entry.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 2. Pair-arity borrow check.
        let (bars_a, bars_b) = ctx.bars_pair().ok_or_else(|| {
            ScanError::Kernel("expected Pair arity (ctx.bars_pair is None)".into())
        })?;

        // 3. Inner-join (CROSS-01 primitive).
        let aligned = inner_join(bars_a, bars_b);
        let aligned_n = aligned.timestamps_ms.len();
        if aligned_n == 0 {
            return Err(ScanError::Kernel(
                "inner join produced zero overlap between legs".into(),
            ));
        }
        if aligned_n < MIN_ALIGNED_N {
            return Err(ScanError::Kernel(format!(
                "Engle-Granger needs >= {MIN_ALIGNED_N} aligned bars; got {aligned_n}"
            )));
        }

        // 4. Resolve regression param (no-op for v1 / always Constant in
        //    the local kernel; the param is read + validated so the
        //    param_schema contract holds and Plan 04-11 can flip the
        //    "ct" branch on without touching the scan body).
        let regression = resolve_regression(req)?;

        // 5. Cancel-poll before kernel.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 6. Run kernel — Engle-Granger operates on LEVELS (closes), not
        //    returns. Per D4-09 / RESEARCH.md §1.7: y = leg_a (close_a),
        //    x = leg_b (close_b).
        let result = kernel::engle_granger(&aligned.close_a, &aligned.close_b, regression);

        // 7. NaN detection — zero-variance regressor or singular system
        //    surfaces through β / α NaN.
        if result.hedge_ratio_beta.is_nan() || result.hedge_ratio_alpha.is_nan() {
            return Err(ScanError::Kernel(
                "Engle-Granger: zero-variance regressor (leg b) yields singular OLS system".into(),
            ));
        }

        // 8. Cancel-poll between kernel and envelope.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 9. Build effect.extra.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert(
            "adf_stat".into(),
            f64_slice_to_raw_array(&[result.adf_stat]),
        );
        extra.insert(
            "hedge_ratio_alpha".into(),
            f64_slice_to_raw_array(&[result.hedge_ratio_alpha]),
        );
        extra.insert(
            "ou_half_life".into(),
            f64_slice_to_raw_array(&[result.ou_half_life]),
        );
        extra.insert(
            "residual_std".into(),
            f64_slice_to_raw_array(&[result.residual_std]),
        );
        extra.insert(
            "residuals".into(),
            f64_slice_to_raw_array(&result.residuals),
        );

        // 10. effect.value = β (the hedge ratio per D4-09).
        // Plan 05-03 / D5-03: hedge_ratio effect_size mirrors β (`hedge_ratio`).
        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: result.hedge_ratio_beta,
            p_value: Some(result.adf_p_value),
            #[allow(
                clippy::cast_possible_truncation,
                reason = "aligned_n <= u64 on supported targets (Phase 1 is 64-bit only)"
            )]
            n: Some(aligned_n as u64),
            ci95: None,
            effect_size: Some(EffectSize {
                kind: "hedge_ratio".to_string(),
                value: result.hedge_ratio_beta,
            }),
            extra,
        };

        // 11. raw.series — leg-labelled closes + aligned timestamps (D-03
        //     canonical key). Engle-Granger uses the closes, not returns,
        //     so the keys are close_a / close_b (NOT returns_a / returns_b).
        let timestamps_ms_f: Vec<f64> = aligned
            .timestamps_ms
            .iter()
            .map(|ms| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
                )]
                let v = *ms as f64;
                v
            })
            .collect();
        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert("close_a".into(), f64_slice_to_raw_array(&aligned.close_a));
        series.insert("close_b".into(), f64_slice_to_raw_array(&aligned.close_b));
        series.insert(
            "timestamps_ms".into(),
            f64_slice_to_raw_array(&timestamps_ms_f),
        );
        let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;

        // 12. D4-03: two-leg sources Vec.
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
                "{SCAN_ID}: expected 2 instruments; got {}",
                req.instruments.len()
            )));
        }

        let result_finding = ResultFinding {
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

        sink.write_envelope(&Finding::Result(result_finding))?;
        Ok(())
    }
}

/// Resolve the `regression` param — optional enum `"c"` (default) or `"ct"`.
fn resolve_regression(req: &ScanRequest) -> Result<AdfRegression, ScanError> {
    let raw = req.resolved_params.get("regression");
    match raw {
        None => Ok(AdfRegression::Constant),
        Some(v) => {
            let s = v.as_str().ok_or_else(|| {
                ScanError::Kernel(format!("regression must be a string; got {v}"))
            })?;
            match s {
                "c" => Ok(AdfRegression::Constant),
                "ct" => Ok(AdfRegression::ConstantTrend),
                other => Err(ScanError::Kernel(format!(
                    "regression must be one of [c, ct]; got {other:?}"
                ))),
            }
        }
    }
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

    fn blake3_hex_zero() -> Blake3Hex {
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

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

    fn make_ts(n: usize) -> Vec<DateTime<Utc>> {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect()
    }

    fn two_leg_fixture(n: usize, seed_a: u64, seed_b: u64) -> (BarFrame, BarFrame) {
        let ts = make_ts(n);
        (
            build_bars("EURUSD", &ts, &lcg_closes(n, seed_a)),
            build_bars("GBPUSD", &ts, &lcg_closes(n, seed_b)),
        )
    }

    fn sample_request(params: serde_json::Value) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 10, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: SCAN_ID.into(),
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

    fn make_ctx<'a>(a: &'a BarFrame, b: &'a BarFrame, cancel: Arc<AtomicBool>) -> ScanCtx<'a> {
        ScanCtx {
            bars: a,
            bars_pair: Some((a, b)),
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

    #[test]
    fn engle_granger_id_and_version() {
        let s = EngleGrangerScan;
        assert_eq!(s.id(), "cross.cointegration.engle_granger");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn engle_granger_arity_is_pair() {
        assert_eq!(EngleGrangerScan.arity(), ScanArity::Pair);
        assert_eq!(EngleGrangerScan.arity().expected_len(), 2);
    }

    #[test]
    fn engle_granger_param_schema() {
        let schema = EngleGrangerScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["regression"]["type"], "string");
        assert_eq!(schema["properties"]["regression"]["default"], "c");
        let enum_arr = schema["properties"]["regression"]["enum"]
            .as_array()
            .expect("enum is array");
        assert_eq!(enum_arr.len(), 2);
        assert!(enum_arr.iter().any(|v| v == "c"));
        assert!(enum_arr.iter().any(|v| v == "ct"));
    }

    /// D4-09 sign-convention pin: for `close_a` = 2 * `close_b` exactly,
    /// regressing `close_a` ~ `close_b` yields β = 2.0 within 1e-10.
    /// Matches `statsmodels.tsa.stattools.coint(y0=close_a`, `y1=close_b`).
    #[test]
    fn engle_granger_hedge_ratio_sign_convention() {
        let n = 40;
        let ts = make_ts(n);
        let closes_b: Vec<f64> = (1..=n)
            .map(|i| f64::from(i32::try_from(i).unwrap()))
            .collect();
        let closes_a: Vec<f64> = closes_b.iter().map(|c| 2.0 * c).collect();
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        EngleGrangerScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        // effect.value = β = 2.0 within 1e-10 (per D4-09 + statsmodels.coint
        // y0 = leg_a / y1 = leg_b ordering).
        assert!(
            (r.effect.value - 2.0).abs() < 1e-10,
            "β = {}; expected 2.0 within 1e-10",
            r.effect.value
        );
    }

    #[test]
    fn engle_granger_emits_one_result() {
        let (a, b) = two_leg_fixture(60, 7, 11);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        EngleGrangerScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn engle_granger_result_envelope_shape() {
        let (a, b) = two_leg_fixture(60, 7, 11);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        EngleGrangerScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "cross.cointegration.engle_granger@1");
        assert_eq!(r.effect.metric, "engle_granger_hedge_ratio");
        // p_value is Some(adf_p) — Engle-Granger emits the headline ADF p.
        assert!(r.effect.p_value.is_some());
        // n = aligned_n = 60 (all bars overlap).
        assert_eq!(r.effect.n, Some(60));
        // effect.extra keys (alphabetical via BTreeMap).
        let keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "adf_stat",
                "hedge_ratio_alpha",
                "ou_half_life",
                "residual_std",
                "residuals"
            ]
        );
        // residuals vector length == aligned_n.
        let residuals = decode_f64(&r.effect.extra["residuals"]);
        assert_eq!(residuals.len(), 60);
        // raw.series carries the leg-labelled CLOSES (not returns) + canonical timestamps key.
        let raw = r.raw.as_ref().expect("raw present");
        assert!(raw.series.contains_key("close_a"));
        assert!(raw.series.contains_key("close_b"));
        assert!(raw.series.contains_key("timestamps_ms"));
        // D4-03: two-leg sources Vec.
        assert_eq!(r.data_slice.sources.len(), 2);
    }

    /// For `close_a` = 2 * `close_b` exactly, `residual_std` should be near zero
    /// (perfect linear fit).
    #[test]
    fn engle_granger_residual_std_matches_hand_derived() {
        let n = 40;
        let ts = make_ts(n);
        let closes_b: Vec<f64> = (1..=n)
            .map(|i| f64::from(i32::try_from(i).unwrap()))
            .collect();
        let closes_a: Vec<f64> = closes_b.iter().map(|c| 2.0 * c).collect();
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        EngleGrangerScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let residual_std = decode_f64(&r.effect.extra["residual_std"])[0];
        assert!(
            residual_std < 1e-9,
            "residual_std = {residual_std}; expected near zero for perfect 2x scaling"
        );
    }

    /// OU half-life is finite + positive for a mean-reverting (stationary)
    /// spread. We construct a strongly mean-reverting AR(1) spread.
    #[test]
    fn engle_granger_ou_half_life_finite_for_cointegrated_pair() {
        // y_t = x_t + ε_t where ε is mean-reverting AR(1).
        let n = 200;
        let ts = make_ts(n);
        // x = drifting walk.
        let mut sx: u32 = 0x1357_9BDF;
        let mut closes_b = Vec::with_capacity(n);
        let mut acc = 1.0_f64;
        for _ in 0..n {
            sx = sx.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let dx = (f64::from(sx) / f64::from(u32::MAX) - 0.5) * 0.01;
            acc += dx;
            closes_b.push(acc);
        }
        // ε = stationary AR(1) with φ = 0.3 (very stationary).
        let mut se: u32 = 0xACE_F123;
        let mut closes_a = Vec::with_capacity(n);
        let mut e_prev = 0.0_f64;
        for i in 0..n {
            se = se.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(se) / f64::from(u32::MAX) - 0.5) * 0.005;
            let e_t = 0.3_f64 * e_prev + noise;
            closes_a.push(closes_b[i] + e_t);
            e_prev = e_t;
        }
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        EngleGrangerScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let half_life = decode_f64(&r.effect.extra["ou_half_life"])[0];
        assert!(
            half_life.is_finite() && half_life > 0.0 && half_life < 100.0,
            "ou_half_life = {half_life}; expected finite + positive + < 100"
        );
    }

    #[test]
    fn engle_granger_cancellation() {
        let (a, b) = two_leg_fixture(60, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(true)));
        EngleGrangerScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel returns Ok");
        assert!(sink.0.is_empty(), "no envelope on cancel-at-entry");
    }

    #[test]
    fn engle_granger_n_too_small() {
        let (a, b) = two_leg_fixture(20, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = EngleGrangerScan
            .run(&ctx, &req, &mut sink)
            .expect_err("n=20 < MIN_ALIGNED_N=30 must reject");
        match err {
            ScanError::Kernel(msg) => {
                assert!(msg.contains("aligned bars"), "msg: {msg}");
            }
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    #[test]
    fn engle_granger_invalid_regression_param() {
        let (a, b) = two_leg_fixture(60, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"regression": "x"}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = EngleGrangerScan
            .run(&ctx, &req, &mut sink)
            .expect_err("invalid regression must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn engle_granger_zero_variance_b_emits_scan_error() {
        let n = 40;
        let ts = make_ts(n);
        let closes_b = vec![1.0_f64; n];
        let closes_a: Vec<f64> = (0..n)
            .map(|i| 1.0 + 0.01 * f64::from(i32::try_from(i).unwrap()))
            .collect();
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = EngleGrangerScan
            .run(&ctx, &req, &mut sink)
            .expect_err("constant leg b must reject");
        match err {
            ScanError::Kernel(msg) => {
                assert!(
                    msg.contains("singular") || msg.contains("zero-variance"),
                    "msg: {msg}"
                );
            }
            other => panic!("expected Kernel; got {other:?}"),
        }
    }
}

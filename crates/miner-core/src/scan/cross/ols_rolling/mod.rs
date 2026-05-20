//! Rolling OLS regression scan (CROSS-03).
//!
//! Pair-arity scan computing per-window β, α, R², and `residual_std` via
//! [`nalgebra`]-backed normal-equations OLS over inner-joined log returns.
//! Reference: `statsmodels.regression.rolling.RollingOLS`. Tolerance 1e-9
//! at the goldens level (Plan 04-11).
//!
//! `effect.value` = last-window β (RESEARCH §1.3 trader-intuition canonical
//! pick — β is the headline pairs-trading hedge-ratio output);
//! `effect.extra` carries `{betas, alphas, r2s, residual_stds,
//! window_starts_ms, window_length}`; `raw.series` carries
//! `{returns_a, returns_b, timestamps_ms}` (D-03 canonical key for the
//! joint aligned timestamps; see `corr_rolling` `FINDING_SHAPE` doc for
//! rationale).
//!
//! ## `DMatrix` vs `SMatrix` decision
//!
//! `RESEARCH.md` and `CLAUDE.md` TL;DR favor `nalgebra::SMatrix<f64, _, 2>`
//! for "small fixed-size" regressions. But the rolling window size is a
//! runtime parameter — `SMatrix` requires const-generic rows, which would
//! force a template specialization per window value at the call site.
//! `DMatrix` is the pragmatic pick: still `O(window)` memory per call (no
//! allocation pool needed for `window <= 512`), still fast on small `N`,
//! and preserves the same dense-linear-algebra path internally.
//! Documented per `CLAUDE.md` "small fixed sizes" note.

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

/// Pair-arity rolling OLS regression scan (CROSS-03).
pub struct OlsRollingScan;

const SCAN_ID: &str = "cross.ols.rolling";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "ols_rolling_beta_last";

const FINDING_SHAPE: ScanFindingShape = ScanFindingShape {
    effect_extra_keys: &[
        "betas",
        "alphas",
        "r2s",
        "residual_stds",
        "window_starts_ms",
        "window_length",
    ],
    // D-03 invariant: Raw::new requires a `timestamps_ms` key. The
    // CROSS-scan body carries the aligned joint timestamps under this
    // canonical key (same data; see corr_rolling::FINDING_SHAPE doc).
    raw_series_keys: &["returns_a", "returns_b", "timestamps_ms"],
};

fn ols_param_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "window": {
                "type": "integer",
                "minimum": 3,
                "description": "Rolling-window size (number of aligned bars). Must be >= 3 (OLS needs >= 3 observations for the residual df = window - 2)."
            }
        },
        "required": ["window"],
        "additionalProperties": false
    })
}

impl Scan for OlsRollingScan {
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
        ols_param_schema()
    }
    fn finding_fields(&self) -> ScanFindingShape {
        FINDING_SHAPE
    }
    /// Phase 5 (Plan 05-03 / D5-04 / HYG-03) — opt-in to bootstrap CI.
    fn supports_bootstrap(&self) -> bool { true }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-04) — opt-in to CircularShift only.
    fn supports_null_method(&self, m: crate::scan::NullMethod) -> bool {
        matches!(m, crate::scan::NullMethod::CircularShift)
    }

    #[allow(clippy::too_many_lines)]
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

        // 2. Pair borrow.
        let (bars_a, bars_b) = ctx.bars_pair().ok_or_else(|| {
            ScanError::Kernel("expected Pair arity (ctx.bars_pair is None)".into())
        })?;

        // 3. Inner-join.
        let aligned = inner_join(bars_a, bars_b);
        if aligned.timestamps_ms.is_empty() {
            return Err(ScanError::Kernel(
                "inner join produced zero overlap between legs".into(),
            ));
        }

        // 4. Log returns per leg.
        let returns_a = log_returns(&aligned.close_a);
        let returns_b = log_returns(&aligned.close_b);
        let returns_n = returns_a.len();
        debug_assert_eq!(returns_n, returns_b.len());
        if returns_n < 3 {
            return Err(ScanError::Kernel(format!(
                "rolling OLS needs at least 3 aligned returns; got {returns_n}"
            )));
        }

        // 5. Resolve window param.
        let window = resolve_window(req, returns_n)?;

        // 6. Cancel before kernel.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 7. Run kernel — regress `returns_a` on `returns_b` (the convention
        //    is `a ~ b` so β represents the hedge ratio "how many units of
        //    asset b to short per long unit of a").
        let ols = kernel::rolling_ols(&returns_a, &returns_b, window);
        debug_assert_eq!(ols.len(), returns_n - window + 1);

        // 8. NaN detection — any singular system or zero-variance window
        //    produced NaN in betas; convert to ScanError::Kernel.
        if let Some(idx) = ols.betas.iter().position(|v| v.is_nan()) {
            return Err(ScanError::Kernel(format!(
                "zero-variance window at index {idx}: OLS regression undefined"
            )));
        }

        // 9. Cancel-poll between kernel and envelope.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 10. window_starts_ms — see corr_rolling for the index-derivation
        //     argument (the return at returns position t corresponds to the
        //     aligned bar at index t+1; the window starting at returns
        //     position i starts at aligned index i+1).
        let mut window_starts_ms: Vec<f64> = Vec::with_capacity(ols.len());
        for i in 0..ols.len() {
            #[allow(
                clippy::cast_precision_loss,
                reason = "epoch-ms fits exactly in f64 mantissa for realistic timestamps"
            )]
            let v = aligned.timestamps_ms[i + 1] as f64;
            window_starts_ms.push(v);
        }

        // 11. effect.extra arrays.
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("betas".into(), f64_slice_to_raw_array(&ols.betas));
        extra.insert("alphas".into(), f64_slice_to_raw_array(&ols.alphas));
        extra.insert("r2s".into(), f64_slice_to_raw_array(&ols.r2s));
        extra.insert(
            "residual_stds".into(),
            f64_slice_to_raw_array(&ols.residual_stds),
        );
        extra.insert(
            "window_starts_ms".into(),
            f64_slice_to_raw_array(&window_starts_ms),
        );
        extra.insert(
            "window_length".into(),
            f64_slice_to_raw_array(&[window_as_f64(window)]),
        );

        let last_beta = *ols.betas.last().expect("betas non-empty by construction");

        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: last_beta,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "usize <= u64 on all supported targets (Phase 1 is 64-bit only)"
            )]
            n: Some(ols.len() as u64),
            ci95: None,
            effect_size: None,
            extra,
        };

        // 12. raw.series — leg-labelled + aligned timestamps.
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

        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert("returns_a".into(), f64_slice_to_raw_array(&returns_a));
        series.insert("returns_b".into(), f64_slice_to_raw_array(&returns_b));
        series.insert(
            "timestamps_ms".into(),
            f64_slice_to_raw_array(&timestamps_ms_aligned),
        );
        let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;

        // 13. D4-03: two-leg sources Vec.
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

fn resolve_window(req: &ScanRequest, returns_n: usize) -> Result<usize, ScanError> {
    let raw = req
        .resolved_params
        .get("window")
        .ok_or_else(|| ScanError::Kernel("rolling OLS: window param required".into()))?;
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

    fn two_leg_fixture(n: usize, seed_a: u64, seed_b: u64) -> (BarFrame, BarFrame) {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
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

    #[test]
    fn ols_rolling_id_and_version() {
        let s = OlsRollingScan;
        assert_eq!(s.id(), "cross.ols.rolling");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn ols_rolling_arity_is_pair() {
        assert_eq!(OlsRollingScan.arity(), ScanArity::Pair);
    }

    #[test]
    fn ols_rolling_param_schema() {
        let schema = OlsRollingScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["window"]["minimum"], 3);
        let required = schema["required"].as_array().expect("required is array");
        assert!(required.iter().any(|v| v == "window"));
    }

    #[test]
    fn ols_rolling_emits_one_result() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        OlsRollingScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn ols_rolling_result_envelope_shape() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        OlsRollingScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "cross.ols.rolling@1");
        assert_eq!(r.effect.metric, "ols_rolling_beta_last");
        // effect.value == last beta (verified by extracting the betas vec).
        let betas = decode_f64(&r.effect.extra["betas"]);
        assert_eq!(r.effect.value, *betas.last().expect("non-empty"));
        // All extras have equal length = values.len() (except the
        // scalar window_length).
        let n_windows = betas.len();
        assert_eq!(decode_f64(&r.effect.extra["alphas"]).len(), n_windows);
        assert_eq!(decode_f64(&r.effect.extra["r2s"]).len(), n_windows);
        assert_eq!(
            decode_f64(&r.effect.extra["residual_stds"]).len(),
            n_windows
        );
        assert_eq!(
            decode_f64(&r.effect.extra["window_starts_ms"]).len(),
            n_windows
        );
        assert_eq!(r.data_slice.sources.len(), 2);
    }

    /// Hand-derived: identical leg returns -> β = 1 per window (within 1e-12).
    #[test]
    fn ols_rolling_perfect_correlation_returns_beta_1() {
        // Build two legs with identical closes so log_returns are identical.
        let n = 32;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        let closes = lcg_closes(n, 99);
        let a = build_bars("EURUSD", &ts, &closes);
        let b = build_bars("GBPUSD", &ts, &closes);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 6}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        OlsRollingScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let betas = decode_f64(&r.effect.extra["betas"]);
        for (i, b_i) in betas.iter().enumerate() {
            assert!(
                (b_i - 1.0).abs() < 1e-12,
                "β[{i}] = {b_i}; expected 1.0 within 1e-12"
            );
        }
        // R² should also be 1.0 (perfect linear relationship).
        let r2s = decode_f64(&r.effect.extra["r2s"]);
        for (i, r2_i) in r2s.iter().enumerate() {
            assert!(
                (r2_i - 1.0).abs() < 1e-10,
                "R²[{i}] = {r2_i}; expected ~1.0"
            );
        }
    }

    /// Hand-derived: leg b = 2 * leg a (in close space) — log returns are
    /// equal so β = 1 (NOT 0.5; we're regressing `returns_a` ~ `returns_b`).
    /// This pins the convention.
    #[test]
    fn ols_rolling_known_beta_2x_close_scaling() {
        let n = 32;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        let closes_a = lcg_closes(n, 1);
        let closes_b: Vec<f64> = closes_a.iter().map(|c| 2.0 * c).collect();
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        OlsRollingScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let betas = decode_f64(&r.effect.extra["betas"]);
        // log(2c[t+1])/log(2c[t]) - log(c[t+1])/log(c[t]) = 0; i.e.
        // log_returns(2c) = log_returns(c) exactly. So β = 1 within 1e-12.
        for (i, b_i) in betas.iter().enumerate() {
            assert!(
                (b_i - 1.0).abs() < 1e-12,
                "β[{i}] = {b_i}; expected 1.0 (log returns are scale-invariant)"
            );
        }
    }

    #[test]
    fn ols_rolling_r2_unity_for_linear_relation() {
        // y = 2*x + 3 — exact linear relation; R² = 1 to within 1e-10.
        let x = [0.001_f64, 0.002, 0.003, 0.004, 0.005, 0.006, 0.007];
        let y: Vec<f64> = x.iter().map(|xi| 2.0 * xi + 0.5).collect();
        let res = kernel::rolling_ols(&y, &x, 5);
        for (i, r2_i) in res.r2s.iter().enumerate() {
            assert!(
                (r2_i - 1.0).abs() < 1e-10,
                "R²[{i}] = {r2_i}; expected ~1.0"
            );
        }
    }

    #[test]
    fn ols_rolling_cancellation_inside_window_loop() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(true)));
        OlsRollingScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel returns Ok");
        assert!(sink.0.is_empty());
    }

    #[test]
    fn ols_rolling_zero_variance_b_emits_scan_error() {
        // Leg b constant -> zero variance in log returns -> singular system.
        let n = 20;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        let closes_a: Vec<f64> = (0..n).map(|i| 1.0 + 0.01 * i as f64).collect();
        let closes_b = vec![1.0; n];
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = OlsRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("zero variance must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn ols_rolling_invalid_window() {
        let (a, b) = two_leg_fixture(20, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"window": 2}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = OlsRollingScan
            .run(&ctx, &req, &mut sink)
            .expect_err("window < 3 must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
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
}

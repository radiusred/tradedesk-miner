//! Lead-lag cross-correlation function scan (CROSS-04).
//!
//! Single-shot Pair-arity scan computing the cross-correlation function
//! (CCF) of two aligned log-return series over a symmetric ±`max_lag` grid,
//! plus the absolute-value argmax-lag (the strongest lead/lag signal,
//! regardless of sign). Reference: `scipy.signal.correlate(a, b,
//! mode='full')` (with normalisation) or `statsmodels.tsa.stattools.ccf`;
//! tolerance 1e-10.
//!
//! `effect.value` = `argmax_lag` cast to `f64` (integer-valued). `effect.extra`
//! carries `{lags, ccf_values, argmax_lag, argmax_value, max_lag}`. `raw.series`
//! carries `{returns_a, returns_b, timestamps_ms}` (D-03 canonical key for
//! the joint aligned timestamps — see `corr_rolling::FINDING_SHAPE` doc for
//! the rationale; the `_aligned` suffix is unambiguous because the raw block
//! only carries aligned data).
//!
//! ## `max_lag` default
//!
//! `max_lag` is an optional integer parameter with default `20`. RESEARCH
//! §1.9 recommends a timeframe-conditional default `{1m → 50, 15m → 20,
//! 1h → 10, 1d → 7}`; this scan ships a single default `20` for
//! implementation simplicity (no per-timeframe dispatch at the scan
//! boundary). The `param_schema` description string enumerates the
//! recommended overrides so callers see the timeframe-aware
//! recommendations directly in `miner scans` output. Users override via
//! `--params max_lag=N`. The single-default choice is documented in
//! `.planning/phases/04-scan-catalogue-anom-cross-seas/04-08-SUMMARY.md`.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    DataSlice, Effect, EffectSize, Finding, FindingSink, Raw, RawArray, ResultFinding, Source,
};
use crate::scan::primitives::raw_array::f64_slice_to_raw_array;
use crate::scan::primitives::returns::log_returns;
use crate::scan::primitives::time_alignment::inner_join;
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// Pair-arity lead-lag CCF scan (CROSS-04).
pub struct LeadLagCcfScan;

const SCAN_ID: &str = "cross.lead_lag.ccf";
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "lead_lag_argmax_lag";

/// Default `max_lag` — single value chosen over RESEARCH §1.9 timeframe-
/// conditional recommendation (1m→50, 15m→20, 1h→10, 1d→7) for simplicity.
/// The `param_schema` description enumerates the recommended overrides.
const DEFAULT_MAX_LAG: usize = 20;

const FINDING_SHAPE: ScanFindingShape = ScanFindingShape {
    effect_extra_keys: &[
        "argmax_lag",
        "argmax_value",
        "ccf_values",
        "lags",
        "max_lag",
    ],
    // D-03 invariant: Raw::new requires a `timestamps_ms` key. The
    // CROSS-scan body carries the aligned joint timestamps under this
    // canonical key (same data; see corr_rolling::FINDING_SHAPE doc).
    raw_series_keys: &["returns_a", "returns_b", "timestamps_ms"],
};

fn lead_lag_param_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "properties": {
            "max_lag": {
                "type": "integer",
                "minimum": 1,
                "default": 20,
                "description": "Symmetric lag-grid half-width: the CCF covers ±max_lag (output length = 2*max_lag + 1). Default 20. Recommended overrides by timeframe: 1m → 50, 15m → 20 (default), 1h → 10, 1d → 7. Simpler single-default chosen over per-timeframe dispatch — see 04-08-SUMMARY.md."
            }
        },
        "additionalProperties": false
    })
}

impl Scan for LeadLagCcfScan {
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
        lead_lag_param_schema()
    }
    fn finding_fields(&self) -> ScanFindingShape {
        FINDING_SHAPE
    }
    /// Phase 5 (Plan 05-03 / D5-04 / HYG-03) — opt-in to bootstrap CI.
    fn supports_bootstrap(&self) -> bool { true }

    /// Phase 5 (Plan 05-03 / D5-04 / HYG-04) — opt-in to null methods
    /// (`PhaseScramble` + `CircularShift`) per the per-scan matrix.
    fn supports_null_method(&self, m: crate::scan::NullMethod) -> bool {
        matches!(m, crate::scan::NullMethod::PhaseScramble | crate::scan::NullMethod::CircularShift)
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
        if aligned.timestamps_ms.is_empty() {
            return Err(ScanError::Kernel(
                "inner join produced zero overlap between legs".into(),
            ));
        }

        // 4. Per-leg log returns after alignment.
        let returns_a = log_returns(&aligned.close_a);
        let returns_b = log_returns(&aligned.close_b);
        let returns_n = returns_a.len();
        debug_assert_eq!(returns_n, returns_b.len());
        if returns_n < 4 {
            return Err(ScanError::Kernel(format!(
                "lead-lag CCF needs at least 4 aligned returns; got {returns_n}"
            )));
        }

        // 5. Resolve max_lag param. Default 20; must be >= 1 and < returns_n/2.
        let max_lag = resolve_max_lag(req, returns_n)?;

        // 6. Cancel-poll before kernel.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 7. Run kernel.
        let result = kernel::lead_lag_ccf(&returns_a, &returns_b, max_lag);

        // 8. NaN detection (zero-variance leg).
        if result.ccf_values.iter().any(|v| v.is_nan()) {
            return Err(ScanError::Kernel(
                "lead-lag CCF: zero-variance leg yields undefined correlation".into(),
            ));
        }

        // 9. Cancel-poll between kernel and envelope.
        if ctx.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }

        // 10. Effect.extra arrays. lags is integer-valued so we cast each
        //     entry to f64 (the wire RawArray is single-Dtype F64 in v1).
        let lags_f64: Vec<f64> = result
            .lags
            .iter()
            .map(|k| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "lag bounded by returns_n/2 << 2^52"
                )]
                let v = *k as f64;
                v
            })
            .collect();
        let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
        extra.insert("lags".into(), f64_slice_to_raw_array(&lags_f64));
        extra.insert(
            "ccf_values".into(),
            f64_slice_to_raw_array(&result.ccf_values),
        );
        #[allow(
            clippy::cast_precision_loss,
            reason = "argmax_lag bounded by max_lag << 2^52"
        )]
        let argmax_lag_f = result.argmax_lag as f64;
        extra.insert("argmax_lag".into(), f64_slice_to_raw_array(&[argmax_lag_f]));
        extra.insert(
            "argmax_value".into(),
            f64_slice_to_raw_array(&[result.argmax_value]),
        );
        #[allow(
            clippy::cast_precision_loss,
            reason = "max_lag bounded by returns_n/2 << 2^52"
        )]
        let max_lag_f = result.max_lag as f64;
        extra.insert("max_lag".into(), f64_slice_to_raw_array(&[max_lag_f]));

        // 11. effect.value = argmax_lag (integer encoded as f64).
        // Plan 05-03 / D5-03: argmax_ccf_value = the CCF magnitude at the
        // argmax lag (i.e., the strongest lead/lag signal value).
        let effect = Effect {
            metric: EFFECT_METRIC.to_string(),
            value: argmax_lag_f,
            p_value: None,
            #[allow(
                clippy::cast_possible_truncation,
                reason = "aligned_n <= u64 on supported targets (Phase 1 is 64-bit only)"
            )]
            n: Some(returns_n as u64),
            ci95: None,
            effect_size: Some(EffectSize {
                kind: "argmax_ccf_value".to_string(),
                value: result.argmax_value,
            }),
            extra,
        };

        // 12. raw.series — leg-labelled + aligned timestamps under canonical
        //     D-03 key. The per-return timestamps drop the first aligned
        //     timestamp (returns are one shorter than closes).
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

/// Resolve `max_lag` param — optional integer, default 20.
/// Constraints: `1 <= max_lag` and `max_lag < returns_n / 2` (a stricter
/// `2 * max_lag + 1 <= returns_n` would only require `< returns_n/2 + 0.5`,
/// but the per-lag `N_eff` = `returns_n - max_lag` needs >= 2 entries to be
/// statistically meaningful, so the `< returns_n/2` cap is the conservative
/// pick).
fn resolve_max_lag(req: &ScanRequest, returns_n: usize) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("max_lag");
    let max_lag: usize = match raw {
        None => DEFAULT_MAX_LAG,
        Some(v) => {
            let i = v
                .as_i64()
                .ok_or_else(|| ScanError::Kernel(format!("max_lag must be an integer; got {v}")))?;
            if i < 1 {
                return Err(ScanError::Kernel(format!("max_lag must be >= 1; got {i}")));
            }
            usize::try_from(i)
                .map_err(|_| ScanError::Kernel(format!("max_lag out of usize range: {i}")))?
        }
    };
    if max_lag >= returns_n / 2 {
        return Err(ScanError::Kernel(format!(
            "max_lag ({max_lag}) must be < returns_n/2 (= {}); got returns_n={returns_n}",
            returns_n / 2
        )));
    }
    Ok(max_lag)
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
    fn lead_lag_id_and_version() {
        let s = LeadLagCcfScan;
        assert_eq!(s.id(), "cross.lead_lag.ccf");
        assert_eq!(s.version(), 1);
    }

    #[test]
    fn lead_lag_arity_is_pair() {
        assert_eq!(LeadLagCcfScan.arity(), ScanArity::Pair);
        assert_eq!(LeadLagCcfScan.arity().expected_len(), 2);
    }

    #[test]
    fn lead_lag_param_schema() {
        let schema = LeadLagCcfScan.param_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["max_lag"]["type"], "integer");
        assert_eq!(schema["properties"]["max_lag"]["minimum"], 1);
        assert_eq!(schema["properties"]["max_lag"]["default"], 20);
        // No required params (max_lag has a default).
        assert!(
            schema.get("required").is_none()
                || schema["required"]
                    .as_array()
                    .is_none_or(std::vec::Vec::is_empty)
        );
        assert_eq!(schema["additionalProperties"], false);
    }

    /// Pin: the `param_schema` description must enumerate the timeframe-
    /// conditional override recommendations (RESEARCH §1.9) so callers
    /// see them in `miner scans` output without having to read SUMMARY.md.
    #[test]
    fn lead_lag_param_schema_documents_overrides() {
        let schema = LeadLagCcfScan.param_schema();
        let desc = schema["properties"]["max_lag"]["description"]
            .as_str()
            .expect("description is string");
        // Must list each timeframe override explicitly (RESEARCH §1.9).
        assert!(desc.contains("1m"), "missing 1m: {desc}");
        assert!(desc.contains("50"), "missing 50: {desc}");
        assert!(desc.contains("15m"), "missing 15m: {desc}");
        assert!(desc.contains("20"), "missing 20: {desc}");
        assert!(desc.contains("1h"), "missing 1h: {desc}");
        assert!(desc.contains("10"), "missing 10: {desc}");
        assert!(desc.contains("1d"), "missing 1d: {desc}");
        assert!(desc.contains('7'), "missing 7: {desc}");
    }

    #[test]
    fn lead_lag_invalid_max_lag_zero() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 0}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = LeadLagCcfScan
            .run(&ctx, &req, &mut sink)
            .expect_err("max_lag = 0 must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn lead_lag_invalid_max_lag_exceeds_data() {
        let (a, b) = two_leg_fixture(10, 1, 2);
        // returns_n = 9; max_lag = 50 -> >= 9/2 -> reject.
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 50}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = LeadLagCcfScan
            .run(&ctx, &req, &mut sink)
            .expect_err("max_lag >= returns_n/2 must reject");
        assert!(matches!(err, ScanError::Kernel(_)));
    }

    #[test]
    fn lead_lag_emits_one_result() {
        let (a, b) = two_leg_fixture(100, 7, 11);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        LeadLagCcfScan.run(&ctx, &req, &mut sink).expect("run ok");
        let findings = parse_findings(&sink);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], Finding::Result(_)));
    }

    #[test]
    fn lead_lag_result_envelope_shape() {
        let (a, b) = two_leg_fixture(100, 7, 11);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        LeadLagCcfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        assert_eq!(r.scan_id_at_version, "cross.lead_lag.ccf@1");
        assert_eq!(r.effect.metric, "lead_lag_argmax_lag");
        // effect.value is integer-valued argmax_lag cast to f64.
        assert_eq!(r.effect.value, r.effect.value.trunc());
        // effect.extra keys (BTreeMap iteration is alphabetical).
        let keys: Vec<&str> = r.effect.extra.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "argmax_lag",
                "argmax_value",
                "ccf_values",
                "lags",
                "max_lag"
            ]
        );
        // lags + ccf_values have length 2*max_lag + 1 = 11.
        let lags = decode_f64(&r.effect.extra["lags"]);
        let ccf = decode_f64(&r.effect.extra["ccf_values"]);
        assert_eq!(lags.len(), 11);
        assert_eq!(ccf.len(), 11);
        // raw.series carries the leg-labelled returns + canonical timestamps key.
        let raw = r.raw.as_ref().expect("raw present");
        assert!(raw.series.contains_key("returns_a"));
        assert!(raw.series.contains_key("returns_b"));
        assert!(raw.series.contains_key("timestamps_ms"));
        // D4-03: two-leg sources Vec.
        assert_eq!(r.data_slice.sources.len(), 2);
    }

    /// Lag-0 element of `ccf_values` equals the full-sample Pearson correlation
    /// of the two log-return series within 1e-10.
    #[test]
    fn lead_lag_zero_lag_returns_pearson_correlation() {
        let (a, b) = two_leg_fixture(100, 7, 11);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        LeadLagCcfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let ccf = decode_f64(&r.effect.extra["ccf_values"]);
        // Lag-0 index = max_lag = 5.
        let lag_zero = ccf[5];
        // Compute Pearson manually over log returns.
        let returns_a = log_returns(&a.close);
        let returns_b = log_returns(&b.close);
        #[allow(clippy::cast_precision_loss)]
        let n_f = returns_a.len() as f64;
        let mean_a = returns_a.iter().sum::<f64>() / n_f;
        let mean_b = returns_b.iter().sum::<f64>() / n_f;
        let var_a = returns_a.iter().map(|v| (v - mean_a).powi(2)).sum::<f64>() / n_f;
        let var_b = returns_b.iter().map(|v| (v - mean_b).powi(2)).sum::<f64>() / n_f;
        let cov = returns_a
            .iter()
            .zip(returns_b.iter())
            .map(|(va, vb)| (va - mean_a) * (vb - mean_b))
            .sum::<f64>()
            / n_f;
        let pearson = cov / (var_a.sqrt() * var_b.sqrt());
        assert!(
            (lag_zero - pearson).abs() < 1e-10,
            "lag-0 = {lag_zero}; full-sample Pearson = {pearson}"
        );
    }

    /// Known-shifted-series argmax: when `returns_b` is `returns_a` shifted by k
    /// bars (a leads b by k), `argmax_lag` should be k (within ±1).
    ///
    /// Uses chirp closes (non-stationary frequency) so the log-return CCF
    /// has a UNIQUE absolute maximum at the true shift. A pure sine would
    /// alias the argmax to ±k symmetrically (sine product-to-sum identity);
    /// the chirp's frequency drift breaks that symmetry.
    #[test]
    fn lead_lag_known_shifted_series_argmax_matches() {
        let n = 200;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        // Chirp closes centered around 1.0 so log returns are
        // non-stationary in frequency.
        let chirp = |i_f: f64| -> f64 {
            let phase = 0.2_f64 * i_f + 0.002_f64 * i_f * i_f;
            1.0 + 0.05 * phase.sin()
        };
        let closes_a: Vec<f64> = (0..n).map(|i| chirp(i as f64)).collect();
        // b is a shifted by k = 3 (a leads b by 3 bars in close space). Fill
        // the prefix by extrapolating the chirp backwards so the prefix log
        // returns continue the chirp pattern.
        let k = 3_usize;
        let mut closes_b = vec![1.0_f64; n];
        for t in 0..n {
            if t >= k {
                closes_b[t] = closes_a[t - k];
            } else {
                let back_i = (t as f64) - (k as f64);
                closes_b[t] = chirp(back_i);
            }
        }
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 10}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        LeadLagCcfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let argmax_lag = r.effect.value as i64;
        // The kernel's argmax-by-|abs| selection may land at ±1 of the true
        // shift on finite samples; accept ±1.
        assert!(
            (argmax_lag - 3).abs() <= 1,
            "argmax_lag={argmax_lag}; expected 3 (within ±1)"
        );
    }

    /// Lags vector is the ascending symmetric integer grid
    /// [-`max_lag`, ..., `max_lag`] of length 2*`max_lag` + 1.
    #[test]
    fn lead_lag_lag_grid_symmetric() {
        let (a, b) = two_leg_fixture(60, 17, 29);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 4}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        LeadLagCcfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let lags = decode_f64(&r.effect.extra["lags"]);
        let expected: Vec<f64> = (-4..=4).map(|k| k as f64).collect();
        assert_eq!(lags, expected);
    }

    #[test]
    fn lead_lag_cancellation() {
        let (a, b) = two_leg_fixture(40, 1, 2);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 5}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(true)));
        LeadLagCcfScan
            .run(&ctx, &req, &mut sink)
            .expect("cancel returns Ok");
        assert!(sink.0.is_empty(), "no envelope on cancel-at-entry");
    }

    #[test]
    fn lead_lag_zero_variance_emits_scan_error() {
        let n = 40;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts: Vec<DateTime<Utc>> = (0..n)
            .map(|i| {
                let i_i64 = i64::try_from(i).expect("n fits");
                start + Duration::minutes(15 * i_i64)
            })
            .collect();
        let closes_a = vec![1.0_f64; n]; // constant -> zero-variance log returns
        let closes_b = lcg_closes(n, 7);
        let a = build_bars("EURUSD", &ts, &closes_a);
        let b = build_bars("GBPUSD", &ts, &closes_b);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({"max_lag": 4}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        let err = LeadLagCcfScan
            .run(&ctx, &req, &mut sink)
            .expect_err("zero-variance leg must reject");
        match err {
            ScanError::Kernel(msg) => {
                assert!(msg.contains("zero-variance"), "msg: {msg}");
            }
            other => panic!("expected Kernel; got {other:?}"),
        }
    }

    /// Default `max_lag` is 20 when the param is absent; verify the scan
    /// emits a result with 41 `ccf_values` (=2*20+1) when given enough data.
    #[test]
    fn lead_lag_default_max_lag_is_20() {
        // returns_n = 99; max_lag = 20; 20 < 99/2 = 49 -> accepted.
        let (a, b) = two_leg_fixture(100, 19, 23);
        let mut sink = VecSink::new();
        let req = sample_request(serde_json::json!({}));
        let ctx = make_ctx(&a, &b, Arc::new(AtomicBool::new(false)));
        LeadLagCcfScan.run(&ctx, &req, &mut sink).expect("ok");
        let findings = parse_findings(&sink);
        let Finding::Result(r) = &findings[0] else {
            panic!("expected Result");
        };
        let ccf = decode_f64(&r.effect.extra["ccf_values"]);
        assert_eq!(ccf.len(), 2 * DEFAULT_MAX_LAG + 1);
        let max_lag_extra = decode_f64(&r.effect.extra["max_lag"]);
        assert_eq!(max_lag_extra.len(), 1);
        assert_eq!(max_lag_extra[0], DEFAULT_MAX_LAG as f64);
    }
}

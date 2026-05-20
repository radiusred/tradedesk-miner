//! Per-scan stat-closure + input-series dispatch table for the post-`Scan::run`
//! hygiene-kernel invocation (Plan 05-03 / D5-04 / HYG-03 + HYG-04).
//!
//! ## Purpose
//!
//! The `hygiene::bootstrap::stationary_bootstrap_ci` and
//! `hygiene::null::circular_shift_null_p` kernels (Plan 05-02) operate on a
//! plain `&[f64]` series and take a `stat: impl Fn(&[f64]) -> f64` closure.
//! Each per-scan opt-in (per the D5-04 matrix) needs to expose TWO things:
//!
//! 1. **Input series** — the `Vec<f64>` over which resampling happens
//!    (typically `log_returns(&ctx.bars.close)` for ANOM scans; some scans
//!    resample over `ctx.bars.close` directly).
//! 2. **Stat closure** — a recipe that, given a resampled slice, recomputes
//!    the SAME scalar statistic the scan's `Effect.value` carries.
//!
//! This module centralises both into two `fn`-returning dispatch helpers
//! keyed on `scan_id@version`. Scans whose row in the D5-04 matrix says
//! `supports_bootstrap == false` AND every `supports_null_method == false`
//! return `None` from `input_series_for` (defensive — preflight already
//! rejects unsupported requests via `validate_hygiene_support`, but the
//! belt-and-braces `None` keeps the engine call site explicit).
//!
//! ## Scope (Plan 05-03 continuation)
//!
//! Single-arity scans for which a clean scalar stat closure exists are wired
//! in this continuation:
//!
//! - `stats.autocorr.ljung_box@1` — input: log returns; stat: Q-stat at the
//!   resolved `lags` parameter.
//! - `stats.summary.welford@1` — input: log returns (matching the default
//!   `series=log_returns`); stat: mean.
//!
//! Pair-arity CROSS scans (Pearson / Spearman / OLS / lead-lag / Engle-
//! Granger) and the rest of the Single-arity scans (ANOM-04..11, SEAS-*)
//! are DEFERRED to Phase 7 — their stat closures need either joint per-leg
//! resampling (Pair-arity) or library reuse that would re-export private
//! kernel state (ANOM-04 ARCH-LM, ANOM-09 Jarque-Bera). The continuation's
//! SUMMARY.md enumerates them.
//!
//! Scans NOT wired here cause the engine to leave `effect.ci95` /
//! `effect.p_value` untouched even when the caller passes bootstrap/null
//! flags AND `validate_hygiene_support` returns Ok — the user-visible
//! effect is that the request silently no-ops on those scans. Plan 05-03's
//! SUMMARY documents this gap so Phase 7 can close it without surprises.

use crate::scan::ScanRequest;
use crate::scan::primitives::returns::log_returns;

/// Statistic closure type. The hygiene kernels accept any
/// `Fn(&[f64]) -> f64`; this alias keeps engine call sites concise.
pub(crate) type StatClosure = Box<dyn Fn(&[f64]) -> f64 + Send + Sync>;

/// Per-scan resample input.
///
/// Returns the `Vec<f64>` over which the hygiene kernels resample.
/// `None` ⇒ the scan is not wired into the hygiene-dispatch table (the
/// engine treats this as "leave `effect.ci95` / `effect.p_value` untouched").
///
/// All series in the dispatch table are derived FROM `ctx.bars.close` to
/// avoid a separate parameter resolution path — the per-scan `Scan::run`
/// body owns parameter-derived series (e.g. squared returns for
/// `LjungBoxSq`); the hygiene path uses the canonical `log_returns(close)`
/// series unless a scan explicitly needs raw closes (none in v1).
pub(crate) fn input_series_for(scan_id_at_version: &str, close: &[f64]) -> Option<Vec<f64>> {
    match scan_id_at_version {
        "stats.autocorr.ljung_box@1" | "stats.summary.welford@1" => {
            // Both scans resample over log returns. LjungBox's Q-stat is a
            // function of the autocorrelation of returns; Welford's mean is
            // the mean of (default) log returns.
            if close.len() < 2 {
                return None;
            }
            Some(log_returns(close))
        }
        _ => None,
    }
}

/// Per-scan statistic closure.
///
/// Returns a `Box<dyn Fn(&[f64]) -> f64>` that recomputes the scalar
/// scan-output statistic over a resampled slice. The bootstrap kernel
/// passes resampled slices through this closure to build the empirical
/// statistic distribution; the null kernel passes circularly-shifted
/// slices through the SAME closure to build the empirical null
/// distribution.
///
/// `req` is borrowed so the closure can resolve scan parameters
/// (e.g. `LjungBox`'s `lags`) at construction time.
///
/// `None` ⇒ the scan is not wired into the dispatch table (mirror of
/// `input_series_for`'s `None`).
pub(crate) fn stat_closure_for(scan_id_at_version: &str, req: &ScanRequest) -> Option<StatClosure> {
    match scan_id_at_version {
        "stats.autocorr.ljung_box@1" => {
            // LjungBox: Q-stat at max(lags). The `lags` parameter is
            // resolved at the same default the scan body uses
            // (min(10, n/5)) when absent. We snapshot the user-supplied
            // value here; if the param is None the closure derives it
            // from the resample's `n` at evaluation time.
            let lags_override: Option<usize> = req
                .resolved_params
                .get("lags")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| usize::try_from(v).ok());
            Some(Box::new(move |slice: &[f64]| {
                let n = slice.len();
                if n < 2 {
                    return 0.0;
                }
                let lags = lags_override.unwrap_or_else(|| default_ljungbox_lags(n));
                if lags < 1 || lags >= n {
                    return 0.0;
                }
                // Recompute the biased ACF + Q-stat using the same kernel
                // the scan body uses, so the stat matches `Effect.value`
                // exactly on the original input.
                let acf = crate::scan::ljung_box::kernel::biased_acf(slice, lags);
                let (q_stats, _) = crate::scan::ljung_box::kernel::ljung_box_q_and_p(n, &acf, lags);
                // Q-stat at the maximum lag (matches Effect.value's
                // `q_stats[lags - 1]` choice).
                q_stats[lags - 1]
            }))
        }
        "stats.summary.welford@1" => {
            // Welford: mean of the resampled series. We deliberately do
            // NOT use the full Welford pass here — the bootstrap CI is for
            // the mean (Effect.value); using mean directly keeps the
            // closure cheap (n*B work for B resamples instead of N*B).
            Some(Box::new(move |slice: &[f64]| {
                if slice.is_empty() {
                    return 0.0;
                }
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "len fits in f64 mantissa for any realistic OHLCV series"
                )]
                let n = slice.len() as f64;
                slice.iter().copied().sum::<f64>() / n
            }))
        }
        _ => None,
    }
}

/// D3-03 default: `min(10, n / 5)`. Mirrors `LjungBoxScan::default_lags`.
#[inline]
fn default_ljungbox_lags(n: usize) -> usize {
    (n / 5).min(10)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregator::Timeframe;
    use crate::engine::gap_policy::GapPolicyKind;
    use crate::engine::param_hash;
    use crate::findings::TimeRange;
    use crate::reader::{Blake3Hex, ClosedRangeUtc, InstrumentSpec, Side};
    use chrono::{TimeZone, Utc};

    fn make_req(scan_id: &str, params: &serde_json::Value) -> ScanRequest {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 30, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: scan_id.into(),
            version: 1,
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
            resolved_params: params.clone(),
            param_hash: param_hash::param_hash(params).expect("hash ok"),
            dry_run: false,
            master_seed: None,
            job_seed: None,
            bootstrap_method: None,
            bootstrap_n: None,
            null_method: None,
            null_n: None,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        }
    }

    fn blake3_hex_zero() -> Blake3Hex {
        Blake3Hex::from_hex_bytes(&[b'0'; 64])
    }

    /// Sanity: `input_series_for` returns `None` for an unwired scan id.
    #[test]
    fn input_series_returns_none_for_unwired_scan() {
        let closes = vec![1.0_f64, 2.0, 3.0, 4.0];
        assert!(input_series_for("cross.cointegration.engle_granger@1", &closes).is_none());
        assert!(input_series_for("seas.bucket.hour_of_day@1", &closes).is_none());
    }

    /// `input_series_for` returns log returns of length `n - 1` for `LjungBox` + Welford.
    #[test]
    fn input_series_returns_log_returns_for_wired_scans() {
        let closes = vec![1.0_f64, 2.0, 3.0, 4.0];
        let lb = input_series_for("stats.autocorr.ljung_box@1", &closes).expect("Some");
        let wf = input_series_for("stats.summary.welford@1", &closes).expect("Some");
        assert_eq!(lb.len(), 3);
        assert_eq!(wf.len(), 3);
        // Both wired scans use the same input series.
        assert_eq!(lb, wf);
    }

    /// `input_series_for` rejects short closes.
    #[test]
    fn input_series_rejects_short_closes() {
        let closes = vec![1.0_f64];
        assert!(input_series_for("stats.autocorr.ljung_box@1", &closes).is_none());
        let empty: Vec<f64> = vec![];
        assert!(input_series_for("stats.summary.welford@1", &empty).is_none());
    }

    /// `stat_closure_for` returns `None` for unwired scans.
    #[test]
    fn stat_closure_returns_none_for_unwired_scan() {
        let req = make_req("cross.lead_lag.ccf", &serde_json::json!({}));
        assert!(stat_closure_for("cross.lead_lag.ccf@1", &req).is_none());
    }

    /// `LjungBox` stat closure recomputes the Q-stat consistent with
    /// `LjungBoxScan::run`'s `Effect.value`. We compare against a
    /// hand-driven kernel call on the same input + same lags.
    #[test]
    fn ljungbox_stat_closure_matches_kernel_q_stat() {
        let req = make_req("stats.autocorr.ljung_box", &serde_json::json!({"lags": 5}));
        let closure = stat_closure_for("stats.autocorr.ljung_box@1", &req).expect("Some");
        // Synthetic series.
        let series: Vec<f64> = (0..50).map(|i| f64::from(i % 5) * 0.01).collect();
        let q_closure = closure(&series);
        // Independent kernel evaluation.
        let acf = crate::scan::ljung_box::kernel::biased_acf(&series, 5);
        let (q_stats, _) = crate::scan::ljung_box::kernel::ljung_box_q_and_p(series.len(), &acf, 5);
        let q_kernel = q_stats[4];
        assert_eq!(q_closure.to_bits(), q_kernel.to_bits(), "closure must match kernel");
    }

    /// Welford stat closure returns the arithmetic mean.
    #[test]
    fn welford_stat_closure_returns_arithmetic_mean() {
        let req = make_req("stats.summary.welford", &serde_json::json!({}));
        let closure = stat_closure_for("stats.summary.welford@1", &req).expect("Some");
        let series = vec![1.0_f64, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(closure(&series).to_bits(), 3.0_f64.to_bits());
    }

    /// Welford stat closure handles empty slices gracefully (returns 0.0).
    #[test]
    fn welford_stat_closure_handles_empty_slice() {
        let req = make_req("stats.summary.welford", &serde_json::json!({}));
        let closure = stat_closure_for("stats.summary.welford@1", &req).expect("Some");
        let empty: [f64; 0] = [];
        assert_eq!(closure(&empty).to_bits(), 0.0_f64.to_bits());
    }

    /// `LjungBox` stat closure handles short slices gracefully (returns 0.0).
    #[test]
    fn ljungbox_stat_closure_handles_short_slice() {
        let req = make_req("stats.autocorr.ljung_box", &serde_json::json!({"lags": 5}));
        let closure = stat_closure_for("stats.autocorr.ljung_box@1", &req).expect("Some");
        let short = [1.0_f64];
        assert_eq!(closure(&short).to_bits(), 0.0_f64.to_bits());
    }

    // Tag-only: surface `blake3_hex_zero` so it's not flagged unused if we
    // extend the test fixtures later.
    #[allow(dead_code)]
    fn _suppress_unused() {
        let _ = blake3_hex_zero();
    }
}

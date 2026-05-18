//! `LjungBoxScan` — Phase 3 demo scan implementing the [`Scan`] trait.
//!
//! Pattern analog: `aggregator.rs::aggregate` — pure-kernel function calling a
//! `Reader`, returning a typed output. The Ljung-Box scan mirrors this shape
//! but reads from the brokering [`ScanCtx`] (Plan 02 wires it) and writes one
//! `Finding::Result` to the sink.
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
//! Wave 0 scaffold: signature only. Plan 04 fills the kernel.
//!
//! [`Scan`]: crate::scan::Scan
//! [`ScanCtx`]: crate::scan::ScanCtx

#![allow(dead_code, unused_variables)]

use crate::findings::FindingSink;
use crate::scan::{Scan, ScanCtx, ScanError, ScanFindingShape, ScanRequest};

pub mod kernel;

/// Phase 3 demo scan — Ljung-Box on log returns of a bar series.
///
/// Stateless unit struct (no fields) — pattern: `gap.rs:152-155` `GapDetector`.
pub struct LjungBoxScan;

impl Scan for LjungBoxScan {
    fn id(&self) -> &'static str {
        // D3-17 naming: <family>.<subfamily>.<scan_name>.
        "stats.autocorr.ljung_box"
    }

    fn version(&self) -> u32 {
        // D3-01: Phase 3 ships v1; bumps on output-shape change.
        1
    }

    fn param_schema(&self) -> serde_json::Value {
        unimplemented!(
            "Plan 04 (03-04-PLAN) wires param_schema; will declare \
             {{type: object, properties: {{lags: {{type: integer, min: 1}}}}}}"
        )
    }

    fn finding_fields(&self) -> ScanFindingShape {
        // D3-04 — declarative key list (compile-time constants). Wave 0
        // ships the literal so `miner scans` introspection compiles; Plan 04
        // confirms the kernel emits these exact keys.
        ScanFindingShape {
            effect_extra_keys: &["lags", "q_stats", "p_values", "acf"],
            raw_series_keys: &["returns", "timestamps_ms"],
        }
    }

    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError> {
        unimplemented!(
            "Plan 04 (03-04-PLAN) wires LjungBoxScan::run; will: \
             1) cancel-check; 2) resolve lags; 3) compute log returns from BarFrame.close; \
             4) call kernel::biased_acf + ljung_box_q_and_p; \
             5) build ResultFinding (effect.value = Q[max_lag], extras + raw); \
             6) sink.write_envelope; 7) sink.flush."
        )
    }
}

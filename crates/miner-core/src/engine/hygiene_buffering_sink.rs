//! `HygieneBufferingSink` — intercepts `Finding::Result` envelopes between
//! scan-body emission and the downstream sink so the engine can mutate
//! them (populate `Effect.ci95` / replace `Effect.p_value` / attach
//! `ResultFinding.repro`) before they are written through (Plan 05-03
//! continuation / D5-04 / HYG-03 + HYG-04).
//!
//! ## Architectural choice
//!
//! Plan 05-03's wire-contract leaves `Scan::run` itself unchanged — every
//! scan continues to call `sink.write_envelope(&Finding::Result(...))`
//! during its body. The engine wraps the user-supplied sink in a
//! `HygieneBufferingSink` BEFORE calling `Scan::run`.
//!
//! ## Hygiene-on vs hygiene-off mode
//!
//! The wrapper carries a `hygiene_active: bool` flag set at construction
//! time:
//!
//! - **`hygiene_active == false`** (no `bootstrap_method` / `null_method`
//!   in the request, OR the scan is not wired in
//!   `hygiene_dispatch`): the wrapper is a thin pass-through; every
//!   envelope (including `Finding::Result(_)`) goes straight to the inner
//!   sink. **This preserves the per-envelope-flush streaming contract
//!   (PITFALLS #4) — Results land on stdout BEFORE any in-`Scan::run`
//!   cancel-aware sleep loop runs, which the Plan 06 SIGINT integration
//!   test depends on.**
//! - **`hygiene_active == true`**: `Finding::Result(_)` envelopes are
//!   BUFFERED into an internal `Vec` for post-`Scan::run` mutation; every
//!   other variant flows through immediately so framing
//!   (`RunStart`/`ScanError`/`GapAborted`/`RunEnd`) stays interleaved in
//!   emission order.
//!
//! After `Scan::run` returns Ok, the engine drains the buffered Results
//! via `into_parts()`, applies hygiene mutations, then writes the
//! mutated envelopes to the inner sink. On `Scan::run` returning Err,
//! the buffered Results are dropped (mirroring the pre-Plan-05-03
//! behaviour where the engine emits a `Finding::ScanError` instead and
//! never writes the in-flight Result).
//!
//! ## Cancel discipline
//!
//! The buffering sink itself does not poll cancel. The engine checks
//! `cancel.load(Ordering::Relaxed)` BETWEEN the scan-body and the hygiene
//! drain (RESEARCH Pitfall 7 cadence — outer-engine cadence N=64 is
//! preserved by the existing per-sub-range cancel poll; this adds one
//! more poll point per scan invocation). On cancel, the buffered Results
//! are dropped without writing through.

use crate::error::MinerError;
use crate::findings::{Finding, FindingSink, ResultFinding};

/// Buffering sink wrapper.
///
/// Holds an exclusive borrow of the user's `&mut dyn FindingSink` plus an
/// internal `Vec<ResultFinding>` for buffered Results.
///
/// The `hygiene_active` flag (set at construction time) determines whether
/// `Finding::Result(_)` envelopes are BUFFERED for post-`Scan::run`
/// mutation (`true`) or PASS THROUGH immediately to the inner sink
/// (`false`). The pass-through mode preserves the per-envelope-flush
/// streaming contract (PITFALLS #4) that the Plan 06 SIGINT integration
/// test depends on.
pub(crate) struct HygieneBufferingSink<'a> {
    inner: &'a mut dyn FindingSink,
    buffered_results: Vec<ResultFinding>,
    hygiene_active: bool,
}

impl<'a> HygieneBufferingSink<'a> {
    /// Wrap an inner sink. `hygiene_active` controls whether
    /// `Finding::Result(_)` envelopes buffer for mutation (`true`) or
    /// flow straight through to the inner sink (`false`).
    pub(crate) fn new(inner: &'a mut dyn FindingSink, hygiene_active: bool) -> Self {
        Self {
            inner,
            buffered_results: Vec::new(),
            hygiene_active,
        }
    }

    /// Consume the wrapper and return the buffered Results + a mutable
    /// borrow of the inner sink so the engine can drain after mutation.
    /// When `hygiene_active == false` the buffered Vec is always empty.
    pub(crate) fn into_parts(self) -> (Vec<ResultFinding>, &'a mut dyn FindingSink) {
        (self.buffered_results, self.inner)
    }

    /// Number of buffered Results (for diagnostic logging / cancel paths).
    #[allow(dead_code)]
    pub(crate) fn buffered_len(&self) -> usize {
        self.buffered_results.len()
    }
}

impl FindingSink for HygieneBufferingSink<'_> {
    fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError> {
        if self.hygiene_active {
            if let Finding::Result(r) = finding {
                // Buffer for post-scan-body mutation. We clone the
                // ResultFinding (the wire envelope is otherwise consumed
                // by serde_json::to_writer at write-time, so cloning
                // here is the only way to defer emission).
                self.buffered_results.push(r.clone());
                return Ok(());
            }
        }
        // Pass-through path: hygiene_active == false OR the envelope is
        // not a Result. Preserves per-envelope flush (PITFALLS #4).
        self.inner.write_envelope(finding)
    }

    fn write_raw_json(&mut self, v: &serde_json::Value) -> std::io::Result<()> {
        self.inner.write_raw_json(v)
    }

    fn flush(&mut self) -> Result<(), MinerError> {
        self.inner.flush()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::sink::VecSink;
    use crate::findings::{
        DataSlice, Effect, Raw, ResultFinding, RunEnd, RunId, RunStart, RunSummary, TimeRange,
    };
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;

    fn sample_run_start() -> Finding {
        Finding::RunStart(RunStart {
            run_id: RunId::new(),
            started_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            miner_version: "0.0.0".into(),
            code_revision: "abc1234".into(),
            request: serde_json::json!({}),
        })
    }

    fn sample_result() -> ResultFinding {
        ResultFinding {
            schema_version: 1,
            scan_id_at_version: "stats.autocorr.ljung_box@1".into(),
            param_hash: "0".repeat(64),
            code_revision: "abc1234".into(),
            data_slice: DataSlice {
                range: TimeRange {
                    start_utc: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                    end_utc: Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(),
                },
                gap_manifest_ref: None,
                gap_manifest: None,
                sources: Vec::new(),
            },
            dsr: None,
            fdr_q: None,
            run_id: RunId::new(),
            produced_at_utc: Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap(),
            params: serde_json::json!({}),
            effect: Effect {
                metric: "ljung_box_q".into(),
                value: 1.0,
                p_value: Some(0.5),
                n: Some(100),
                ci95: None,
                effect_size: None,
                extra: BTreeMap::new(),
            },
            raw: Some(Raw {
                series: BTreeMap::new(),
            }),
            repro: None,
        }
    }

    fn sample_run_end() -> Finding {
        Finding::RunEnd(RunEnd {
            run_id: RunId::new(),
            ended_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap(),
            wall_clock_ms: 1000,
            summary: RunSummary::default(),
        })
    }

    /// `hygiene_active == true`: `HygieneBufferingSink` forwards non-Result
    /// envelopes to the inner sink immediately AND buffers Results for
    /// later draining.
    #[test]
    fn forwards_non_result_and_buffers_result_when_active() {
        let mut inner = VecSink::new();
        let mut sink = HygieneBufferingSink::new(&mut inner, true);
        sink.write_envelope(&sample_run_start()).expect("ok");
        let r = sample_result();
        sink.write_envelope(&Finding::Result(r.clone())).expect("ok");
        sink.write_envelope(&sample_run_end()).expect("ok");
        assert_eq!(sink.buffered_len(), 1, "one Result buffered");
        let (buffered, _inner_borrow) = sink.into_parts();
        assert_eq!(buffered.len(), 1);
        let lines: Vec<&[u8]> = inner.0.split(|&b| b == b'\n').filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines.len(),
            2,
            "exactly two non-Result envelopes forwarded (RunStart + RunEnd)"
        );
    }

    /// `hygiene_active == false`: ALL envelopes (including Results) flow
    /// through to the inner sink immediately — no buffering. This is the
    /// path the Plan 06 SIGINT integration test depends on
    /// (per-envelope-flush PITFALLS #4 contract).
    #[test]
    fn passes_through_results_when_hygiene_inactive() {
        let mut inner = VecSink::new();
        let mut sink = HygieneBufferingSink::new(&mut inner, false);
        sink.write_envelope(&sample_run_start()).expect("ok");
        let r = sample_result();
        sink.write_envelope(&Finding::Result(r.clone())).expect("ok");
        sink.write_envelope(&sample_run_end()).expect("ok");
        assert_eq!(sink.buffered_len(), 0, "no Result buffered when hygiene off");
        let (buffered, _) = sink.into_parts();
        assert!(buffered.is_empty());
        let lines: Vec<&[u8]> = inner.0.split(|&b| b == b'\n').filter(|l| !l.is_empty()).collect();
        assert_eq!(
            lines.len(),
            3,
            "all three envelopes forwarded (RunStart + Result + RunEnd) when hygiene off"
        );
    }

    /// Empty drain — no Results buffered, `into_parts` returns an empty Vec.
    #[test]
    fn empty_drain_returns_empty_vec() {
        let mut inner = VecSink::new();
        let sink = HygieneBufferingSink::new(&mut inner, true);
        let (buffered, _) = sink.into_parts();
        assert!(buffered.is_empty());
    }

    /// Multiple Results preserve emission order in the buffer.
    #[test]
    fn buffer_preserves_emission_order() {
        let mut inner = VecSink::new();
        let mut sink = HygieneBufferingSink::new(&mut inner, true);
        let mut r1 = sample_result();
        r1.effect.value = 1.0;
        let mut r2 = sample_result();
        r2.effect.value = 2.0;
        let mut r3 = sample_result();
        r3.effect.value = 3.0;
        sink.write_envelope(&Finding::Result(r1)).expect("ok");
        sink.write_envelope(&Finding::Result(r2)).expect("ok");
        sink.write_envelope(&Finding::Result(r3)).expect("ok");
        let (buffered, _) = sink.into_parts();
        assert_eq!(buffered.len(), 3);
        assert_eq!(buffered[0].effect.value.to_bits(), 1.0_f64.to_bits());
        assert_eq!(buffered[1].effect.value.to_bits(), 2.0_f64.to_bits());
        assert_eq!(buffered[2].effect.value.to_bits(), 3.0_f64.to_bits());
    }
}

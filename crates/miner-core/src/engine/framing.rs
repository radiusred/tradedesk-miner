//! `RunStart` / `RunEnd` framing-record builders (D-09, D-11).
//!
//! Pattern analog: `miner-cli/src/main.rs::emit_fixture` (lines 140-165) — the
//! existing `RunStart` + `RunEnd` construction with shared `RunId: Copy`.
//! Phase 3 lifts the pattern out of `emit-fixture` into pure builder functions
//! the facade calls.
//!
//! ## Clock-read discipline (D3-23)
//!
//! `chrono::Utc::now()` is called ONLY here — in the framing builders — never
//! inside `Scan::run` or the kernel functions. This is the determinism
//! guarantee: same inputs → same JSONL output bytes modulo `run_id` +
//! timestamps + `wall_clock_ms` (the four volatile fields the determinism
//! test masks; pattern: `cli_streams.rs:323-344`).
//!
//! Wave 0 scaffold: signature only. Plan 03-02 fills the bodies.

#![allow(dead_code, unused_variables)]

use chrono::{DateTime, Utc};

use crate::findings::{Finding, RunSummary};
use crate::findings::run_id::RunId;
use crate::scan::ScanRequest;

/// Build the opening `Finding::RunStart` envelope.
///
/// `run_id` is supplied by the caller (so the facade can share the same
/// `RunId` across `RunStart` and `RunEnd` — relies on `RunId: Copy`).
/// `started` is the caller-captured `Utc::now()` reading (so the caller can
/// also compute `wall_clock_ms` against the same baseline for `RunEnd`).
///
/// `code_revision` is `miner_core::CODE_REVISION` at the call site — the
/// builder is `code_revision`-agnostic so tests can inject a stable string.
///
/// Wave 0 scaffold: signature only. Plan 03-02 fills the body.
#[must_use]
pub fn build_run_start(
    req: &ScanRequest,
    run_id: RunId,
    started: DateTime<Utc>,
    code_revision: &str,
) -> Finding {
    unimplemented!(
        "Plan 03-02 wires build_run_start; will mirror miner-cli/src/main.rs:145-152 \
         emit_fixture's RunStart construction with `request` built from the ScanRequest"
    )
}

/// Build the closing `Finding::RunEnd` envelope.
///
/// `wall_clock_ms` is computed from `ended.signed_duration_since(started)`
/// (mirror `miner-cli/src/main.rs:158`).
///
/// Wave 0 scaffold: signature only. Plan 03-02 fills the body.
#[must_use]
pub fn build_run_end(
    run_id: RunId,
    started: DateTime<Utc>,
    ended: DateTime<Utc>,
    summary: RunSummary,
) -> Finding {
    unimplemented!(
        "Plan 03-02 wires build_run_end; will mirror miner-cli/src/main.rs:155-160 \
         emit_fixture's RunEnd construction"
    )
}

//! Phase 3 facade — single library entry point CLI/MCP/HTTP all call.
//!
//! Pattern analog: `cache.rs:519-573` ([`crate::cache::BarCache::get_or_build`]) —
//! a single-method facade returning a value with a multi-line algorithm doc.
//! `engine::run_one` follows the same shape — it OWNS `RunStart`/`RunEnd`
//! framing emission, `param_hash` computation, run-id assignment, sink
//! dispatch, error classification, and the `RunOutcome` the CLI maps to an
//! exit code.
//!
//! ## Wave 0 scaffold
//!
//! Plan 03-01 lays down signature-only bodies (`unimplemented!()`). The seven
//! sub-files (`preflight`, `gap_policy`, `param_hash`, `framing`) carry the
//! same scaffold discipline so Plan 03-02..06 fill in real behaviour without
//! adding files.
//!
//! ## Module decomposition (D3-15 broker + D3-22 cancel + D3-24 exit-code routing)
//!
//! - [`preflight`] — parse + validate `--params`, reject unknown scans.
//! - [`gap_policy`] — strict / `continuous_only` dispatch + sub-range partitioning.
//! - [`param_hash`] — blake3 hash of canonical resolved params (D3-13).
//! - [`framing`] — `RunStart` / `RunEnd` envelope builders (clock reads ONLY here).
//!
//! The facade is sync + std-only (FOUND-04). No tokio, no async-std, no async
//! traits — Phase 5 will fan out via rayon, never tokio.

// `run_one` is still a scaffold body (Plan 04 fills) — its `cancel: Arc<AtomicBool>`
// argument is unused inside the `unimplemented!()` stub, so clippy's
// `needless_pass_by_value` lint fires spuriously. Plan 04's real body will consume
// the arg (passing it into `ScanCtx`); the allow can be removed once that lands.
#![allow(dead_code, unused_variables, clippy::needless_pass_by_value)]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::config::MinerConfig;
use crate::error::MinerError;
use crate::findings::FindingSink;
use crate::reader::Reader;
use crate::scan::ScanRequest;

pub mod framing;
pub mod gap_policy;
pub mod param_hash;
pub mod preflight;

// Plan 04 imports these via `use miner_core::engine::*;` — re-export the
// two enums so callers don't need to spell out the inner module path.
pub use gap_policy::{GapDispatch, GapPolicyKind};

// ---------------------------------------------------------------------------
// RunOutcome — internal enum the CLI maps to an exit code (D3-24).
// ---------------------------------------------------------------------------

/// Outcome of a single [`run_one`] invocation.
///
/// Pattern analog: `gap.rs:117-130` `GapReason` — tagged enum with explicit
/// `Eq`-safe variants (no `f64` inside). `RunOutcome` is INTERNAL — no
/// `Serialize` derive needed; the CLI maps it to a POSIX exit code per D3-24:
///
/// | `RunOutcome`            | exit code |
/// |-------------------------|-----------|
/// | `Ok`                    | `0` (or `2` if SIGINT mid-run → `130`) |
/// | `HadScanErrors`         | `2` |
/// | `PreflightFailed`       | `1` |
///
/// SIGINT (D3-22) is detected on the cancellation flag AFTER `run_one` returns
/// and overrides the outcome — the CLI emits exit code `130` regardless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// `RunEnd` emitted; at least one `Result` / `GapAborted` / `DryRun` finding
    /// streamed (or zero findings if the scan computed none).
    Ok,
    /// `RunEnd` emitted AND at least one mid-stream `Finding::ScanError` was
    /// emitted. Stream may have a mix of `Result` + `ScanError` findings.
    HadScanErrors,
    /// Pre-flight rejection: unknown scan, invalid param, missing config, etc.
    /// Stdout is empty; the CLI writes a single `WireError` JSON line to
    /// stderr and exits 1.
    PreflightFailed,
}

// ---------------------------------------------------------------------------
// run_one — the single-entry facade. Pattern: cache.rs:569-573.
// ---------------------------------------------------------------------------

/// Execute one scan request end-to-end.
///
/// Pattern analog: `cache.rs:569-573` ([`crate::cache::BarCache::get_or_build`])
/// — a single-method facade returning a value with a numbered algorithm doc.
///
/// ## Algorithm (Plan 02..06 fill the body)
///
/// 1. Cancel-check early (RESEARCH Pattern 4 polling site 1).
/// 2. Preflight: resolve scan from registry, validate params via
///    [`preflight::parse_params_kv`]; on failure return
///    [`RunOutcome::PreflightFailed`].
/// 3. Compute [`param_hash::param_hash`] over resolved params (D3-13).
/// 4. Emit `RunStart` via [`framing::build_run_start`].
/// 5. Fetch the [`crate::aggregator::BarFrame`] via
///    [`crate::cache::BarCache::get_or_build`].
/// 6. Detect gaps via [`crate::gap::GapDetector::detect`]; dispatch via
///    [`gap_policy::dispatch`]:
///    - `Aborted(manifest)` → emit `Finding::GapAborted`; return `Ok`.
///    - `SubRanges(ranges)` → for each sub-range, call `Scan::run` with a
///      fresh `ScanRequest::sub_range`.
/// 7. Emit `RunEnd` via [`framing::build_run_end`].
/// 8. Return `RunOutcome::{Ok | HadScanErrors}` based on whether any
///    `Finding::ScanError` was emitted mid-run.
///
/// ## SIGINT (D3-22)
///
/// The `cancel` flag is polled by the scan kernel (between findings) and by
/// the rayon fanout (between sub-ranges). On observed cancellation the
/// function returns `Ok(RunOutcome::Ok)` after emitting `RunEnd` (so any
/// streamed Result findings survive — OP-06); the CLI inspects the flag
/// post-return and exits 130.
///
/// # Errors
/// Returns [`MinerError`] only on infrastructure failure (sink IO, cache
/// corruption). Per-finding scan errors are emitted as `Finding::ScanError`
/// envelopes and surface in the return as `RunOutcome::HadScanErrors`, NOT
/// as `Err`.
pub fn run_one<R: Reader>(
    req: &ScanRequest,
    cfg: &MinerConfig,
    reader: &R,
    sink: &mut dyn FindingSink,
    cancel: Arc<AtomicBool>,
) -> Result<RunOutcome, MinerError> {
    unimplemented!(
        "Plan 03-02..06 fill run_one's body per the numbered algorithm doc above"
    )
}

//! Gap-policy dispatch: `strict` / `continuous_only` → finding-emission plan.
//!
//! Pattern analog: `gap.rs:152-187` ([`crate::gap::GapDetector`]) — stateless
//! function dispatch (no fields, no configuration) + tagged-enum policy kind
//! mirroring `GapReason` shape (`gap.rs:117-130`).
//!
//! ## Phase 3 contract (D3-08..D3-12)
//!
//! - `strict` + gaps present → one `Finding::GapAborted` (D3-11), NO `Result`.
//! - `strict` + zero gaps → fast path: scan runs, `data_slice.gap_manifest = None`.
//! - `continuous_only` + gaps → partition the requested range into
//!   maximal gap-free sub-ranges (D3-10); one `Finding::Result` per sub-range
//!   with the FULL gap manifest inlined in `data_slice.gap_manifest`.
//! - `continuous_only` + zero gaps → one `Result` with
//!   `data_slice.gap_manifest = Some(GapManifest { gaps: vec![] })` (D3-12).
//!
//! Wave 0 scaffold: signature only. Plan 03-03 fills the bodies.

#![allow(dead_code, unused_variables)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::findings::TimeRange;
use crate::gap::GapManifest;
use crate::reader::ClosedRangeUtc;

// ---------------------------------------------------------------------------
// GapPolicyKind — tagged enum, mirrors gap.rs:117-130 GapReason shape.
// ---------------------------------------------------------------------------

/// Gap-handling policy for a scan invocation (D3-19 CLI surface).
///
/// `#[serde(rename_all = "snake_case")]` so JSON wire form is
/// `"strict"` / `"continuous_only"` (matches the user-facing `--gap-policy`
/// flag spelling).
///
/// Pattern analog: `reader.rs::Side` (`Bid`/`Ask`) unit-variant enum with
/// `as_str(&self) -> &'static str`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GapPolicyKind {
    /// Reject any window touching a gap — emit one `Finding::GapAborted`,
    /// NO `Result`. Exit 0 (per D-08 — strict + gaps is NOT a preflight
    /// failure, it's a documented outcome).
    Strict,
    /// Partition the window into maximal gap-free sub-ranges — emit one
    /// `Finding::Result` per sub-range with the full manifest inlined.
    ContinuousOnly,
}

impl GapPolicyKind {
    /// Wire form, matching `--gap-policy <VALUE>`.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            GapPolicyKind::Strict => "strict",
            GapPolicyKind::ContinuousOnly => "continuous_only",
        }
    }
}

// ---------------------------------------------------------------------------
// GapDispatch — what the policy dispatcher returns to the facade.
// ---------------------------------------------------------------------------

/// Output of [`dispatch`] — drives the facade's finding-emission decision.
///
/// `Aborted(manifest)` carries the manifest the `Finding::GapAborted` envelope
/// will inline (D3-11). `SubRanges(Vec<TimeRange>)` carries the gap-free
/// partition the facade iterates over, calling `Scan::run` once per sub-range
/// (D3-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapDispatch {
    /// Strict policy + gaps present — emit one `GapAborted` and stop.
    Aborted(GapManifest),
    /// Continuous-only OR strict-with-zero-gaps — run the scan over these
    /// (possibly single-element) maximal gap-free sub-ranges.
    SubRanges(Vec<TimeRange>),
}

// ---------------------------------------------------------------------------
// dispatch — the stateless function the facade calls.
// ---------------------------------------------------------------------------

/// Compute the gap-policy dispatch for a (manifest, requested, policy) triple.
///
/// Pattern analog: `gap.rs:188-279` `GapDetector::detect` — pure function with
/// a numbered algorithm doc.
///
/// ## Algorithm (Plan 03-03 fills the body)
///
/// 1. Inspect `manifest.gaps`:
///    - Empty → return `SubRanges(vec![requested as TimeRange])` (single
///      pass-through sub-range, works for both policies' fast path).
///    - Non-empty:
///      - `policy == Strict` → return `Aborted(manifest.clone())`.
///      - `policy == ContinuousOnly` → walk the sorted manifest, compute
///        maximal gap-free sub-ranges of `requested`, return `SubRanges(v)`.
/// 2. NEVER silently swallow a gap (the
///    `never_silently_emits_on_hole_proptest` regression pins this).
///
/// Wave 0 scaffold: signature only. Plan 03-03 fills the body.
#[must_use]
pub fn dispatch(
    manifest: &GapManifest,
    requested: ClosedRangeUtc,
    policy: GapPolicyKind,
) -> GapDispatch {
    unimplemented!(
        "Plan 03-03 wires gap_policy::dispatch per the numbered algorithm doc"
    )
}

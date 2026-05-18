//! `Scan` trait + supporting types — D3-14 / Phase 3.
//!
//! Every scan is a `Send + Sync` polymorphic compute kernel registered in the
//! [`crate::scan::registry::Registry`] and dispatched by [`crate::engine::run_one`].
//! Implementations: [`crate::scan::ljung_box::LjungBoxScan`] (Phase 3 demo);
//! Phase 4 adds 21 more.
//!
//! ## Module shape
//!
//! - [`Scan`] — the polymorphic trait. Mirrors [`crate::reader::Reader`]'s
//!   `Send + Sync + &'static str id` shape (`reader.rs:198-258`). Unlike `Reader`,
//!   `Scan` does NOT carry an associated `Error` type because every scan shares the
//!   single [`ScanError`] enum (kernel / cancel / io); readers each have their own
//!   `Self::Error`.
//! - [`ScanCtx`] — brokering object the facade constructs and passes to
//!   [`Scan::run`]. Holds `cache`, `gap_detector`, `run_id`, `code_revision`,
//!   and the cancellation flag. Plan 02 fills the fields and accessor methods.
//! - [`ScanRequest`] — the typed, post-preflight, resolved request the facade
//!   hands to a scan. Plan 02 fills the fields.
//! - [`ScanError`] — `thiserror`-derived enum following the
//!   `crate::aggregator::AggregateError` shape (`aggregator.rs:201-219`).
//! - [`ScanFindingShape`] — re-exported from [`shape`] for `miner scans`
//!   catalogue introspection.
//!
//! ## Wave 0 scaffold
//!
//! Plan 03-01 lays down signature-only bodies (`unimplemented!()` / `todo!()`); Plan
//! 02..06 fill them. The trait IS object-safe — the [`tests::scan_trait_object_safe`]
//! regression gate compiles the type-erased coercion and fails the build if a future
//! method introduces non-dyn-safe self-types.

#![allow(dead_code, unused_variables)]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::findings::FindingSink;
use crate::reader::ClosedRangeUtc;

pub mod ljung_box;
pub mod registry;
pub mod shape;

pub use registry::{Registry, bootstrap};
pub use shape::ScanFindingShape;

// ---------------------------------------------------------------------------
// Scan trait — D3-14, mirrors `reader.rs:198-258` Send+Sync+&'static str shape.
// ---------------------------------------------------------------------------

/// Polymorphic scan kernel.
///
/// Implementations are registered into [`Registry`] and dispatched by
/// [`crate::engine::run_one`]. `Send + Sync` because Phase 5 will park scans in
/// a static registry shared across rayon workers.
///
/// The trait MUST stay object-safe (every method takes `&self`, no generic
/// type parameters, no `where Self: Sized`); the
/// [`tests::scan_trait_object_safe`] compile-time gate pins the invariant.
pub trait Scan: Send + Sync {
    /// Stable scan identifier — `<family>.<subfamily>.<scan_name>` per D3-17
    /// (e.g., `"stats.autocorr.ljung_box"`). Compile-time constant per scan.
    fn id(&self) -> &'static str;

    /// Major version of the scan's output shape. Bumps on any change to the
    /// emitted `effect` / `raw` keys. Phase 3 ships `version() == 1`.
    fn version(&self) -> u32;

    /// JSON Schema fragment describing the scan's `--params` shape. Used by
    /// `miner scans` introspection and by [`crate::engine::preflight`] to
    /// validate user-supplied params at the boundary.
    fn param_schema(&self) -> serde_json::Value;

    /// Declarative `effect.extra` + `raw.series` key list — consumed by
    /// `miner scans` so MCP/HTTP wrappers can render a catalogue without
    /// executing a scan.
    fn finding_fields(&self) -> ScanFindingShape;

    /// Execute the scan. The facade has already preflighted the request,
    /// emitted `RunStart`, fetched the `BarFrame`, and partitioned the gap
    /// manifest before calling this method.
    ///
    /// Implementations write findings via `sink.write_envelope(&Finding::…)`
    /// and poll [`ScanCtx::cancel`] between findings (D3-22).
    ///
    /// # Errors
    /// Returns [`ScanError`] on kernel / io / cancellation failure. The facade
    /// converts the error into a `Finding::ScanError` envelope and continues
    /// (per-finding errors are NOT preflight failures).
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
    ) -> Result<(), ScanError>;
}

// ---------------------------------------------------------------------------
// ScanCtx — brokering object the facade constructs and passes to Scan::run.
// ---------------------------------------------------------------------------

/// Per-run brokering object passed to [`Scan::run`].
///
/// Phase 3 ships the field skeleton; Plan 02 wires the accessor methods
/// (`bars(symbol, side, tf, range) -> BarFrame`, `gap_manifest(...)`, etc.).
///
/// The lifetime parameter `'a` carries references to the facade-owned cache /
/// detector borrows.
pub struct ScanCtx<'a> {
    /// Cooperative cancellation flag installed by the CLI's `ctrlc` handler.
    /// Scans poll between findings; rayon workers exit at the next yield point.
    pub cancel: Arc<AtomicBool>,
    /// Phantom lifetime carrier; Plan 02 will turn this into real `&'a BarCache` /
    /// `&'a Calendar` references.
    pub _lifetime: std::marker::PhantomData<&'a ()>,
}

// ---------------------------------------------------------------------------
// ScanRequest — the typed, post-preflight, resolved request the facade owns.
// ---------------------------------------------------------------------------

/// Resolved scan request. The facade builds one from a [`crate::config::MinerConfig`]
/// + CLI args (or MCP / HTTP request payload). Plan 02 fills the fields.
///
/// The struct intentionally carries primitive types (String / typed enums from
/// `miner-core`) rather than clap-derived types — `miner-core` does not depend
/// on clap (D-16).
pub struct ScanRequest {
    pub scan_id: String,
    pub version: u32,
    pub instrument: String,
    pub side: crate::reader::Side,
    pub timeframe: String,
    pub window: ClosedRangeUtc,
    pub gap_policy: crate::engine::gap_policy::GapPolicyKind,
    pub dry_run: bool,
    /// Resolved, post-defaults parameter object (the input to `param_hash` per D3-13).
    pub resolved_params: serde_json::Value,
}

// ---------------------------------------------------------------------------
// ScanError — thiserror enum, mirrors aggregator.rs:201-219 AggregateError.
// ---------------------------------------------------------------------------

/// Errors raised by a [`Scan::run`] implementation.
///
/// Mirrors [`crate::aggregator::AggregateError`]'s `thiserror`-derived shape —
/// no `Serialize` derive (kernel errors become `Finding::ScanError` via the
/// engine's `ScanErrorCode::as_str` mapping; serde stays at the engine
/// boundary, not the kernel boundary).
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// Computation failure inside the scan kernel (e.g., NaN propagation,
    /// invalid sample size after slicing).
    #[error("scan kernel error: {0}")]
    Kernel(String),

    /// Sink write failed during finding emission.
    #[error("sink io error: {0}")]
    Io(#[from] std::io::Error),

    /// Cooperative cancellation requested (SIGINT, D3-22).
    #[error("scan cancelled")]
    Cancelled,

    /// Underlying `MinerError` (cache lookup, framing, etc.).
    #[error(transparent)]
    Miner(#[from] crate::error::MinerError),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// Compile-time regression gate (mirrors `reader.rs:272-274`
    /// `reader_trait_object_safe`). If `Scan` becomes non-dyn-compatible the
    /// workspace stops building — that's the test.
    #[test]
    fn scan_trait_object_safe() {
        fn _accept(_s: &dyn crate::scan::Scan) {}
    }
}

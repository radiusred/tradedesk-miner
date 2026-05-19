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
//!   [`Scan::run`]. Holds `bars`, `gap_manifest`, `run_id`, `code_revision`,
//!   the cancellation flag, and the test-only `sleep_after_first_finding_ms`
//!   Pitfall 8 hook (cfg-gated under `test` or `feature = "test-internal"`).
//! - [`ScanRequest`] — the typed, post-preflight, resolved request the facade
//!   hands to a scan. Includes the canonical `dry_run: bool` signal per D3-21.
//! - [`ScanError`] — `thiserror`-derived enum following the
//!   `crate::aggregator::AggregateError` shape (`aggregator.rs:201-219`).
//! - [`ScanFindingShape`] — re-exported from [`shape`] for `miner scans`
//!   catalogue introspection.
//!
//! ## Plan 03-02 fills bodies
//!
//! Plan 03-01 laid down signature-only bodies (`unimplemented!()` / `todo!()`);
//! Plan 03-02 (this commit) fills the support-type bodies (everything except
//! `LjungBoxScan::run` + `param_schema` which Plan 04 owns). The trait IS
//! object-safe — the [`tests::scan_trait_object_safe`] regression gate compiles
//! the type-erased coercion and fails the build if a future method introduces
//! non-dyn-safe self-types.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::aggregator::{BarFrame, Timeframe};
use crate::engine::gap_policy::GapPolicyKind;
use crate::findings::{FindingSink, RunId, TimeRange};
use crate::gap::GapManifest;
use crate::reader::{Blake3Hex, ClosedRangeUtc, InstrumentSpec};

pub mod anom;
pub mod cross;
pub mod ljung_box;
pub mod primitives;
pub mod registry;
pub mod seas;
pub mod shape;

pub use registry::{Registry, bootstrap};
pub use shape::ScanFindingShape;

// ---------------------------------------------------------------------------
// ScanArity — Phase 4 (Plan 04-01 / D4-02) declarative arity for each scan.
// ---------------------------------------------------------------------------

/// Declarative arity for a scan — how many `InstrumentSpec` entries the
/// scan expects in `ScanRequest.instruments` (D4-02). Validated at preflight
/// via `PreflightCode::WrongInstrumentArity` (added in Plan 04-01 Task 3).
///
/// Pattern analog: `engine::gap_policy::GapPolicyKind` — same derives, same
/// `#[serde(rename_all = "snake_case")]`, sibling `as_str` method.
///
/// v2 extension path: add `Many(min, max)` for basket scans (Johansen etc.).
/// Adding an enum variant is schema-additive for schemars 1.x (the wire form
/// `oneOf` gains a new option without invalidating existing parsers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanArity {
    /// Single-leg scan (ANOM / SEAS family). `instruments.len() == 1`.
    Single,
    /// Two-leg scan (CROSS family). `instruments.len() == 2`.
    Pair,
    // v2: Many(min, max) — basket scans (Johansen etc.)
}

impl ScanArity {
    /// `snake_case` wire form — mirrors `GapPolicyKind::as_str` (the analog
    /// at `engine/gap_policy.rs`).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            ScanArity::Single => "single",
            ScanArity::Pair => "pair",
        }
    }

    /// Expected length of `ScanRequest.instruments` for this arity. The
    /// preflight `validate_arity` helper (Plan 04-02) rejects when
    /// `instruments.len() != self.expected_len()` with
    /// `PreflightCode::WrongInstrumentArity`.
    #[must_use]
    pub fn expected_len(&self) -> usize {
        match self {
            ScanArity::Single => 1,
            ScanArity::Pair => 2,
        }
    }
}

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

    /// Declarative arity per D4-02. ANOM / SEAS scans return
    /// [`ScanArity::Single`]; CROSS scans return [`ScanArity::Pair`]. The
    /// engine's preflight (`validate_arity`, Plan 04-02) rejects requests
    /// where `instruments.len() != self.arity().expected_len()` with
    /// `PreflightCode::WrongInstrumentArity`.
    ///
    /// No default body — every `Scan` impl must declare arity explicitly.
    /// This is intentional: the arity is a load-bearing piece of the wire
    /// contract (exposed via `miner scans` catalogue output so MCP/HTTP
    /// wrappers in Phase 6 can render a typed parameter surface) and a
    /// missing-default-causes-silent-Single-arity would be a footgun.
    fn arity(&self) -> ScanArity;

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
/// Holds borrowed references to the bar frame the facade fetched plus run-level
/// metadata (`run_id`, `code_revision`) and the cooperative cancellation flag
/// installed by the CLI wrapper's `ctrlc` handler (D3-22). The `gap_manifest`
/// field carries the partitioned manifest under `--gap-policy=continuous_only`
/// (Plan 03-03 wires the dispatch).
///
/// ## Test-only Pitfall 8 hook (`sleep_after_first_finding_ms`)
///
/// Under `#[cfg(any(test, feature = "test-internal"))]` the struct gains an
/// extra `sleep_after_first_finding_ms: Option<u64>` field. When `Some(ms)`,
/// Plan 04's `LjungBoxScan::run` performs a cancel-aware sleep loop after
/// emitting the first `Finding::Result` — this makes the Plan 06 SIGINT
/// integration test's race deterministic without flake. Production builds
/// (`cargo build` / `cargo build --release` without the `test-internal`
/// feature) do NOT activate the cfg, so the field is genuinely absent from the
/// release surface (T-03-02-05 mitigation).
pub struct ScanCtx<'a> {
    /// The bar frame the facade fetched via `BarCache::get_or_build`. Scans
    /// read columns (`open`, `high`, `low`, `close`, `volume`, `ts_open_utc`)
    /// for their kernel input.
    ///
    /// For Pair-arity scans this borrow points at leg A's frame as a
    /// default-leg anchor (matches the single-leg API surface so existing
    /// Phase 3 scan bodies don't have to special-case Pair); CROSS scans
    /// MUST access both legs via [`ScanCtx::bars_pair`] explicitly.
    pub bars: &'a BarFrame,
    /// Phase 4 (Plan 04-02 / D4-02): Two-leg borrow for `ScanArity::Pair`
    /// scans. `None` for `ScanArity::Single` scans; `Some((leg_a, leg_b))`
    /// for CROSS scans (the order matches `ScanRequest.instruments`).
    /// The leg-A reference is the same pointer as [`ScanCtx::bars`].
    pub bars_pair: Option<(&'a BarFrame, &'a BarFrame)>,
    /// The partitioned gap manifest under `--gap-policy=continuous_only` (None
    /// under `strict` success-path; the strict-with-gaps path emits
    /// `Finding::GapAborted` BEFORE reaching `Scan::run`, so the kernel never
    /// sees a strict-mode manifest).
    pub gap_manifest: Option<&'a GapManifest>,
    /// ULID identifying the run. Echoed into every `Finding::Result`'s
    /// `run_id` field so the consumer can group findings by invocation.
    pub run_id: RunId,
    /// Git SHA (or `dirty-<sha>`) of the source revision that built this
    /// binary — wired into every finding's `code_revision` field.
    pub code_revision: &'a str,
    /// Cooperative cancellation flag installed by the CLI's `ctrlc` handler.
    /// Scans poll between findings; rayon workers exit at the next yield point.
    pub cancel: Arc<AtomicBool>,
    /// **Test-only Pitfall 8 hook** (Plan 06 SIGINT race mitigation). When
    /// `Some(ms)`, `LjungBoxScan::run` pauses after the first `Finding::Result`
    /// emit for the requested duration via a cancel-aware sleep loop. NEVER
    /// reachable in release production builds — the `#[cfg(...)]` gates the
    /// field out of the struct entirely without the `test-internal` feature
    /// or `cfg(test)` activation.
    #[cfg(any(test, feature = "test-internal"))]
    pub sleep_after_first_finding_ms: Option<u64>,
}

/// Read-only view of a [`BarFrame`] truncated at or before a cutoff timestamp
/// — the look-ahead-safety enforcement surface (Plan 04-02 / RESEARCH §1.5).
///
/// Returned by [`ScanCtx::bars_up_to`]; carries borrowed slices into the
/// underlying frame's columns. Rolling / causal scans MUST consume this
/// (not raw `ctx.bars`) when emitting per-window outputs — the structural
/// guarantee is that the view contains no bar whose `ts_open_utc > cutoff`.
/// Plan 04-11 will pin this with a shuffled-future regression proptest.
pub struct BarFrameView<'a> {
    pub source_id: &'a str,
    pub symbol: &'a str,
    pub side: crate::reader::Side,
    pub tf: Timeframe,
    pub ts_open_utc: &'a [DateTime<Utc>],
    pub open: &'a [f64],
    pub high: &'a [f64],
    pub low: &'a [f64],
    pub close: &'a [f64],
    pub tick_volume: &'a [f64],
}

impl BarFrameView<'_> {
    /// Number of bars in the view (length of every column).
    #[must_use]
    pub fn len(&self) -> usize {
        self.ts_open_utc.len()
    }

    /// `true` if the view contains no bars.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ts_open_utc.is_empty()
    }
}

impl<'a> ScanCtx<'a> {
    /// Pair-leg accessor. Returns `Some((leg_a, leg_b))` for CROSS scans;
    /// `None` for ANOM / SEAS single-leg scans. Convenience over reading
    /// the public [`ScanCtx::bars_pair`] field — keeps CROSS bodies short.
    #[must_use]
    pub fn bars_pair(&self) -> Option<(&'a BarFrame, &'a BarFrame)> {
        self.bars_pair
    }

    /// Return a look-ahead-safe [`BarFrameView`] of [`ScanCtx::bars`]
    /// truncated to bars whose `ts_open_utc <= ts`. Rolling / causal scans
    /// MUST call this (not slice `ctx.bars` directly) for any per-window
    /// kernel output that consumers will index by timestamp.
    ///
    /// The cutoff is computed via `partition_point(|t| *t <= ts)` so the
    /// returned view contains every bar with `ts_open_utc <= ts` and none
    /// past it. Empty input or `ts` before the first bar returns an empty
    /// view; `ts` past the last bar returns the full frame.
    ///
    /// Threat-model note (T-04-02-04): the `partition_point` predicate is
    /// the SOLE source of truth for the cutoff — Plan 04-11's
    /// shuffled-future proptest pins the invariant by comparing per-window
    /// outputs against a frame whose post-cutoff bars are randomly
    /// permuted.
    #[must_use]
    pub fn bars_up_to(&self, ts: DateTime<Utc>) -> BarFrameView<'a> {
        let cutoff = self.bars.ts_open_utc.partition_point(|t| *t <= ts);
        BarFrameView {
            source_id: &self.bars.source_id,
            symbol: &self.bars.symbol,
            side: self.bars.side,
            tf: self.bars.tf,
            ts_open_utc: &self.bars.ts_open_utc[..cutoff],
            open: &self.bars.open[..cutoff],
            high: &self.bars.high[..cutoff],
            low: &self.bars.low[..cutoff],
            close: &self.bars.close[..cutoff],
            tick_volume: &self.bars.tick_volume[..cutoff],
        }
    }
}

// ---------------------------------------------------------------------------
// ScanRequest — the typed, post-preflight, resolved request the facade owns.
// ---------------------------------------------------------------------------

/// Resolved scan request. The facade builds one from a
/// [`crate::config::MinerConfig`] + CLI args (or MCP / HTTP request payload).
///
/// The struct intentionally carries primitive types (String / typed enums from
/// `miner-core`) rather than clap-derived types — `miner-core` does not depend
/// on clap (D-16).
///
/// ## `dry_run` is the canonical signal (D3-21 / Blocker 2)
///
/// `ScanRequest.dry_run: bool` is the CANONICAL dry-run signal end-to-end. No
/// separate `RunOutcome::DryRun` enum variant exists; the signal is in
/// `RunStart.request.dry_run` (echoed by Plan 04's framing) and in the
/// `Finding::DryRun` envelope. Plan 04's `engine::run_one` branches on
/// `req.dry_run`; Plan 05's `ScanArgs::to_scan_request` populates it; Plan
/// 03-03's gap-policy dispatch never runs when `dry_run` is true (the
/// dry-run short-circuit emits `Finding::DryRun` between `RunStart` and
/// `RunEnd` and skips the scan body entirely).
///
/// ## `sleep_after_first_finding_ms` is cfg-gated (Pitfall 8 / Blocker 3)
///
/// Under `#[cfg(any(test, feature = "test-internal"))]` the struct gains an
/// extra `sleep_after_first_finding_ms: Option<u64>` field forwarded into
/// `ScanCtx` by Plan 04's `run_one`. Production builds do NOT include the
/// field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRequest {
    /// Scan id without the version suffix — e.g. `"stats.autocorr.ljung_box"`.
    pub scan_id: String,
    /// Scan version. The pair `(scan_id, version)` keys `Registry::scans`.
    pub version: u32,
    /// Phase 4 (Plan 04-01 / D4-01) — leg-labelled instrument vector.
    /// ANOM / SEAS scans take length 1; CROSS scans take length 2. The
    /// engine's preflight `validate_arity` (Plan 04-02) rejects requests
    /// whose `instruments.len()` does not match `scan.arity().expected_len()`
    /// with `PreflightCode::WrongInstrumentArity`. Replaces the Phase 3
    /// singleton `instrument: String + side: Side` pair.
    pub instruments: Vec<InstrumentSpec>,
    /// Output timeframe — `Tf15m` / `Tf1h` / `Tf1d`.
    pub timeframe: Timeframe,
    /// Request-level OUTPUT window (D3-06): the half-open range whose bars the
    /// scan emits statistics about. The scan MAY read earlier bars internally
    /// for warm-up.
    pub window: ClosedRangeUtc,
    /// Per-finding consumed range, post gap-partitioning (Pitfall 4 pin —
    /// distinct from `window`). Under `--gap-policy=continuous_only` with gaps,
    /// Plan 04 invokes the scan once per sub-range with a different `sub_range`
    /// per invocation; under the zero-gap fast path `sub_range` equals the
    /// requested window translated through `ClosedRangeUtc::to_timerange()`.
    pub sub_range: TimeRange,
    /// Gap policy — `strict` or `continuous_only` (D3-19 default
    /// `continuous_only`).
    pub gap_policy: GapPolicyKind,
    /// Resolved, post-defaults parameter object (the input to `param_hash`
    /// per D3-13).
    pub resolved_params: serde_json::Value,
    /// Blake3 hex of `serde_json::to_vec(&resolved_params)` (D3-13). Computed
    /// once at the facade boundary and echoed into every finding's
    /// `param_hash` field for byte-stable provenance.
    pub param_hash: Blake3Hex,
    /// **Canonical dry-run signal (D3-21 / Blocker 2).** When `true`, Plan
    /// 04's `engine::run_one` emits one `Finding::DryRun` envelope between
    /// `RunStart` and `RunEnd` and skips the scan body. Echoed into
    /// `RunStart.request` so `param_hash` + `data_slice.range` provenance both
    /// encode whether this was a dry-run. There is NO `RunOutcome::DryRun`
    /// variant — the signal lives here.
    #[serde(default)]
    pub dry_run: bool,
    /// **Test-only Pitfall 8 hook (Blocker 3).** Forwarded into `ScanCtx` by
    /// Plan 04's `run_one`. Plan 05 surfaces the matching
    /// `--sleep-after-first-finding-ms` CLI flag (also cfg-gated). Plan 06's
    /// SIGINT integration test uses it to make the race deterministic.
    #[cfg(any(test, feature = "test-internal"))]
    #[serde(default)]
    pub sleep_after_first_finding_ms: Option<u64>,
}

impl ScanRequest {
    /// Construct a `ScanRequest` with the 10 universally-available fields.
    ///
    /// Plan 03-05 chains this with `with_sleep_after_first_finding_ms`
    /// (cfg-gated under `test` / `feature = "test-internal"`) to forward the
    /// CLI's `--sleep-after-first-finding-ms` hook into the request without
    /// per-field cfg in struct literals.
    ///
    /// Under release builds (no `test-internal`) the cfg-gated
    /// `sleep_after_first_finding_ms` field is absent from the struct entirely,
    /// so this constructor is the canonical entry point.
    #[allow(
        clippy::too_many_arguments,
        reason = "ScanRequest is a flat 10-field request type and this constructor is the canonical builder; introducing an intermediate builder type would obscure the call site"
    )]
    #[must_use]
    pub fn new(
        scan_id: String,
        version: u32,
        instruments: Vec<InstrumentSpec>,
        timeframe: Timeframe,
        window: ClosedRangeUtc,
        sub_range: TimeRange,
        gap_policy: GapPolicyKind,
        dry_run: bool,
        resolved_params: serde_json::Value,
        param_hash: Blake3Hex,
    ) -> Self {
        Self {
            scan_id,
            version,
            instruments,
            timeframe,
            window,
            sub_range,
            gap_policy,
            resolved_params,
            param_hash,
            dry_run,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        }
    }

    /// Chained setter for the cfg-gated `sleep_after_first_finding_ms` hook
    /// (Plan 03-02 / Pitfall 8). Forwards the value into `ScanRequest`; the
    /// method itself is `#[cfg]`-gated so the call site in Plan 03-05's
    /// `ScanArgs::to_scan_request` must also be cfg-gated when invoking it.
    ///
    /// NEVER reachable in release production builds — the cfg gate matches the
    /// matching field gate (`#[cfg(any(test, feature = "test-internal"))]`).
    #[cfg(any(test, feature = "test-internal"))]
    #[must_use]
    pub fn with_sleep_after_first_finding_ms(mut self, value: Option<u64>) -> Self {
        self.sleep_after_first_finding_ms = value;
        self
    }
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
// Cfg-gated parameter-passing dependencies declared above for the typed
// ScanRequest fields; the JsonSchema derive deliberately stays OFF (ScanRequest
// is the engine-internal request type — the wire-form description lives on the
// per-scan `param_schema()` JSON Schema fragment + the framing `request` blob
// inside `RunStart`).
// ---------------------------------------------------------------------------

#[allow(dead_code)]
const _: fn() = || {
    // Compile-only: confirm `DateTime<Utc>` + `JsonSchema` are usable from this
    // module (`schemars` is a direct dependency). Plan 04's param_schema()
    // implementations will lean on this.
    fn _accept_dt(_v: DateTime<Utc>) {}
    fn _accept_js<T: JsonSchema>() {}
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::Side;

    /// Compile-time regression gate (mirrors `reader.rs:272-274`
    /// `reader_trait_object_safe`). If `Scan` becomes non-dyn-compatible the
    /// workspace stops building — that's the test.
    #[test]
    fn scan_trait_object_safe() {
        fn _accept(_s: &dyn crate::scan::Scan) {}
    }

    /// Plan 04-01 Task 2 — Behavior Test 1: `scan_arity_serialises_snake_case`.
    /// `ScanArity::Single` serialises as `"single"`; `ScanArity::Pair` as
    /// `"pair"`. Round-trip deser equality holds for both. The `as_str()`
    /// helper mirrors `GapPolicyKind::as_str` (D4-02 / Pattern F).
    #[test]
    fn scan_arity_serialises_snake_case() {
        let cases = [(ScanArity::Single, "single"), (ScanArity::Pair, "pair")];
        for (arity, expected) in cases {
            let json = serde_json::to_string(&arity).expect("serialise");
            assert_eq!(json, format!("\"{expected}\""), "wire form mismatch");
            let parsed: ScanArity = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(parsed, arity);
            assert_eq!(arity.as_str(), expected);
        }
        // expected_len() helper used by validate_arity in Plan 04-02.
        assert_eq!(ScanArity::Single.expected_len(), 1);
        assert_eq!(ScanArity::Pair.expected_len(), 2);
    }

    /// Plan 04-01 Task 2 — Behavior Test 3:
    /// `scan_request_instruments_len_one_serialises`. ScanRequest with
    /// `instruments = vec![InstrumentSpec { symbol: "EURUSD", side: Side::Bid }]`
    /// serialises to JSON where `instruments` is a JSON array of length 1
    /// (D4-01).
    #[test]
    fn scan_request_instruments_len_one_serialises() {
        let req = sample_scan_request(false);
        assert_eq!(req.instruments.len(), 1, "Single-leg request");
        let value = serde_json::to_value(&req).expect("serialise");
        let arr = value
            .get("instruments")
            .and_then(|v| v.as_array())
            .expect("instruments must be a JSON array");
        assert_eq!(arr.len(), 1, "instruments JSON array must have length 1");
        let leg = arr[0].as_object().expect("leg must be an object");
        assert_eq!(leg["symbol"], "EURUSD");
        assert_eq!(leg["side"], "bid");
        // Round-trip via a string (the Blake3Hex deserializer requires a
        // borrowed str, which `serde_json::from_value` cannot supply via its
        // owned Value path — use from_str on the serialised JSON instead).
        let json = serde_json::to_string(&req).expect("serialise to string");
        let parsed: ScanRequest = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(parsed.instruments.len(), 1);
        assert_eq!(parsed.instruments[0].symbol, "EURUSD");
        assert!(matches!(parsed.instruments[0].side, Side::Bid));
    }

    /// Plan 03-02 Task 2 — `scan_request_dry_run_defaults_false_when_absent`.
    ///
    /// JSON object WITHOUT a `dry_run` field deserialises into a `ScanRequest`
    /// with `dry_run == false` (per `#[serde(default)]` on the field). Pins
    /// Blocker 2 acceptance: the dry-run signal is opt-in, not opt-out, and the
    /// MCP / HTTP wrappers in Phase 6 can omit the field without changing
    /// behaviour.
    #[test]
    fn scan_request_dry_run_defaults_false_when_absent() {
        use crate::aggregator::Timeframe;
        use crate::engine::gap_policy::GapPolicyKind;
        use crate::reader::Side;
        let json = serde_json::json!({
            "scan_id": "stats.autocorr.ljung_box",
            "version": 1,
            "instruments": [{"symbol": "EURUSD", "side": "bid"}],
            "timeframe": "15m",
            "window": {
                "start_utc": "2026-01-01T00:00:00Z",
                "end_utc": "2026-02-01T00:00:00Z"
            },
            "sub_range": {
                "start_utc": "2026-01-01T00:00:00Z",
                "end_utc": "2026-02-01T00:00:00Z"
            },
            "gap_policy": "continuous_only",
            "resolved_params": {"lags": 20},
            "param_hash": "0000000000000000000000000000000000000000000000000000000000000000",
            // dry_run intentionally omitted — must default to false.
        });
        let s = serde_json::to_string(&json).expect("serialise");
        let req: ScanRequest = serde_json::from_str(&s).expect("deserialise");
        assert!(
            !req.dry_run,
            "dry_run must default to false when absent (Blocker 2)"
        );
        // Spot-check D4-01 instruments Vec + the rest of the struct.
        assert_eq!(req.instruments.len(), 1);
        assert_eq!(req.instruments[0].symbol, "EURUSD");
        assert!(matches!(req.instruments[0].side, Side::Bid));
        assert!(matches!(req.timeframe, Timeframe::Tf15m));
        assert!(matches!(req.gap_policy, GapPolicyKind::ContinuousOnly));
    }

    /// Plan 03-02 Task 2 — `scan_request_dry_run_round_trips`.
    ///
    /// A `ScanRequest` constructed with `dry_run = true` round-trips through
    /// `serde_json::to_string` → `from_str` with the flag preserved. Pins the
    /// wire-form symmetry the MCP / HTTP wrappers will rely on.
    #[test]
    fn scan_request_dry_run_round_trips() {
        let req = sample_scan_request(true);
        let json = serde_json::to_string(&req).expect("serialise");
        let parsed: ScanRequest = serde_json::from_str(&json).expect("deserialise");
        assert!(
            parsed.dry_run,
            "dry_run flag must survive serde round-trip; serialised: {json}"
        );
        // Same again for false — make sure `#[serde(default)]` doesn't drop the
        // explicit value on the wire.
        let req_false = sample_scan_request(false);
        let json_false = serde_json::to_string(&req_false).expect("serialise");
        let parsed_false: ScanRequest = serde_json::from_str(&json_false).expect("deserialise");
        assert!(!parsed_false.dry_run);
    }

    /// Plan 03-02 Task 2 — `scan_ctx_has_sleep_after_first_finding_ms_field`
    /// (Blocker 3 / Pitfall 8).
    ///
    /// Under `cargo test` the `cfg(test)` predicate activates the cfg-gated
    /// `sleep_after_first_finding_ms: Option<u64>` field on both `ScanCtx`
    /// and `ScanRequest`. This test constructs both with the field present
    /// (compile-time evidence the field is reachable under the test cfg) and
    /// asserts the default is `None`. A release `cargo build` (without
    /// `--features test-internal`) does NOT compile this test module and the
    /// field is absent from the production binary.
    #[test]
    #[cfg(any(test, feature = "test-internal"))]
    fn scan_ctx_has_sleep_after_first_finding_ms_field() {
        let req = sample_scan_request(false);
        assert!(
            req.sleep_after_first_finding_ms.is_none(),
            "sleep_after_first_finding_ms must default to None on ScanRequest"
        );
        // Compile-time evidence the field is reachable on ScanCtx too — the
        // struct literal explicitly mentions the cfg-gated field.
        let bars = sample_bar_frame();
        let cancel = Arc::new(AtomicBool::new(false));
        let ctx = ScanCtx {
            bars: &bars,
            bars_pair: None,
            gap_manifest: None,
            run_id: RunId::new(),
            code_revision: "abc123",
            cancel,
            sleep_after_first_finding_ms: None,
        };
        assert!(ctx.sleep_after_first_finding_ms.is_none());
    }

    /// Plan 04-02 Task 2 — Behavior Test 7:
    /// `scan_ctx_bars_up_to_partitions_at_cutoff`. Given a BarFrame with
    /// `ts_open_utc = [t0, t1, t2, t3]`, `ctx.bars_up_to(t1)` returns a
    /// `BarFrameView` whose `ts_open_utc.len() == 2` (only `[t0, t1]`).
    /// `ctx.bars_up_to(t0 - 1m)` returns len 0; `ctx.bars_up_to(t3 + 1d)`
    /// returns len 4 (no entries past t3 — partition_point's "<=" predicate
    /// gives an inclusive upper bound).
    #[test]
    fn scan_ctx_bars_up_to_partitions_at_cutoff() {
        use chrono::{Duration, TimeZone};
        use crate::aggregator::Timeframe;
        use crate::reader::Side;
        let t0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let ts = vec![
            t0,
            t0 + Duration::minutes(15),
            t0 + Duration::minutes(30),
            t0 + Duration::minutes(45),
        ];
        let bars = BarFrame {
            source_id: "test".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: ts.clone(),
            ts_close_utc: ts.iter().map(|t| *t + Duration::minutes(15)).collect(),
            open: vec![1.0; 4],
            high: vec![1.1; 4],
            low: vec![0.9; 4],
            close: vec![1.0; 4],
            tick_volume: vec![1.0; 4],
        };
        let ctx = ScanCtx {
            bars: &bars,
            bars_pair: None,
            gap_manifest: None,
            run_id: RunId::new(),
            code_revision: "test",
            cancel: Arc::new(AtomicBool::new(false)),
            sleep_after_first_finding_ms: None,
        };
        // Cutoff at t[1] -> view length 2 ([t0, t1] inclusive).
        let view = ctx.bars_up_to(ts[1]);
        assert_eq!(view.len(), 2, "bars_up_to(t[1]) must include t0 and t1");
        assert_eq!(view.ts_open_utc.len(), 2);
        assert_eq!(view.close.len(), 2);
        // Cutoff before t[0] -> empty view.
        let empty = ctx.bars_up_to(t0 - Duration::minutes(1));
        assert_eq!(empty.len(), 0, "cutoff before first bar -> empty");
        // Cutoff well past t[3] -> full view.
        let full = ctx.bars_up_to(ts[3] + Duration::days(1));
        assert_eq!(full.len(), 4, "cutoff past last bar -> full view");
        // Cutoff exactly equal to last bar's ts_open -> includes it.
        let inclusive = ctx.bars_up_to(ts[3]);
        assert_eq!(
            inclusive.len(),
            4,
            "cutoff at last bar's ts_open_utc includes it (partition_point <=)"
        );
    }

    fn sample_scan_request(dry_run: bool) -> ScanRequest {
        use chrono::TimeZone;
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
        ScanRequest {
            scan_id: "stats.autocorr.ljung_box".into(),
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
            resolved_params: serde_json::json!({"lags": 20}),
            param_hash: blake3_hex_zero(),
            dry_run,
            #[cfg(any(test, feature = "test-internal"))]
            sleep_after_first_finding_ms: None,
        }
    }

    fn blake3_hex_zero() -> Blake3Hex {
        // 64-char lowercase-hex zero blake3 — deterministic placeholder for tests
        // that don't care about the actual hash value. `Blake3Hex::from_hex_bytes`
        // takes a fixed-size 64-byte ASCII array; we materialise one from a 64-char
        // hex string here.
        let bytes: [u8; 64] = [b'0'; 64];
        Blake3Hex::from_hex_bytes(&bytes)
    }

    fn sample_bar_frame() -> BarFrame {
        // Minimal empty BarFrame for ctx-construction tests. `BarFrame` is a pub
        // struct (no `new` constructor — Plan 02-02), so we build it via struct
        // literal with empty column vectors. The scan kernel never reads from
        // this frame in the ctx-shape unit tests above.
        BarFrame {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: Side::Bid,
            tf: Timeframe::Tf15m,
            ts_open_utc: Vec::new(),
            ts_close_utc: Vec::new(),
            open: Vec::new(),
            high: Vec::new(),
            low: Vec::new(),
            close: Vec::new(),
            tick_volume: Vec::new(),
        }
    }
}

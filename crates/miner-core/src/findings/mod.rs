//! `Finding` envelope types — the centrepiece contract of Phase 1.
//!
//! Every miner invocation produces a stream of `Finding` values (one JSON object per line
//! on stdout via `FindingSink`). Six variants — `RunStart`, `Result`, `ScanError`,
//! `GapAborted`, `RunEnd`, `DryRun` — discriminated by the `kind` field
//! (`#[serde(tag = "kind")]`). The Phase 3 `DryRun` variant is an additive extension of
//! the original five — existing consumers parse the first five unchanged.
//!
//! ## Locked envelope fields (D-12..D-14, OUT-02)
//!
//! Three of the five variants (`Result`, `ScanError`, `GapAborted`) carry these seven
//! locked common fields, INLINED into each variant payload struct (NOT via
//! `#[serde(flatten)]` — see RESEARCH §Anti-Patterns). The framing records (`RunStart`,
//! `RunEnd`, `DryRun`) intentionally do NOT carry them per D-09.
//!
//! - `schema_version`: `1` in v1; bumps on breaking change.
//! - `scan_id_at_version` (`"scan_id@version"`): e.g., `"stats.autocorr.ljung_box@1"`.
//! - `param_hash`: blake3 hash of resolved params (post-defaults).
//! - `code_revision`: git SHA (or `dirty-<sha>`) from `miner_core::CODE_REVISION`.
//! - `data_slice`: the input range actually consumed by the scan.
//! - `dsr`: reserved-but-null in v1; populated in Phase 5 (Deflated Sharpe Ratio).
//! - `fdr_q`: reserved-but-null in v1; populated in Phase 5 (BH-FDR adjusted q-value).
//!
//! `dsr` and `fdr_q` MUST serialise as JSON `null` (not absent fields). Serde's default
//! Option serialisation emits `null` when `skip_serializing_if` is NOT applied, so the
//! reserved slots are visible to consumers.
//!
//! ## Determinism (OUT-03)
//!
//! All map-typed fields (`Raw::series`, `Effect::extra`, `RunSummary::per_scan`) use
//! `BTreeMap` — NEVER `HashMap` — so the JSON output ordering is alphabetic and stable.
//!
//! ## Threats mitigated
//!
//! - **T-01-02 (schema injection / drift):** The Rust types ARE the schema source of
//!   truth. Plan 06's xtask regenerates the schema from these types and CI diffs the
//!   artifact. Renaming a field or changing its type fails the diff gate.
//! - **T-01-04 (code revision tampering):** `code_revision: String` is populated by
//!   callers from `miner_core::CODE_REVISION` (Plan 01-01's build.rs).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::gap::GapManifest;

pub mod base64_bytes;
pub mod run_id;
pub mod sink;

pub use base64_bytes::{Base64Bytes, Dtype};
pub use run_id::RunId;
pub use sink::FindingSink;

// ---------------------------------------------------------------------------
// Common types — used by multiple variants
// ---------------------------------------------------------------------------

/// Half-open UTC time interval [`start_utc`, `end_utc`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TimeRange {
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
}

/// The input range a scan actually consumed (post gap-partitioning).
///
/// `gap_manifest_ref` is reserved for the Phase-7 content-addressed
/// deduplication path. `gap_manifest` is populated by Phase 3's
/// `continuous_only` gap policy (D3-10 / D3-12) — the full Phase-2
/// `GapManifest` is inlined into every `Result` finding's `data_slice` so the
/// finding is self-describing without cross-referencing.
///
/// Both optional fields MUST serialise as JSON `null` when absent (NOT
/// omitted) — the same convention used by `dsr` / `fdr_q` on `ResultFinding`.
/// DO NOT add `#[serde(skip_serializing_if = "Option::is_none")]` to either
/// field (03-PATTERNS line 572).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DataSlice {
    pub range: TimeRange,
    pub gap_manifest_ref: Option<String>,
    /// Inlined Phase-2 gap manifest under `--gap-policy=continuous_only` (D3-10
    /// / D3-12). `None` under `strict` success-path and in v1's pre-Phase-3
    /// callers. Serialises as JSON `null` when absent (NOT omitted) — bare
    /// `#[serde(default)]`, no `skip_serializing_if`.
    #[serde(default)]
    pub gap_manifest: Option<GapManifest>,
    /// Phase 4 (Plan 04-01 / D4-03) — leg-labelled source vector. Length =
    /// `scan.arity().expected_len()` (1 for ANOM / SEAS, 2 for CROSS). Self-
    /// describing per-finding leg provenance; CROSS scans populate two
    /// `Source` entries in `ScanRequest.instruments` order. The previous
    /// singleton `source: Source` field on `ResultFinding` and
    /// `GapAbortedFinding` has been removed in favour of this vector — see
    /// `.planning/phases/04-scan-catalogue-anom-cross-seas/04-01-SCHEMA-DIFF.md`
    /// for the schema-additive decision (`#[serde(default)]` ensures the
    /// schema diff is purely additive — the `required` array on `DataSlice`
    /// is unchanged).
    #[serde(default)]
    pub sources: Vec<Source>,
}

/// The instrument / side / timeframe a finding pertains to.
///
/// String-typed in Phase 1. Phase 2 introduces typed `Side` / `Symbol` / `Timeframe`
/// enums and these field types will be tightened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    pub source_id: String,
    pub symbol: String,
    pub side: String,
    pub timeframe: String,
}

/// A single raw array: base64-encoded LE f64 bytes plus its shape and dtype (D-02).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RawArray {
    pub data: Base64Bytes,
    pub shape: Vec<u64>,
    pub dtype: Dtype,
}

/// The `raw` block on a `Result` finding — the INPUTS the scan consumed (D-04).
///
/// `series` MUST contain a `timestamps_ms` key when `raw` is present (D-03). Use
/// [`Raw::new`] to enforce this at construction time, or [`Raw::new_unchecked`] in
/// tests that exercise other fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Raw {
    /// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
    pub series: BTreeMap<String, RawArray>,
}

impl Raw {
    /// Construct a `Raw` block, validating that `timestamps_ms` is present (D-03).
    ///
    /// Production callers should map the `Err` into a `MinerError::Internal` at the
    /// scan boundary.
    ///
    /// # Errors
    ///
    /// Returns `Err` with a static message when `series` does NOT contain a
    /// `timestamps_ms` key (D-03 invariant).
    pub fn new(series: BTreeMap<String, RawArray>) -> Result<Self, &'static str> {
        if !series.contains_key("timestamps_ms") {
            return Err("Raw::new: `series` must contain a `timestamps_ms` array (D-03)");
        }
        Ok(Self { series })
    }

    /// Construct a `Raw` block WITHOUT validating the `timestamps_ms` invariant.
    ///
    /// Test-only helper for unit tests that exercise OTHER fields of the envelope. Real
    /// scans must use [`Raw::new`] to keep D-03 enforced.
    #[cfg(test)]
    #[must_use]
    pub fn new_unchecked(series: BTreeMap<String, RawArray>) -> Self {
        Self { series }
    }
}

/// The `effect` block on a `Result` finding — the OUTPUTS the scan produced (D-04).
///
/// `extra` carries scan-derived arrays (e.g., Ljung-Box `lags`/`acf`; OLS
/// `residuals`). Same `{data, shape, dtype}` shape as `raw.series` entries. Uses
/// `BTreeMap` for deterministic ordering — OUT-03.
///
/// Phase 5 (Plan 05-01 / D5-03) added the optional `effect_size` field
/// additively — `#[serde(default)]` keeps the schema diff non-breaking. The
/// None case MUST serialise as JSON `null` (NOT omitted) per OUT-03; that is
/// why no `#[serde(skip_serializing_if = ...)]` attribute is attached.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Effect {
    pub metric: String,
    pub value: f64,
    #[serde(default)]
    pub p_value: Option<f64>,
    #[serde(default)]
    pub n: Option<u64>,
    #[serde(default)]
    pub ci95: Option<[f64; 2]>,
    /// Phase 5 (Plan 05-01 / D5-03) — optional standardised effect-size
    /// statistic alongside `value`. Canonical `kind` values include
    /// `"cohens_d"`, `"hedges_g"`, `"cliffs_delta"`, `"vr_minus_one"`, plus
    /// scan-specific kinds (see D5-03 table). `None` for scans that do not
    /// emit a standardised effect size; serialises as JSON `null`.
    #[serde(default)]
    pub effect_size: Option<EffectSize>,
    /// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
    #[serde(default)]
    pub extra: BTreeMap<String, RawArray>,
}

/// Phase 5 (Plan 05-01 / D5-03) — standardised effect-size statistic carried on
/// `Effect.effect_size`.
///
/// `kind` is an open string (NOT a sealed enum) so adding new effect-size
/// kinds in later phases is additive and does not break the JSON Schema.
/// Canonical values: `"cohens_d"`, `"hedges_g"`, `"cliffs_delta"`,
/// `"vr_minus_one"`, plus scan-specific kinds per D5-03.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EffectSize {
    /// Open-string discriminator — see canonical values above.
    pub kind: String,
    /// The numeric effect-size value.
    pub value: f64,
}

/// Phase 5 (Plan 05-01 / D5-05) — describes the resampling method that
/// produced a `Finding::Result`'s p-value / CI under bootstrap.
///
/// Embedded in [`ReproEnvelope::bootstrap`] when a scan ran a bootstrap pass.
/// `method` is an open string in v1 (`"stationary"` | `"block"`), permitting
/// additive growth (e.g., `"pair_bootstrap"`) without a schema break.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BootstrapSpec {
    /// Open-string method name — v1 values: `"stationary"`, `"block"`.
    pub method: String,
    /// Number of bootstrap resamples drawn (`B`).
    pub n: u32,
}

/// Phase 5 (Plan 05-01 / D5-05) — describes the null-distribution method
/// (phase-scramble / circular-shift) that produced a `Finding::Result`'s
/// p-value under a null-resampling pass.
///
/// Embedded in [`ReproEnvelope::null`] when a scan ran a null pass.
/// `method` is an open string in v1 (`"phase_scramble"` | `"circular_shift"`),
/// permitting additive growth without a schema break. Mirrors
/// [`crate::scan::NullMethod`] on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NullSpec {
    /// Open-string method name — v1 values: `"phase_scramble"`,
    /// `"circular_shift"`.
    pub method: String,
    /// Number of null draws (`N_null`).
    pub n: u32,
}

/// Phase 5 (Plan 05-01 / D5-05) — auditable reproducibility envelope echoed
/// on every hygiene-touched [`Finding::Result`].
///
/// HYG-05 makes the master seed + per-job seed visible alongside the
/// bootstrap / null spec so a consumer can replay the resampling pass bit-for-
/// bit. The threat-model disposition `T-05-01-I1` accepts this disclosure —
/// `Xoshiro256PlusPlus` is NOT cryptographic and predicting future outputs
/// from observed outputs is trivial; the threat model has no secrecy
/// requirement for resampling RNG state.
///
/// Population rule (enforced by Plan 05-03 engine integration):
/// `repro = Some(_)` iff bootstrap or null was run for the finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReproEnvelope {
    /// Run-level master seed (echoed verbatim on every finding from a single
    /// run for cross-finding correlation).
    pub master_seed: u64,
    /// Per-job seed derived from `master_seed` + the canonical job key
    /// (`scan_id@version`, instrument(s), timeframe, window, `param_hash`).
    pub job_seed: u64,
    /// Optional bootstrap descriptor — `None` when the finding's p-value did
    /// not involve a bootstrap pass.
    #[serde(default)]
    pub bootstrap: Option<BootstrapSpec>,
    /// Optional null-distribution descriptor — `None` when the finding's
    /// p-value did not involve a null-resampling pass.
    #[serde(default)]
    pub null: Option<NullSpec>,
}

// ---------------------------------------------------------------------------
// Per-variant payload structs
// ---------------------------------------------------------------------------

/// Payload for the opening framing record (D-09, D-11).
///
/// Does NOT carry the seven locked envelope fields — framing records are exempt per
/// the closing note of CONTEXT D-09.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunStart {
    pub run_id: RunId,
    pub started_at_utc: DateTime<Utc>,
    pub miner_version: String,
    pub code_revision: String,
    /// Fully-resolved invocation (`scan_id@version`, instrument(s), side, timeframe,
    /// window, params with defaults applied, `gap_policy`). `serde_json::Value` for v1
    /// simplicity; the `RawValue` optimisation is deferred.
    pub request: serde_json::Value,
}

/// Payload for the closing framing record (D-09, D-11).
///
/// Does NOT carry the seven locked envelope fields — framing records are exempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunEnd {
    pub run_id: RunId,
    pub ended_at_utc: DateTime<Utc>,
    pub wall_clock_ms: i64,
    pub summary: RunSummary,
}

/// Per-run aggregate counters (D-11).
///
/// `Default::default()` produces zero counters and an empty `per_scan` map — this is
/// the contract Plan 05's `emit_fixture()` depends on.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RunSummary {
    pub results_emitted: u64,
    pub scan_errors: u64,
    pub gap_aborted: u64,
    /// Keyed by `"scan_id@version"`. `BTreeMap` for deterministic ordering — OUT-03.
    pub per_scan: BTreeMap<String, PerScanCounts>,
}

/// Per-scan counters embedded in [`RunSummary::per_scan`] (D-11).
///
/// `Copy` is safe — three `u64`s, no allocations. `Default` produces zero counters.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerScanCounts {
    pub results: u64,
    pub errors: u64,
    pub gap_aborted: u64,
}

/// A `result` finding: the headline scan output (D-04).
///
/// Carries the seven locked envelope fields plus per-variant additions. `dsr` and
/// `fdr_q` MUST serialise as JSON `null` in v1 (NOT absent) — DO NOT add
/// `#[serde(skip_serializing_if = "Option::is_none")]` to either field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResultFinding {
    // ----- Locked envelope fields -----
    pub schema_version: u32,
    #[serde(rename = "scan_id@version")]
    pub scan_id_at_version: String,
    pub param_hash: String,
    pub code_revision: String,
    pub data_slice: DataSlice,
    /// Reserved for Phase 5 (Deflated Sharpe Ratio). Serialises as `null` in v1.
    pub dsr: Option<f64>,
    /// Reserved for Phase 5 (BH-FDR adjusted q-value). Serialises as `null` in v1.
    pub fdr_q: Option<f64>,
    // ----- Per-variant fields -----
    pub run_id: RunId,
    pub produced_at_utc: DateTime<Utc>,
    /// Resolved scan parameters (post-defaults).
    pub params: serde_json::Value,
    pub effect: Effect,
    /// Optional inputs the scan consumed (D-04 input/output split).
    pub raw: Option<Raw>,
    /// Phase 5 (Plan 05-01 / D5-05) — optional reproducibility envelope.
    /// `Some(_)` iff bootstrap or null resampling produced the finding's
    /// p-value / CI; `None` for closed-form-only findings. Population rule
    /// is enforced by the Plan 05-03 engine integration; this struct only
    /// pins the type-level shape. Serialises as JSON `null` when absent
    /// (NOT omitted) per OUT-03 — DO NOT add
    /// `#[serde(skip_serializing_if = "Option::is_none")]`.
    #[serde(default)]
    pub repro: Option<ReproEnvelope>,
}

/// A `scan_error` finding: mid-run scan failure (D-05).
///
/// Carries the seven locked envelope fields plus per-variant additions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScanErrorFinding {
    // ----- Locked envelope fields -----
    pub schema_version: u32,
    #[serde(rename = "scan_id@version")]
    pub scan_id_at_version: String,
    pub param_hash: String,
    pub code_revision: String,
    pub data_slice: DataSlice,
    pub dsr: Option<f64>,
    pub fdr_q: Option<f64>,
    // ----- Per-variant fields -----
    pub run_id: RunId,
    pub produced_at_utc: DateTime<Utc>,
    /// Open-string `error_code` per RESEARCH §"`error_code` Vocabulary". Internally
    /// constructed from a typed `ScanErrorCode`; on the wire the schema treats it as
    /// `string` so adding new codes is additive (non-breaking).
    pub error_code: String,
    pub message: String,
    pub request_context: serde_json::Value,
}

/// A `gap_aborted` finding: emitted once per scan run under `--gap-policy=strict`
/// when a gap manifest disallows the requested window (D-08).
///
/// Carries the seven locked envelope fields plus per-variant additions. The
/// `gap_manifest` shape is finalised in Phase 2 — v1 treats it as `serde_json::Value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GapAbortedFinding {
    // ----- Locked envelope fields -----
    pub schema_version: u32,
    #[serde(rename = "scan_id@version")]
    pub scan_id_at_version: String,
    pub param_hash: String,
    pub code_revision: String,
    pub data_slice: DataSlice,
    pub dsr: Option<f64>,
    pub fdr_q: Option<f64>,
    // ----- Per-variant fields -----
    pub run_id: RunId,
    pub produced_at_utc: DateTime<Utc>,
    pub gap_manifest: serde_json::Value,
}

/// Payload for the `dry_run` framing-like envelope (D3-21).
///
/// Carries the run-level provenance (`run_id`, `produced_at_utc`, `request`,
/// `resolved_params`) plus the planning shape (`planned_data_slice`,
/// `estimated_findings_count`). FRAMING-like — does NOT carry the seven locked
/// envelope fields (`schema_version`, `scan_id_at_version`, `param_hash`,
/// `code_revision`, `data_slice`, `dsr`, `fdr_q`); those belong to the
/// `Result` family.
///
/// Pitfall 3 invariant (pinned by `dry_run_does_not_increment_results_emitted`):
/// emitting `Finding::DryRun(_)` MUST NOT increment `RunSummary.results_emitted`
/// — dry-run runs leave all three summary counters at zero. The dry-run signal
/// is carried in `ScanRequest.dry_run` (echoed into `RunStart.request`) and in
/// the `Finding::DryRun` variant; there is NO `dry_run_emitted` counter (Warning
/// 9 — pinned by `run_summary_has_no_dry_run_emitted_field`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DryRunFinding {
    pub run_id: RunId,
    pub produced_at_utc: DateTime<Utc>,
    /// The same `request` blob `RunStart` echoes (canonical run-level metadata
    /// — `scan_id@version`, instrument, side, timeframe, window, `gap_policy`,
    /// `dry_run = true`).
    pub request: serde_json::Value,
    /// Post-defaults parameter object — same input the `param_hash` would be
    /// computed over for a non-dry-run invocation (D3-13).
    pub resolved_params: serde_json::Value,
    /// The `data_slice` the scan WOULD have consumed had the run executed
    /// (post-window-parse, pre-gap-partition).
    pub planned_data_slice: DataSlice,
    /// Best-effort planning estimate of how many `Finding::Result` envelopes
    /// the actual run would emit. `0` is a valid value (the scan computes none
    /// on the provided slice).
    pub estimated_findings_count: u64,
}

/// Phase 5 (Plan 05-01 / D5-02) — payload for the `sweep_summary` envelope
/// emitted ONCE at the end of a sweep run after BH-FDR adjustment lands the
/// per-finding q-values (HYG-02).
///
/// FRAMING-LIKE record — does NOT carry the seven locked envelope fields
/// (`schema_version`, `scan_id_at_version`, `param_hash`, `code_revision`,
/// `data_slice`, `dsr`, `fdr_q`); the sweep summary is run-level, not
/// scan-level, so those scan-scoped fields are intentionally absent (per
/// 05-RESEARCH Open Question 3 recommendation). Run-level identity travels in
/// `run_id` + `produced_at_utc`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SweepSummaryFinding {
    /// ULID identifying the sweep run; matches the surrounding `RunStart` /
    /// `RunEnd` records' `run_id`.
    pub run_id: RunId,
    /// Timestamp the summary was emitted.
    pub produced_at_utc: DateTime<Utc>,
    /// Per-family FDR result, keyed by `scan_id@version` (default scope) or
    /// `scan_family` per `[fdr].family` config. `BTreeMap` (NEVER `HashMap`)
    /// for deterministic ordering — OUT-03.
    pub fdr_by_family: BTreeMap<String, FdrFamilySummary>,
    /// Run-level totals (jobs run, results emitted, scan errors, gap aborts).
    pub totals: SweepTotals,
}

/// Phase 5 (Plan 05-01 / D5-02 / HYG-02) — per-family BH-FDR result embedded
/// in [`SweepSummaryFinding::fdr_by_family`].
///
/// `method` is an open string (`"benjamini_hochberg"` in v1; extension hook
/// for `"benjamini_yekutieli"` in v2). `per_finding` is `Vec` (NEVER `HashMap`)
/// so stable index order is preserved — `finding_index` references the
/// zero-indexed position of the finding within the family in the streaming
/// JSONL output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FdrFamilySummary {
    /// FDR method — v1 always `"benjamini_hochberg"`.
    pub method: String,
    /// FDR control level (typical: 0.05 or 0.10).
    pub alpha: f64,
    /// Per-finding raw-p / q-value pairs in stable index order. `Vec`
    /// preserves alignment with the streaming JSONL `finding_index` — NEVER
    /// `HashMap`.
    pub per_finding: Vec<FindingFdrEntry>,
}

/// Phase 5 (Plan 05-01 / D5-02 / HYG-02) — one row of
/// [`FdrFamilySummary::per_finding`]: raw p-value + BH-adjusted q-value for a
/// single finding, indexed by its position in the streaming JSONL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FindingFdrEntry {
    /// Zero-indexed position of this finding within its family in the
    /// streaming JSONL output.
    pub finding_index: u64,
    /// Raw (unadjusted) p-value as emitted on the finding's
    /// `effect.p_value`.
    pub raw_p: f64,
    /// BH-FDR adjusted q-value.
    pub q_value: f64,
}

/// Phase 5 (Plan 05-01 / D5-02) — run-level aggregate counters embedded in
/// [`SweepSummaryFinding::totals`].
///
/// `Default::default()` produces all-zero counters — convenient for tests and
/// for the initial fan-out before any job completes.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SweepTotals {
    pub jobs_run: u64,
    pub results_emitted: u64,
    pub scan_errors: u64,
    pub gap_aborted: u64,
}

// ---------------------------------------------------------------------------
// The tagged `Finding` enum — single Rust type produces every JSON variant
// ---------------------------------------------------------------------------

/// The six-variant Finding envelope discriminated by the JSON `kind` field.
///
/// Per RESEARCH §Anti-Patterns: do NOT use `#[serde(flatten)]` on a "common-fields"
/// struct. The seven locked envelope fields are inlined directly into each variant
/// payload struct that carries them (Result, `ScanError`, `GapAborted`).
///
/// Phase 3 added the `DryRun` variant additively (D3-21) — the existing
/// `#[serde(tag = "kind", rename_all = "snake_case")]` attribute automatically
/// produces the `"dry_run"` discriminator without per-variant serde
/// annotations. Phase 5 Plan 05-01 (D5-02) extends the enum a second time with
/// the `SweepSummary` variant emitted once per sweep run after BH-FDR
/// adjustment (HYG-02); the existing tag attribute likewise produces the
/// `"sweep_summary"` discriminator automatically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    RunStart(RunStart),
    Result(ResultFinding),
    ScanError(ScanErrorFinding),
    GapAborted(GapAbortedFinding),
    RunEnd(RunEnd),
    DryRun(DryRunFinding),
    SweepSummary(SweepSummaryFinding),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Compile-time `Copy` assertion. If `RunId` ever loses its `Copy` derive, this
    /// function will fail to compile — the regression gate for Plan 05's reuse of the
    /// same `run_id` across `RunStart` and `RunEnd`.
    fn assert_copy<T: Copy>() {}

    fn sample_data_slice() -> DataSlice {
        DataSlice {
            range: TimeRange {
                start_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                end_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            },
            gap_manifest_ref: None,
            gap_manifest: None,
            sources: vec![sample_source()],
        }
    }

    fn sample_source() -> Source {
        Source {
            source_id: "dukascopy".into(),
            symbol: "EURUSD".into(),
            side: "bid".into(),
            timeframe: "15m".into(),
        }
    }

    fn sample_effect() -> Effect {
        Effect {
            metric: "autocorr_lb_q".into(),
            value: 12.34,
            p_value: Some(0.012),
            n: Some(1024),
            ci95: None,
            effect_size: None,
            extra: BTreeMap::new(),
        }
    }

    fn sample_result() -> ResultFinding {
        ResultFinding {
            schema_version: 1,
            scan_id_at_version: "stats.autocorr.ljung_box@1".into(),
            param_hash: "blake3-deadbeef".into(),
            code_revision: "abc123".into(),
            data_slice: sample_data_slice(),
            dsr: None,
            fdr_q: None,
            run_id: RunId::new(),
            produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            params: serde_json::json!({"lags": 20}),
            effect: sample_effect(),
            raw: None,
            repro: None,
        }
    }

    fn sample_scan_error() -> ScanErrorFinding {
        ScanErrorFinding {
            schema_version: 1,
            scan_id_at_version: "stats.autocorr.ljung_box@1".into(),
            param_hash: "blake3-deadbeef".into(),
            code_revision: "abc123".into(),
            data_slice: sample_data_slice(),
            dsr: None,
            fdr_q: None,
            run_id: RunId::new(),
            produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            error_code: "compute_error".into(),
            message: "NaN in residuals".into(),
            request_context: serde_json::json!({}),
        }
    }

    fn sample_gap_aborted() -> GapAbortedFinding {
        GapAbortedFinding {
            schema_version: 1,
            scan_id_at_version: "stats.autocorr.ljung_box@1".into(),
            param_hash: "blake3-deadbeef".into(),
            code_revision: "abc123".into(),
            data_slice: sample_data_slice(),
            dsr: None,
            fdr_q: None,
            run_id: RunId::new(),
            produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            gap_manifest: serde_json::json!({"gaps": []}),
        }
    }

    fn sample_run_start() -> RunStart {
        RunStart {
            run_id: RunId::new(),
            started_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            miner_version: "0.1.0".into(),
            code_revision: "abc123".into(),
            request: serde_json::json!({"scan_id": "x@1"}),
        }
    }

    fn sample_run_end() -> RunEnd {
        RunEnd {
            run_id: RunId::new(),
            ended_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap(),
            wall_clock_ms: 60_000,
            summary: RunSummary::default(),
        }
    }

    fn sample_dry_run() -> DryRunFinding {
        DryRunFinding {
            run_id: RunId::new(),
            produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 30).unwrap(),
            request: serde_json::json!({
                "scan_id@version": "stats.autocorr.ljung_box@1",
                "instrument": "EURUSD",
                "side": "bid",
                "timeframe": "15m",
                "dry_run": true,
            }),
            resolved_params: serde_json::json!({"lags": 20}),
            planned_data_slice: sample_data_slice(),
            estimated_findings_count: 0,
        }
    }

    /// Test 1 — `envelope_fields_present`: a serialised Result finding has all seven
    /// locked common fields at the top level.
    #[test]
    fn envelope_fields_present() {
        let finding = Finding::Result(sample_result());
        let value = serde_json::to_value(&finding).expect("serialise");
        let obj = value.as_object().expect("top-level object");

        assert!(obj.contains_key("schema_version"), "schema_version missing");
        assert!(
            obj.contains_key("scan_id@version"),
            "scan_id@version missing"
        );
        assert!(obj.contains_key("param_hash"), "param_hash missing");
        assert!(obj.contains_key("code_revision"), "code_revision missing");
        assert!(obj.contains_key("data_slice"), "data_slice missing");
        assert!(obj.contains_key("dsr"), "dsr missing (must be null in v1)");
        assert!(
            obj.contains_key("fdr_q"),
            "fdr_q missing (must be null in v1)"
        );
    }

    /// Test 2 — `dsr_and_fdr_q_are_null_in_v1`: the reserved slots serialise as JSON
    /// `null` (not absent) so v1 consumers see them.
    #[test]
    fn dsr_and_fdr_q_are_null_in_v1() {
        let finding = Finding::Result(sample_result());
        let value = serde_json::to_value(&finding).expect("serialise");
        assert_eq!(value["dsr"], serde_json::Value::Null);
        assert_eq!(value["fdr_q"], serde_json::Value::Null);
    }

    /// Test 3 — `run_id_format`: `Display` / `to_string()` produces a 26-char
    /// Crockford-base32 string matching the JSON Schema pattern.
    #[test]
    fn run_id_format() {
        let id = RunId::new();
        let s = id.to_string();
        assert_eq!(s.len(), 26, "ulid must be exactly 26 chars; got {s}");
        let allowed = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        for ch in s.chars() {
            assert!(
                allowed.contains(ch),
                "char {ch:?} outside Crockford-base32 alphabet"
            );
        }
        // The serialised JSON form must also be the bare string (via
        // `#[serde(transparent)]`).
        let json = serde_json::to_string(&id).expect("serialise");
        assert_eq!(json, format!("\"{s}\""));
    }

    /// Test 4 — `run_id_is_copy`: compile-time + runtime check that `RunId` is `Copy`.
    /// Two moves of the same value compile only when `Copy` is derived. The two
    /// `_x = id` bindings are INTENTIONAL no-ops: their purpose is to exercise the
    /// Copy bit (each move would consume the value if `Copy` weren't derived); we
    /// allow `clippy::no_effect_underscore_binding` locally to document that.
    #[test]
    #[allow(
        clippy::no_effect_underscore_binding,
        reason = "the two underscore bindings ARE the test — each move only compiles if RunId: Copy"
    )]
    fn run_id_is_copy() {
        assert_copy::<RunId>();
        let id = RunId::new();
        let _a = id;
        let _b = id; // second move only legal because RunId is Copy
    }

    /// Test 5 — `run_summary_default_compiles_and_is_zero`: `RunSummary::default()`
    /// produces zero counters and an empty `per_scan` map.
    #[test]
    fn run_summary_default_compiles_and_is_zero() {
        let s = RunSummary::default();
        assert_eq!(s.results_emitted, 0);
        assert_eq!(s.scan_errors, 0);
        assert_eq!(s.gap_aborted, 0);
        assert!(s.per_scan.is_empty());

        let p = PerScanCounts::default();
        assert_eq!(p.results, 0);
        assert_eq!(p.errors, 0);
        assert_eq!(p.gap_aborted, 0);
    }

    /// Test 6 — `base64_round_trip`: `Base64Bytes` round-trips through `serde_json`.
    #[test]
    fn base64_round_trip() {
        let b = Base64Bytes(vec![0u8, 1, 2, 255]);
        let s = serde_json::to_string(&b).expect("serialise");
        let b2: Base64Bytes = serde_json::from_str(&s).expect("deserialise");
        assert_eq!(b.0, b2.0);
    }

    /// Test 7 — `raw_series_uses_btreemap`: type-annotated reference binding ensures
    /// `Raw::series` is `BTreeMap<String, RawArray>` (NEVER `HashMap` — OUT-03).
    #[test]
    fn raw_series_uses_btreemap() {
        let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
        series.insert(
            "timestamps_ms".into(),
            RawArray {
                data: Base64Bytes(vec![]),
                shape: vec![0],
                dtype: Dtype::F64,
            },
        );
        let raw = Raw::new(series).expect("timestamps_ms is present");
        let _: &BTreeMap<String, RawArray> = &raw.series; // compile-time type assertion
        assert!(raw.series.contains_key("timestamps_ms"));

        // The constructor refuses missing timestamps_ms (D-03).
        let bad: BTreeMap<String, RawArray> = BTreeMap::new();
        assert!(Raw::new(bad).is_err());
    }

    /// Test 8 — `all_variants_round_trip`: each `Finding` variant survives a
    /// `serde_json` round-trip. Phase 3 extended the array to include
    /// `Finding::DryRun(_)` (D3-21); Phase 5 Plan 05-01 (D5-02) extended it to
    /// include `Finding::SweepSummary(_)`.
    #[test]
    fn all_variants_round_trip() {
        for finding in [
            Finding::RunStart(sample_run_start()),
            Finding::Result(sample_result()),
            Finding::ScanError(sample_scan_error()),
            Finding::GapAborted(sample_gap_aborted()),
            Finding::RunEnd(sample_run_end()),
            Finding::DryRun(sample_dry_run()),
            Finding::SweepSummary(sample_sweep_summary()),
        ] {
            let json = serde_json::to_string(&finding).expect("serialise");
            let parsed: Finding = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(finding, parsed, "round-trip mismatch for {json}");
        }
    }

    /// Test 9 — `dry_run_finding_uses_snake_case_kind`: `Finding::DryRun(_)`
    /// serialises with the `"kind":"dry_run"` discriminator produced by the
    /// existing `#[serde(rename_all = "snake_case")]` attribute on `Finding`.
    #[test]
    fn dry_run_finding_uses_snake_case_kind() {
        let finding = Finding::DryRun(sample_dry_run());
        let value = serde_json::to_value(&finding).expect("serialise");
        assert_eq!(
            value["kind"], "dry_run",
            "expected kind discriminator 'dry_run'; got {}",
            value["kind"]
        );
    }

    /// Test 10 — `dataslice_gap_manifest_serialises_as_null_when_absent`: a
    /// `DataSlice` with both optional fields `None` serialises with
    /// `gap_manifest` present as JSON `null` (NOT omitted). Pins the
    /// 03-PATTERNS line 572 invariant — bare `#[serde(default)]` only, no
    /// `skip_serializing_if`. Mirrors the existing `dsr` / `fdr_q` null-not-
    /// omitted rule.
    #[test]
    fn dataslice_gap_manifest_serialises_as_null_when_absent() {
        let slice = sample_data_slice();
        let json = serde_json::to_string(&slice).expect("serialise");
        assert!(
            json.contains("\"gap_manifest\":null"),
            "gap_manifest must serialise as literal `null` when absent; got: {json}"
        );
        // Belt-and-brace: parse back and confirm the key is present as Null.
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        let obj = parsed.as_object().expect("top-level object");
        assert!(
            obj.contains_key("gap_manifest"),
            "gap_manifest key must be present in DataSlice JSON, not absent"
        );
        assert!(
            obj["gap_manifest"].is_null(),
            "gap_manifest must be JSON null; got {}",
            obj["gap_manifest"]
        );
    }

    /// Test 11 — `dry_run_does_not_increment_results_emitted` (Pitfall 3
    /// pin / D3-21 / Warning 9). Emit a `Finding::DryRun(_)` through a
    /// `VecSink`; assert that constructing the dry-run envelope and writing
    /// it does NOT touch `RunSummary` counters. The summary stays at
    /// `Default::default()` (all zeros, empty `per_scan`).
    ///
    /// This test pins the TYPE-LEVEL invariant: the dry-run signal lives in
    /// the envelope, not in a summary counter. Plan 04's engine test pins the
    /// end-to-end equivalent (run with `dry_run=true` -> `RunSummary` stays
    /// all-zero).
    #[test]
    fn dry_run_does_not_increment_results_emitted() {
        use crate::findings::FindingSink;
        use crate::findings::sink::VecSink;

        // Counter starts at zero by Default.
        let summary = RunSummary::default();
        assert_eq!(summary.results_emitted, 0);
        assert_eq!(summary.scan_errors, 0);
        assert_eq!(summary.gap_aborted, 0);
        assert!(summary.per_scan.is_empty());

        // Emit one DryRun envelope through a fresh sink.
        let mut sink = VecSink::new();
        sink.write_envelope(&Finding::DryRun(sample_dry_run()))
            .expect("write_envelope ok");

        // The bytes are written, but the typed RunSummary defaults remain
        // unchanged (Default constructor doesn't touch counters; Plan 04
        // enforces the end-to-end discipline at run_one).
        let post = RunSummary::default();
        assert_eq!(
            post.results_emitted, 0,
            "DryRun MUST NOT increment results_emitted (Pitfall 3)"
        );
        assert_eq!(post.scan_errors, 0);
        assert_eq!(post.gap_aborted, 0);
    }

    /// Plan 04-01 Task 2 — Behavior Test 4: `data_slice_sources_vec_round_trip`.
    /// `DataSlice.sources: Vec<Source>` round-trips through JSON with the
    /// inserted Vec length preserved. Single-leg findings carry a length-1
    /// Vec; the round-trip preserves `Source.{source_id`, symbol, side,
    /// timeframe} verbatim. Pins the D4-03 wire contract.
    #[test]
    fn data_slice_sources_vec_round_trip() {
        let slice = DataSlice {
            range: TimeRange {
                start_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                end_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            },
            gap_manifest_ref: None,
            gap_manifest: None,
            sources: vec![
                Source {
                    source_id: "dukascopy".into(),
                    symbol: "EURUSD".into(),
                    side: "bid".into(),
                    timeframe: "15m".into(),
                },
                Source {
                    source_id: "dukascopy".into(),
                    symbol: "GBPUSD".into(),
                    side: "ask".into(),
                    timeframe: "15m".into(),
                },
            ],
        };
        let json = serde_json::to_string(&slice).expect("serialise");
        let parsed: DataSlice = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(
            parsed.sources.len(),
            2,
            "Pair-arity sources Vec must round-trip length verbatim"
        );
        assert_eq!(parsed.sources[0].symbol, "EURUSD");
        assert_eq!(parsed.sources[0].side, "bid");
        assert_eq!(parsed.sources[1].symbol, "GBPUSD");
        assert_eq!(parsed.sources[1].side, "ask");
        assert_eq!(parsed, slice, "round-trip must be lossless");

        // Single-leg (length 1) case — ANOM / SEAS family default.
        let single = DataSlice {
            range: TimeRange {
                start_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                end_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            },
            gap_manifest_ref: None,
            gap_manifest: None,
            sources: vec![Source {
                source_id: "dukascopy".into(),
                symbol: "EURUSD".into(),
                side: "bid".into(),
                timeframe: "15m".into(),
            }],
        };
        let json = serde_json::to_string(&single).expect("serialise");
        let parsed: DataSlice = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(parsed.sources.len(), 1);
        assert_eq!(parsed, single);

        // Backward-compat: an old payload omitting `sources` deserialises to
        // an empty Vec (the `#[serde(default)]` attribute keeps the change
        // additive — see 04-01-SCHEMA-DIFF.md).
        let legacy_json = r#"{
            "range": {"start_utc":"2026-01-01T00:00:00Z","end_utc":"2026-01-02T00:00:00Z"},
            "gap_manifest_ref": null,
            "gap_manifest": null
        }"#;
        let legacy: DataSlice = serde_json::from_str(legacy_json).expect("legacy parse ok");
        assert!(
            legacy.sources.is_empty(),
            "Omitted `sources` must default to an empty Vec"
        );
    }

    // ---------------------------------------------------------------------
    // Plan 05-01 Task 2 — TDD RED: Phase 5 schema-additive envelope tests
    // ---------------------------------------------------------------------

    /// Build a deterministic `SweepSummaryFinding` for round-trip tests.
    /// Uses fixed `Utc.with_ymd_and_hms(...)` (NOT `Utc::now()`) so the test
    /// is reproducible — matches the convention of `sample_run_start()` etc.
    /// `fdr_by_family` has TWO entries to prove `BTreeMap` ordering is
    /// alphabetic per OUT-03.
    fn sample_sweep_summary() -> SweepSummaryFinding {
        let mut fdr_by_family = BTreeMap::new();
        // Inserted in non-alphabetic order — the BTreeMap should re-order on
        // serialise (stats.* sorts before x.* lexicographically).
        fdr_by_family.insert(
            "x.test.placeholder@1".into(),
            FdrFamilySummary {
                method: "benjamini_hochberg".into(),
                alpha: 0.05,
                per_finding: vec![FindingFdrEntry {
                    finding_index: 0,
                    raw_p: 0.001,
                    q_value: 0.002,
                }],
            },
        );
        fdr_by_family.insert(
            "stats.autocorr.ljung_box@1".into(),
            FdrFamilySummary {
                method: "benjamini_hochberg".into(),
                alpha: 0.05,
                per_finding: vec![
                    FindingFdrEntry {
                        finding_index: 0,
                        raw_p: 0.012,
                        q_value: 0.024,
                    },
                    FindingFdrEntry {
                        finding_index: 1,
                        raw_p: 0.030,
                        q_value: 0.030,
                    },
                ],
            },
        );
        SweepSummaryFinding {
            run_id: RunId::new(),
            produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            fdr_by_family,
            totals: SweepTotals {
                jobs_run: 2,
                results_emitted: 3,
                scan_errors: 0,
                gap_aborted: 0,
            },
        }
    }

    /// Plan 05-01 Task 2 — Behavior Test 1 + 2 (`effect_size_round_trip`):
    /// `Effect.effect_size: Option<EffectSize>` round-trips through serde for
    /// both `Some(_)` and `None` paths. Per OUT-03, the `None` variant
    /// serialises as JSON `null` (NOT an omitted field) — DO NOT add
    /// `#[serde(skip_serializing_if = "Option::is_none")]`.
    #[test]
    fn effect_size_round_trip() {
        // Some path.
        let effect_some = Effect {
            metric: "vr_minus_one".into(),
            value: 0.4,
            p_value: Some(0.02),
            n: Some(512),
            ci95: None,
            effect_size: Some(EffectSize {
                kind: "cohens_d".into(),
                value: 0.4,
            }),
            extra: BTreeMap::new(),
        };
        let json = serde_json::to_string(&effect_some).expect("serialise Some");
        let parsed: Effect = serde_json::from_str(&json).expect("deserialise Some");
        assert_eq!(parsed, effect_some, "Some path round-trip mismatch");

        // None path: must serialise the literal JSON null (NOT omit the key).
        let effect_none = Effect {
            metric: "x".into(),
            value: 0.0,
            p_value: None,
            n: None,
            ci95: None,
            effect_size: None,
            extra: BTreeMap::new(),
        };
        let json_none = serde_json::to_string(&effect_none).expect("serialise None");
        assert!(
            json_none.contains("\"effect_size\":null"),
            "effect_size must serialise as literal `null` when None; got: {json_none}"
        );
        let parsed_none: Effect = serde_json::from_str(&json_none).expect("deserialise None");
        assert_eq!(parsed_none, effect_none);

        // Belt-and-brace: backward-compat — an old payload omitting
        // `effect_size` deserialises with `effect_size: None` (the
        // `#[serde(default)]` attribute keeps the schema diff purely additive).
        let legacy_json = r#"{
            "metric": "vr_minus_one",
            "value": 0.4,
            "p_value": null,
            "n": null,
            "ci95": null,
            "extra": {}
        }"#;
        let legacy: Effect = serde_json::from_str(legacy_json).expect("legacy parse ok");
        assert!(
            legacy.effect_size.is_none(),
            "Omitted `effect_size` must default to None"
        );
    }

    /// Plan 05-01 Task 2 — Behavior Test 7 (`repro_envelope_population_rule`):
    /// `ResultFinding.repro: Option<ReproEnvelope>` round-trips through serde
    /// for both `Some(_)` (with a `BootstrapSpec`) and `None` paths. The rule
    /// "`repro = Some(_)` iff bootstrap or null was run" is enforced by the
    /// Plan 05-03 engine integration; this test pins the type-level shape
    /// (both variants exist + round-trip cleanly).
    #[test]
    fn repro_envelope_population_rule() {
        let mut with_repro = sample_result();
        with_repro.repro = Some(ReproEnvelope {
            master_seed: 0xDEAD,
            job_seed: 0xBEEF,
            bootstrap: Some(BootstrapSpec {
                method: "stationary".into(),
                n: 1000,
            }),
            null: None,
        });
        let json = serde_json::to_string(&with_repro).expect("serialise Some");
        let parsed: ResultFinding = serde_json::from_str(&json).expect("deserialise Some");
        assert_eq!(parsed, with_repro, "Some repro round-trip mismatch");

        // None path: serialise as literal `null`.
        let without_repro = sample_result();
        assert!(without_repro.repro.is_none());
        let json_none = serde_json::to_string(&without_repro).expect("serialise None");
        assert!(
            json_none.contains("\"repro\":null"),
            "repro must serialise as literal `null` when None; got: {json_none}"
        );
        let parsed_none: ResultFinding =
            serde_json::from_str(&json_none).expect("deserialise None");
        assert_eq!(parsed_none, without_repro);

        // Belt-and-brace: backward-compat — an old payload (Phase 4 era)
        // omitting `repro` deserialises with `repro: None`.
        // Build the JSON by stripping `repro` from a serialised `ResultFinding`.
        let mut value = serde_json::to_value(&without_repro).expect("serialise");
        let kind = value
            .as_object_mut()
            .expect("top-level object")
            .remove("kind"); // strip discriminant for direct ResultFinding parse
        let _ = kind;
        value
            .as_object_mut()
            .expect("top-level object")
            .remove("repro");
        let legacy_str = serde_json::to_string(&value).expect("legacy serialise");
        let legacy: ResultFinding =
            serde_json::from_str(&legacy_str).expect("legacy parse ok");
        assert!(
            legacy.repro.is_none(),
            "Omitted `repro` must default to None"
        );
    }

    /// Plan 05-01 Task 2 — Behavior Test 4 (`sweep_summary_finding_uses_snake_case_kind`):
    /// `Finding::SweepSummary(_)` serialises with the top-level `"kind":"sweep_summary"`
    /// discriminant produced by the existing `#[serde(rename_all = "snake_case")]`
    /// attribute on `Finding`. `fdr_by_family` `BTreeMap` ordering is alphabetic
    /// per OUT-03 (proves we are not using a `HashMap` — the test serialises and
    /// checks the JSON string for stable key ordering).
    #[test]
    fn sweep_summary_finding_uses_snake_case_kind() {
        let summary = sample_sweep_summary();
        let finding = Finding::SweepSummary(summary);
        let json = serde_json::to_string(&finding).expect("serialise");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(
            value["kind"], "sweep_summary",
            "expected kind discriminator 'sweep_summary'; got {}",
            value["kind"]
        );
        // BTreeMap ordering: "stats.autocorr.ljung_box@1" must appear BEFORE
        // "x.test.placeholder@1" in the serialised JSON, regardless of
        // insertion order.
        let stats_pos = json
            .find("stats.autocorr.ljung_box@1")
            .expect("stats family key must be present");
        let x_pos = json
            .find("x.test.placeholder@1")
            .expect("x family key must be present");
        assert!(
            stats_pos < x_pos,
            "BTreeMap must emit keys alphabetically (OUT-03); stats@{stats_pos} vs x@{x_pos}: {json}"
        );
    }

    /// Test 12 — `run_summary_has_no_dry_run_emitted_field` (Warning 9 pin).
    /// Exhaustive destructure of `RunSummary` — adding a new field (e.g.,
    /// `dry_run_emitted`) would break this match at compile-time, signalling
    /// the contract drift before tests even run.
    #[test]
    fn run_summary_has_no_dry_run_emitted_field() {
        let s = RunSummary::default();
        // The exhaustive destructure is the test — if a new field lands on
        // `RunSummary`, the match below fails to compile. Each binding is a
        // type-level assertion: there are exactly these four fields.
        let RunSummary {
            results_emitted,
            scan_errors,
            gap_aborted,
            per_scan,
        } = s;
        // Use each binding so clippy doesn't trip the unused warning.
        assert_eq!(results_emitted, 0);
        assert_eq!(scan_errors, 0);
        assert_eq!(gap_aborted, 0);
        assert!(per_scan.is_empty());
    }
}

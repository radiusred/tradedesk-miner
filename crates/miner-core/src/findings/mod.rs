//! `Finding` envelope types — the centrepiece contract of Phase 1.
//!
//! Every miner invocation produces a stream of `Finding` values (one JSON object per line
//! on stdout via `FindingSink`). Five variants — `RunStart`, `Result`, `ScanError`,
//! `GapAborted`, `RunEnd` — discriminated by the `kind` field (`#[serde(tag = "kind")]`).
//!
//! ## Locked envelope fields (D-12..D-14, OUT-02)
//!
//! Three of the five variants (`Result`, `ScanError`, `GapAborted`) carry these seven
//! locked common fields, INLINED into each variant payload struct (NOT via
//! `#[serde(flatten)]` — see RESEARCH §Anti-Patterns). The framing records (`RunStart`,
//! `RunEnd`) intentionally do NOT carry them per D-09.
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

pub mod base64_bytes;
pub mod run_id;
// `pub mod sink;` is added in Task 2 of Plan 03 (sink.rs has cross-dependencies on
// `crate::error::MinerError` which is defined in that same task).

pub use base64_bytes::{Base64Bytes, Dtype};
pub use run_id::RunId;

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
/// `gap_manifest_ref` is `None` in v1; Phase 3 populates it under the
/// `continuous_only` gap policy (D-08).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DataSlice {
    pub range: TimeRange,
    pub gap_manifest_ref: Option<String>,
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
    /// Returns `Err` with a static message when the invariant is violated. Production
    /// callers should map this into a `MinerError::Internal` at the scan boundary.
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
    /// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
    #[serde(default)]
    pub extra: BTreeMap<String, RawArray>,
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
    pub source: Source,
    /// Resolved scan parameters (post-defaults).
    pub params: serde_json::Value,
    pub effect: Effect,
    /// Optional inputs the scan consumed (D-04 input/output split).
    pub raw: Option<Raw>,
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
    pub source: Source,
    pub gap_manifest: serde_json::Value,
}

// ---------------------------------------------------------------------------
// The tagged `Finding` enum — single Rust type produces every JSON variant
// ---------------------------------------------------------------------------

/// The five-variant Finding envelope discriminated by the JSON `kind` field.
///
/// Per RESEARCH §Anti-Patterns: do NOT use `#[serde(flatten)]` on a "common-fields"
/// struct. The seven locked envelope fields are inlined directly into each variant
/// payload struct that carries them (Result, `ScanError`, `GapAborted`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    RunStart(RunStart),
    Result(ResultFinding),
    ScanError(ScanErrorFinding),
    GapAborted(GapAbortedFinding),
    RunEnd(RunEnd),
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
            source: sample_source(),
            params: serde_json::json!({"lags": 20}),
            effect: sample_effect(),
            raw: None,
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
            source: sample_source(),
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
    /// Two moves of the same value compile only when `Copy` is derived.
    #[test]
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

    /// Test 6 — `base64_round_trip`: Base64Bytes round-trips through serde_json.
    #[test]
    fn base64_round_trip() {
        let b = Base64Bytes(vec![0u8, 1, 2, 255]);
        let s = serde_json::to_string(&b).expect("serialise");
        let b2: Base64Bytes = serde_json::from_str(&s).expect("deserialise");
        assert_eq!(b.0, b2.0);
    }

    /// Test 7 — `raw_series_uses_btreemap`: type-annotated reference binding ensures
    /// `Raw::series` is `BTreeMap<String, RawArray>` (NEVER HashMap — OUT-03).
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

    /// Test 8 — `all_five_variants_round_trip`: each Finding variant survives a
    /// serde_json round-trip.
    #[test]
    fn all_five_variants_round_trip() {
        for finding in [
            Finding::RunStart(sample_run_start()),
            Finding::Result(sample_result()),
            Finding::ScanError(sample_scan_error()),
            Finding::GapAborted(sample_gap_aborted()),
            Finding::RunEnd(sample_run_end()),
        ] {
            let json = serde_json::to_string(&finding).expect("serialise");
            let parsed: Finding = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(finding, parsed, "round-trip mismatch for {json}");
        }
    }
}

//! Locked Phase 1 `error_code` vocabulary (per 01-RESEARCH §"`error_code` Vocabulary").
//!
//! Two contexts (D-05 / D-06):
//! - [`PreflightCode`] — emitted to stderr before any scan work begins (pre-flight
//!   rejection path). Seven variants.
//! - [`ScanErrorCode`] — emitted inside `kind: scan_error` findings during a run
//!   (mid-stream error path). Four variants.
//!
//! [`WireError`] is the wire-form: `{ code, message, context }`. `code` is `String`
//! (NOT a typed enum) for additive extensibility — new codes in later phases do not
//! break the schema.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Pre-flight error codes (D-06). Emitted to stderr as the `error` field of a
/// structured-error JSON line before any scan work begins. Exit code is 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreflightCode {
    /// A CLI / MCP / HTTP parameter failed type / range / enum validation.
    InvalidParameter,
    /// The requested `scan_id@version` is not in the registry.
    UnknownScan,
    /// The requested instrument is not in the source catalog.
    UnknownInstrument,
    /// Phase 4 (Plan 04-01 / D4-02) — `ScanRequest.instruments.len()` does
    /// not match the scan's declared `Scan::arity().expected_len()`.
    /// Emitted by `engine::preflight::validate_arity` (Plan 04-02) before
    /// any reader or cache work; the wire-form `code` string is
    /// `"wrong_instrument_arity"`. Variant positioned alphabetically-by-
    /// semantic-group AFTER `UnknownScan` per PATTERNS.md Pattern G.
    WrongInstrumentArity,
    /// A required `MinerConfig` field could not be resolved from any layer.
    MissingRequiredConfig,
    /// A config file / env value failed parse or type-check.
    InvalidConfig,
    /// Sweep cardinality exceeds a configured upper bound (Phase 5+).
    SweepTooLarge,
    /// Catastrophic failure unrelated to inputs.
    InternalError,
}

impl PreflightCode {
    /// `snake_case` wire form — used to construct `WireError::code` from a typed value.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            PreflightCode::InvalidParameter => "invalid_parameter",
            PreflightCode::UnknownScan => "unknown_scan",
            PreflightCode::UnknownInstrument => "unknown_instrument",
            PreflightCode::WrongInstrumentArity => "wrong_instrument_arity",
            PreflightCode::MissingRequiredConfig => "missing_required_config",
            PreflightCode::InvalidConfig => "invalid_config",
            PreflightCode::SweepTooLarge => "sweep_too_large",
            PreflightCode::InternalError => "internal_error",
        }
    }
}

/// Mid-stream scan-error codes (D-05). Emitted inside `kind: scan_error` envelopes
/// during a run. The schema treats `error_code` as an open string, so additional codes
/// added in later phases are non-breaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanErrorCode {
    /// A scan discovered a gap manifest mid-run that the gap policy disallowed.
    CoverageGap,
    /// A scan's compute kernel failed (generic catch-all for Phase 4+).
    ComputeError,
    /// Derived-bar cache read failed checksum / version (Phase 2+).
    CacheCorruption,
    /// A panic was caught at the scan boundary. Should be vanishingly rare.
    InternalPanicCaught,
}

impl ScanErrorCode {
    /// `snake_case` wire form — used to construct `WireError::code` from a typed value.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            ScanErrorCode::CoverageGap => "coverage_gap",
            ScanErrorCode::ComputeError => "compute_error",
            ScanErrorCode::CacheCorruption => "cache_corruption",
            ScanErrorCode::InternalPanicCaught => "internal_panic_caught",
        }
    }
}

/// Wire form for errors embedded in `kind: scan_error` findings or emitted to stderr
/// during pre-flight rejection.
///
/// `code` is `String` (NOT a sealed enum) — adding new codes is additive and does
/// not break the JSON Schema for consumers. Internally, callers should construct a
/// `WireError` from a typed [`PreflightCode`] / [`ScanErrorCode`] via the helpers
/// below; this gives compile-time guarantees for the locked Phase 1 vocabulary
/// without sealing the wire-format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WireError {
    pub code: String,
    pub message: String,
    /// Free-form context map. `BTreeMap` for deterministic ordering (OUT-03).
    #[serde(default)]
    pub context: BTreeMap<String, serde_json::Value>,
}

impl WireError {
    /// Construct a `WireError` from a typed [`PreflightCode`].
    #[must_use]
    pub fn preflight(code: PreflightCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: message.into(),
            context: BTreeMap::new(),
        }
    }

    /// Construct a `WireError` from a typed [`ScanErrorCode`].
    #[must_use]
    pub fn scan(code: ScanErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: message.into(),
            context: BTreeMap::new(),
        }
    }

    /// Builder-style helper: add a context key-value.
    #[must_use]
    pub fn with_context(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.context.insert(key.into(), value);
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 1 — `preflight_code_serialises_snake_case`: every variant round-trips
    /// through `serde_json` as the locked `snake_case` string. Phase 4 Plan
    /// 04-01 Task 3 added `WrongInstrumentArity` per D4-02; the test cases
    /// array now has 8 rows.
    #[test]
    fn preflight_code_serialises_snake_case() {
        let cases = [
            (PreflightCode::InvalidParameter, "invalid_parameter"),
            (PreflightCode::UnknownScan, "unknown_scan"),
            (PreflightCode::UnknownInstrument, "unknown_instrument"),
            (
                PreflightCode::WrongInstrumentArity,
                "wrong_instrument_arity",
            ),
            (
                PreflightCode::MissingRequiredConfig,
                "missing_required_config",
            ),
            (PreflightCode::InvalidConfig, "invalid_config"),
            (PreflightCode::SweepTooLarge, "sweep_too_large"),
            (PreflightCode::InternalError, "internal_error"),
        ];
        for (code, expected) in cases {
            let json = serde_json::to_string(&code).expect("serialise");
            assert_eq!(json, format!("\"{expected}\""), "wire form mismatch");
            let parsed: PreflightCode = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(parsed, code);
            assert_eq!(code.as_str(), expected);
        }
    }

    /// Plan 04-01 Task 3 — Test 2: `preflight_code_as_str_wrong_instrument_arity`.
    /// Sibling test pinning the `as_str()` return for the new D4-02 variant
    /// independently of the cases-array driver above.
    #[test]
    fn preflight_code_as_str_wrong_instrument_arity() {
        assert_eq!(
            PreflightCode::WrongInstrumentArity.as_str(),
            "wrong_instrument_arity"
        );
    }

    /// Test 2 — `scan_error_code_serialises_snake_case`: every variant round-trips.
    #[test]
    fn scan_error_code_serialises_snake_case() {
        let cases = [
            (ScanErrorCode::CoverageGap, "coverage_gap"),
            (ScanErrorCode::ComputeError, "compute_error"),
            (ScanErrorCode::CacheCorruption, "cache_corruption"),
            (ScanErrorCode::InternalPanicCaught, "internal_panic_caught"),
        ];
        for (code, expected) in cases {
            let json = serde_json::to_string(&code).expect("serialise");
            assert_eq!(json, format!("\"{expected}\""), "wire form mismatch");
            let parsed: ScanErrorCode = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(parsed, code);
            assert_eq!(code.as_str(), expected);
        }
    }

    /// Test 3 — `wire_error_serialises_open_string_code`: `code` is `String`, so an
    /// arbitrary (e.g., future) code string is accepted.
    #[test]
    fn wire_error_serialises_open_string_code() {
        let err = WireError {
            code: "future_unknown_code".to_string(),
            message: "x".into(),
            context: BTreeMap::new(),
        };
        let json = serde_json::to_string(&err).expect("serialise");
        assert!(
            json.contains("\"future_unknown_code\""),
            "open-string code missing from {json}"
        );
        let parsed: WireError = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(parsed.code, "future_unknown_code");
        assert_eq!(parsed.message, "x");
    }
}

//! Plan 07 Task 1 — runtime schema validation across every `Finding` variant (D-22).
//!
//! Loads the COMMITTED `schemas/findings-v1.schema.json` artifact (NOT a freshly-generated
//! one) and validates one example of each `Finding` variant against it using the
//! `jsonschema` crate. The schema is loaded as JSON Schema 2020-12 via
//! `jsonschema::validator_for`.
//!
//! This test closes the FOUND-03 + OUT-02 contract end-to-end:
//!
//! - FOUND-03: the schema is the locked envelope contract; every runtime `Finding`
//!   instance must validate against it. Combined with Plan 06's CI sync gate, drift
//!   between Rust types and the committed schema fails in TWO places (locally on
//!   `cargo test` AND in CI on the schema-sync diff).
//! - OUT-02: `dsr` and `fdr_q` MUST serialise as JSON `null` (NOT absent) in v1.
//!   `dsr_and_fdr_q_present_as_null_in_v1` asserts this against the parsed JSON.
//!
//! Mitigates threat T-01-02 (schema drift). Adding a field to a Rust variant struct
//! without regenerating the schema makes the new instance fail validation here.
//!
//! The schema path is resolved via `env!("CARGO_MANIFEST_DIR")` + `../../schemas/`. From
//! the miner-core crate at `crates/miner-core/`, that walks up to the workspace root.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{TimeZone, Utc};
use miner_core::{
    Base64Bytes, DataSlice, Dtype, Effect, Finding, GapAbortedFinding, PerScanCounts, Raw,
    RawArray, ResultFinding, RunEnd, RunId, RunStart, RunSummary, ScanErrorFinding, Source,
    TimeRange,
};

// ---------------------------------------------------------------------------
// Schema loader — locate the committed artifact relative to this crate's
// manifest directory. `env!("CARGO_MANIFEST_DIR")` is the path to
// `crates/miner-core/`; walking up two levels lands at the workspace root.
// ---------------------------------------------------------------------------

fn load_validator() -> jsonschema::Validator {
    let schema_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/findings-v1.schema.json");
    let schema_text = std::fs::read_to_string(&schema_path)
        .unwrap_or_else(|e| panic!("read schema at {}: {}", schema_path.display(), e));
    let schema_json: serde_json::Value = serde_json::from_str(&schema_text)
        .expect("schemas/findings-v1.schema.json must be valid JSON");
    jsonschema::validator_for(&schema_json).expect("schema must be a valid JSON Schema 2020-12")
}

// ---------------------------------------------------------------------------
// Sample-instance constructors (mirror the in-crate unit-test fixtures in
// `findings/mod.rs::tests` but live here at the integration boundary).
// ---------------------------------------------------------------------------

fn sample_data_slice() -> DataSlice {
    DataSlice {
        range: TimeRange {
            start_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            end_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
        },
        gap_manifest_ref: None,
        gap_manifest: None,
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

fn sample_effect_empty_extra() -> Effect {
    Effect {
        metric: "autocorr_lb_q".into(),
        value: 12.34,
        p_value: Some(0.012),
        n: Some(1024),
        ci95: Some([0.5, 1.5]),
        extra: BTreeMap::new(),
    }
}

fn sample_run_start() -> Finding {
    Finding::RunStart(RunStart {
        run_id: RunId::new(),
        started_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        miner_version: "0.1.0".into(),
        code_revision: "abc123def456".into(),
        request: serde_json::json!({ "scan_id@version": "stats.autocorr.ljung_box@1" }),
    })
}

fn sample_run_end() -> Finding {
    let mut per_scan = BTreeMap::new();
    per_scan.insert(
        "stats.autocorr.ljung_box@1".to_string(),
        PerScanCounts {
            results: 7,
            errors: 0,
            gap_aborted: 0,
        },
    );
    Finding::RunEnd(RunEnd {
        run_id: RunId::new(),
        ended_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap(),
        wall_clock_ms: 60_000,
        summary: RunSummary {
            results_emitted: 7,
            scan_errors: 0,
            gap_aborted: 0,
            per_scan,
        },
    })
}

/// Build a `Result` finding WITHOUT the optional `raw` block.
fn sample_result_no_raw() -> Finding {
    Finding::Result(ResultFinding {
        schema_version: 1,
        scan_id_at_version: "stats.autocorr.ljung_box@1".into(),
        param_hash: "blake3_dummy_hash_64chars_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        code_revision: "abc123def456".into(),
        data_slice: sample_data_slice(),
        dsr: None,
        fdr_q: None,
        run_id: RunId::new(),
        produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
        source: sample_source(),
        params: serde_json::json!({"lags": 20}),
        effect: sample_effect_empty_extra(),
        raw: None,
    })
}

/// Build a `Result` finding WITH a populated `raw` block (D-03 `timestamps_ms` invariant).
fn sample_result_with_raw() -> Finding {
    let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
    series.insert(
        "timestamps_ms".into(),
        RawArray {
            // 2 LE-f64 values — content is irrelevant; the schema only checks shape and
            // that `data` is a base64 string.
            data: Base64Bytes(vec![0u8; 16]),
            shape: vec![2, 1],
            dtype: Dtype::F64,
        },
    );
    series.insert(
        "returns".into(),
        RawArray {
            data: Base64Bytes(vec![1u8; 16]),
            shape: vec![2, 1],
            dtype: Dtype::F64,
        },
    );
    let raw = Raw::new(series).expect("timestamps_ms is present");

    Finding::Result(ResultFinding {
        schema_version: 1,
        scan_id_at_version: "stats.autocorr.ljung_box@1".into(),
        param_hash: "blake3_dummy_hash_64chars_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        code_revision: "abc123def456".into(),
        data_slice: sample_data_slice(),
        dsr: None,
        fdr_q: None,
        run_id: RunId::new(),
        produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
        source: sample_source(),
        params: serde_json::json!({"lags": 20}),
        effect: sample_effect_empty_extra(),
        raw: Some(raw),
    })
}

fn sample_scan_error() -> Finding {
    Finding::ScanError(ScanErrorFinding {
        schema_version: 1,
        scan_id_at_version: "stats.autocorr.ljung_box@1".into(),
        param_hash: "blake3_dummy_hash_64chars_cccccccccccccccccccccccccccccccccccccccc".into(),
        code_revision: "abc123def456".into(),
        data_slice: sample_data_slice(),
        dsr: None,
        fdr_q: None,
        run_id: RunId::new(),
        produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
        error_code: "compute_error".into(),
        message: "NaN in residuals".into(),
        request_context: serde_json::json!({"symbol": "EURUSD"}),
    })
}

fn sample_gap_aborted() -> Finding {
    Finding::GapAborted(GapAbortedFinding {
        schema_version: 1,
        scan_id_at_version: "stats.autocorr.ljung_box@1".into(),
        param_hash: "blake3_dummy_hash_64chars_dddddddddddddddddddddddddddddddddddddddd".into(),
        code_revision: "abc123def456".into(),
        data_slice: sample_data_slice(),
        dsr: None,
        fdr_q: None,
        run_id: RunId::new(),
        produced_at_utc: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
        source: sample_source(),
        gap_manifest: serde_json::json!({"gaps": []}),
    })
}

/// Validate a single envelope; on failure, panic with a descriptive message
/// including the instance and the collected validation errors.
fn assert_validates(validator: &jsonschema::Validator, finding: &Finding, label: &str) {
    let instance = serde_json::to_value(finding).expect("serialise finding to JSON");
    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| format!("  - {e}"))
        .collect();
    assert!(
        errors.is_empty() && validator.is_valid(&instance),
        "schema validation FAILED for {label}\ninstance: {}\nerrors:\n{}",
        serde_json::to_string_pretty(&instance).unwrap(),
        errors.join("\n"),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Test 1 — `all_kinds_validate` (D-22).
///
/// Constructs one instance of each `Finding` variant — including BOTH "Result with
/// raw" and "Result without raw" — serialises each with `serde_json::to_value`,
/// loads the committed schema with `jsonschema::validator_for`, and asserts every
/// instance is valid. This is the canonical runtime schema-validation gate from
/// CONTEXT D-22.
#[test]
fn all_kinds_validate() {
    let validator = load_validator();

    assert_validates(&validator, &sample_run_start(), "RunStart");
    assert_validates(&validator, &sample_result_no_raw(), "Result (no raw)");
    assert_validates(&validator, &sample_result_with_raw(), "Result (with raw)");
    assert_validates(&validator, &sample_scan_error(), "ScanError");
    assert_validates(&validator, &sample_gap_aborted(), "GapAborted");
    assert_validates(&validator, &sample_run_end(), "RunEnd");
}

/// Test 2 — `dsr_and_fdr_q_present_as_null_in_v1` (OUT-02 reserved-but-null contract).
///
/// Serialise a `Result` finding, parse the JSON, and assert `dsr` and `fdr_q` are
/// present as `Value::Null` — NOT absent. Tests both `value["dsr"].is_null()` AND
/// that the keys are actually present (since `parsed["missing_key"]` returns
/// `Value::Null` from `Index` and would silently pass an `is_null()` check on a
/// missing key).
#[test]
fn dsr_and_fdr_q_present_as_null_in_v1() {
    let finding = sample_result_no_raw();
    let json = serde_json::to_string(&finding).expect("serialise");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
    let obj = parsed.as_object().expect("top-level is object");

    assert!(
        obj.contains_key("dsr"),
        "dsr key must be present in v1 envelope JSON, not absent"
    );
    assert!(
        obj.contains_key("fdr_q"),
        "fdr_q key must be present in v1 envelope JSON, not absent"
    );
    assert!(
        obj["dsr"].is_null(),
        "dsr must serialise as JSON null in v1; got {}",
        obj["dsr"]
    );
    assert!(
        obj["fdr_q"].is_null(),
        "fdr_q must serialise as JSON null in v1; got {}",
        obj["fdr_q"]
    );
}

/// Test 3 — `raw_array_content_encoding_path_works`.
///
/// A `Result` finding with a populated `raw.series` exercises the
/// `contentEncoding: base64` path in the schema (the `Base64Bytes` newtype's
/// manual `JsonSchema` impl emits this keyword). Validates against the committed
/// schema; success proves the contentEncoding code path is wired correctly.
#[test]
fn raw_array_content_encoding_path_works() {
    let validator = load_validator();
    let finding = sample_result_with_raw();
    assert_validates(
        &validator,
        &finding,
        "Result with raw.series (contentEncoding)",
    );

    // Also assert the produced JSON for `raw.series.timestamps_ms.data` is a string
    // (the base64-encoded form), not a number/array. This is the wire shape
    // promised by the schema's `Base64Bytes` $def.
    let instance = serde_json::to_value(&finding).expect("serialise");
    let data = &instance["raw"]["series"]["timestamps_ms"]["data"];
    assert!(
        data.is_string(),
        "raw.series.timestamps_ms.data must be a string (base64); got {data}"
    );
}

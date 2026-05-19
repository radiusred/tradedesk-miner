//! Phase 3 integration test — `miner scans` introspection (OP-07 / SC-2a).
//!
//! Spawns the `miner scans` subcommand via `assert_cmd`, asserts:
//! - Exit 0.
//! - Exactly one JSONL line for Phase 3 (one registered scan —
//!   `stats.autocorr.ljung_box@1`).
//! - The line validates against `schemas/scans-catalogue-v1.schema.json`
//!   (Open Question 8 resolution — the catalogue schema, NOT findings-v1).
//! - The line does NOT pass `schemas/findings-v1.schema.json` (negative
//!   assertion — confirms the catalogue lines are structurally distinct
//!   from Findings).
//! - The line carries the four required catalogue keys (`scan_id`,
//!   `version`, `params`, `finding_fields`) with the expected Phase 3
//!   values.

use std::path::PathBuf;

/// Path to the committed schemas, relative to this crate's manifest dir.
fn schemas_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

fn load_validator(filename: &str) -> jsonschema::Validator {
    let path = schemas_dir().join(filename);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read schema at {}: {}", path.display(), e));
    let json: serde_json::Value = serde_json::from_str(&text).expect("schema must be valid JSON");
    jsonschema::validator_for(&json).expect("schema must be valid JSON Schema")
}

#[test]
#[serial_test::serial]
fn scans_emits_one_line_per_registered_scan() {
    let mut cmd = assert_cmd::Command::cargo_bin("miner").expect("cargo_bin miner");
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MINER_CACHE_ROOT", "/tmp/c")
        .env("MINER_BAR_CACHE_ROOT", "/tmp/bc")
        .env("MINER_OUTPUT", "stdout")
        .arg("scans");
    let out = cmd.output().expect("spawn miner scans");
    let stdout = String::from_utf8(out.stdout).expect("stdout utf-8");
    let stderr = String::from_utf8(out.stderr).expect("stderr utf-8");
    assert_eq!(
        out.status.code(),
        Some(0),
        "miner scans must exit 0; stderr: {stderr}",
    );

    let lines: Vec<serde_json::Value> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("line is valid JSON"))
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "Phase 3 ships one registered scan; got {} lines",
        lines.len(),
    );

    let line = &lines[0];
    // Required keys per D3-20 + Plan 04-02 / D4-02 (`arity`).
    for key in ["scan_id", "version", "arity", "params", "finding_fields"] {
        assert!(
            line.get(key).is_some(),
            "catalogue line missing required key {key:?}: {line}",
        );
    }
    assert_eq!(line["scan_id"], "stats.autocorr.ljung_box");
    assert_eq!(line["version"], 1);
    // Plan 04-02 / D4-02: LjungBoxScan is single-leg.
    assert_eq!(line["arity"], "single");
    // finding_fields.effect_extra_keys is a non-empty array.
    let extra_keys = line["finding_fields"]["effect_extra_keys"]
        .as_array()
        .expect("effect_extra_keys is array");
    assert!(!extra_keys.is_empty(), "Phase 3 scan declares extra keys");
    // The four extra keys from D3-04.
    let mut got: Vec<&str> = extra_keys.iter().map(|v| v.as_str().unwrap()).collect();
    got.sort_unstable();
    assert_eq!(got, vec!["acf", "lags", "p_values", "q_stats"]);
    // The two raw.series keys from D3-04.
    let raw_keys = line["finding_fields"]["raw_series_keys"]
        .as_array()
        .expect("raw_series_keys is array");
    let mut got_raw: Vec<&str> = raw_keys.iter().map(|v| v.as_str().unwrap()).collect();
    got_raw.sort_unstable();
    assert_eq!(got_raw, vec!["returns", "timestamps_ms"]);

    // Positive: validates against schemas/scans-catalogue-v1.schema.json.
    let catalogue_validator = load_validator("scans-catalogue-v1.schema.json");
    let errors_catalogue: Vec<_> = catalogue_validator.iter_errors(line).collect();
    assert!(
        errors_catalogue.is_empty(),
        "catalogue line failed scans-catalogue-v1 schema: {errors_catalogue:?}\nline: {line}",
    );

    // Negative: must NOT validate as a Finding (the catalogue lines are
    // structurally distinct from envelopes per Pitfall 7 / Open Question 8).
    let findings_validator = load_validator("findings-v1.schema.json");
    assert!(
        !findings_validator.is_valid(line),
        "catalogue line MUST NOT pass findings-v1.schema.json (it has no `kind` field)",
    );
}

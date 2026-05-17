//! Plan 07 Task 2 — end-to-end CLI integration tests (FOUND-02, OUT-01, OUT-02, OUT-03).
//!
//! Spawns the actual `miner` binary (built by `cargo test`) via
//! `assert_cmd::Command::cargo_bin("miner")` and asserts:
//!
//! 1. **emit-fixture happy path**: exit 0, exactly 2 newline-terminated JSON lines on
//!    stdout (first `kind: run_start`, second `kind: run_end`), shared `run_id`
//!    across both envelopes.
//! 2. **stdout/stderr split (T-01-03 regression gate)**: tracing log lines land on
//!    stderr; stdout contains zero tracing output.
//! 3. **Schema validation (FOUND-03, T-01-02 regression gate)**: each stdout line
//!    validates against the committed `schemas/findings-v1.schema.json`.
//! 4. **Preflight missing-config failure (D-06, D-07)**: with cleared env + empty
//!    CWD + no `--config`, exit 1, stdout empty, stderr has exactly one `WireError`
//!    JSON line with `code: "missing_required_config"`.
//! 5. **Preflight invalid-TOML failure (Plan 05 figment-error classifier gate)**:
//!    with `cache_root = 42` (integer instead of path), exit 1, stdout empty,
//!    stderr `WireError` has `code: "invalid_config"` (NOT `missing_required_config`).
//! 6. **OUT-03 full closure — twice-run byte-identity (envelope determinism)**: run
//!    emit-fixture TWICE with identical env, mask the four KNOWN-volatile fields
//!    (`run_id`, `started_at_utc`, `ended_at_utc`, `wall_clock_ms`) to sentinel
//!    values, re-serialise both, assert the masked bytes are IDENTICAL across runs.
//!    Proves every other field — key order, scalar values, nested map ordering inside
//!    `summary` / `per_scan` / `data_slice`, the `request` echo — is byte-stable.
//!
//! The 4 known-volatile fields are the ONLY deliberately non-deterministic envelope
//! fields by design (D-10 fresh ULID per run, D-11 wall-clock timestamps). Any other
//! cross-run drift means the `BTreeMap` discipline somewhere collapsed (`HashMap` snuck
//! in, `serde_json/preserve_order` got enabled, schemars insertion order regressed,
//! etc.) and Phase 1 is broken.

use std::path::PathBuf;
use std::process::ExitStatus;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Path to the committed envelope schema, relative to this crate's manifest dir.
fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/findings-v1.schema.json")
}

/// Load the committed schema as a `jsonschema::Validator`.
fn load_validator() -> jsonschema::Validator {
    let path = schema_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read schema at {}: {}", path.display(), e));
    let json: serde_json::Value =
        serde_json::from_str(&text).expect("schemas/findings-v1.schema.json must be valid JSON");
    jsonschema::validator_for(&json).expect("schema must be valid JSON Schema 2020-12")
}

/// Spawn `miner emit-fixture` with the three required env vars set and capture
/// (stdout, stderr, exit status). Returns the captured streams as `String`s for
/// downstream assertions. Used by every happy-path test.
fn run_emit_fixture_happy() -> (String, String, ExitStatus) {
    let mut cmd = assert_cmd::Command::cargo_bin("miner").expect("cargo_bin miner");
    cmd.env_clear()
        // PATH is needed for binary lookup on some runners; copy it from the
        // surrounding env. Everything else is cleared so MINER_* from the
        // developer's shell does not leak in.
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MINER_CACHE_ROOT", "/tmp/cache")
        .env("MINER_BAR_CACHE_ROOT", "/tmp/bar")
        .env("MINER_OUTPUT", "stdout")
        .arg("emit-fixture");
    let out = cmd.output().expect("spawn miner emit-fixture");
    (
        String::from_utf8(out.stdout).expect("stdout must be valid utf-8"),
        String::from_utf8(out.stderr).expect("stderr must be valid utf-8"),
        out.status,
    )
}

/// Parse a stdout buffer of `n` newline-terminated JSON lines into a `Vec<Value>`.
fn parse_stdout_lines(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| {
                panic!("emit-fixture stdout line is not valid JSON: {e}\n  line: {line}")
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Test 1 — emit_fixture_writes_two_jsonl_lines_to_stdout (FOUND-02, OUT-01)
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn emit_fixture_writes_two_jsonl_lines_to_stdout() {
    let (stdout, _stderr, status) = run_emit_fixture_happy();

    assert_eq!(status.code(), Some(0), "exit code must be 0");

    // Exactly 2 newline-terminated lines on stdout.
    let newlines = stdout.bytes().filter(|&b| b == b'\n').count();
    assert_eq!(
        newlines, 2,
        "expected exactly 2 newlines on stdout; got {newlines}\nstdout: {stdout:?}"
    );

    let lines = parse_stdout_lines(&stdout);
    assert_eq!(lines.len(), 2, "expected 2 JSON lines on stdout");

    assert_eq!(
        lines[0]["kind"], "run_start",
        "first envelope must be kind=run_start; got {}",
        lines[0]
    );
    assert_eq!(
        lines[1]["kind"], "run_end",
        "second envelope must be kind=run_end; got {}",
        lines[1]
    );
}

// ---------------------------------------------------------------------------
// Test 2 — emit_fixture_writes_tracing_to_stderr_not_stdout (T-01-03)
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn emit_fixture_writes_tracing_to_stderr_not_stdout() {
    let (stdout, stderr, status) = run_emit_fixture_happy();
    assert_eq!(status.code(), Some(0));

    // The CLI's emit_fixture() calls `tracing::info!("emitting fixture")`. With the
    // tracing-subscriber configured to write to stderr (Plan 05), the literal
    // substring must appear on stderr and MUST NOT appear on stdout.
    assert!(
        stderr.contains("emitting fixture"),
        "stderr must contain the tracing::info! line 'emitting fixture'; got: {stderr:?}"
    );
    assert!(
        !stdout.contains("emitting fixture"),
        "stdout MUST NOT contain tracing output (T-01-03 stdout discipline); got: {stdout:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — emit_fixture_run_ids_match_across_envelopes
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn emit_fixture_run_ids_match_across_envelopes() {
    let (stdout, _stderr, status) = run_emit_fixture_happy();
    assert_eq!(status.code(), Some(0));

    let lines = parse_stdout_lines(&stdout);
    assert_eq!(lines.len(), 2);

    let a = lines[0]["run_id"].as_str().expect("run_start has run_id");
    let b = lines[1]["run_id"].as_str().expect("run_end has run_id");
    assert_eq!(
        a, b,
        "run_id must be shared across RunStart and RunEnd; got {a:?} vs {b:?}"
    );
    assert_eq!(a.len(), 26, "run_id must be a 26-char ULID; got {a:?}");
}

// ---------------------------------------------------------------------------
// Test 4 — emit_fixture_validates_against_committed_schema (FOUND-03, T-01-02)
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn emit_fixture_validates_against_committed_schema() {
    let (stdout, _stderr, status) = run_emit_fixture_happy();
    assert_eq!(status.code(), Some(0));

    let validator = load_validator();
    let lines = parse_stdout_lines(&stdout);
    assert_eq!(lines.len(), 2);

    for (i, line) in lines.iter().enumerate() {
        let errors: Vec<String> = validator.iter_errors(line).map(|e| format!("  - {e}")).collect();
        assert!(
            errors.is_empty() && validator.is_valid(line),
            "stdout line {i} (kind={}) failed schema validation:\nline: {}\nerrors:\n{}",
            line["kind"],
            serde_json::to_string_pretty(line).unwrap(),
            errors.join("\n")
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5 — preflight_missing_config_emits_wireerror_to_stderr_exit_1 (D-06, D-07)
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn preflight_missing_config_emits_wireerror_to_stderr_exit_1() {
    let tmp = tempfile::TempDir::new().expect("tempdir");

    let mut cmd = assert_cmd::Command::cargo_bin("miner").expect("cargo_bin miner");
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        // Working dir is an empty tempdir so the CLI's CWD fallback to `./miner.toml`
        // finds nothing and falls through to the figment extract-without-cache_root path.
        .current_dir(tmp.path())
        // XDG_CONFIG_HOME pointed at the same tempdir prevents the directories crate from
        // finding a real user-level miner.toml on the developer's machine.
        .env("XDG_CONFIG_HOME", tmp.path())
        .env("HOME", tmp.path())
        .arg("emit-fixture");
    let out = cmd.output().expect("spawn miner emit-fixture");

    assert_eq!(
        out.status.code(),
        Some(1),
        "preflight failure must exit 1; stderr was: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "stdout must be empty on preflight failure (T-01-03); got: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );

    // First non-empty stderr line is the structured WireError JSON.
    let stderr = String::from_utf8(out.stderr).expect("stderr utf-8");
    let wire_line = stderr
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON-object line on stderr; got:\n{stderr}"));
    let wire: serde_json::Value =
        serde_json::from_str(wire_line).expect("stderr WireError must be valid JSON");
    assert_eq!(
        wire["code"], "missing_required_config",
        "PreflightCode::MissingRequiredConfig must serialise as 'missing_required_config'; got: {wire}"
    );
    assert!(
        wire["message"].is_string(),
        "WireError.message must be a string; got: {wire}"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — preflight_invalid_toml_emits_invalid_config_code (Plan 05 mapper gate)
//
// BLOCKER regression gate for classify_figment_error: an InvalidType figment error
// MUST map to "invalid_config", NOT "missing_required_config".
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn preflight_invalid_toml_emits_invalid_config_code() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let bad_toml = tmp.path().join("bad.toml");
    // cache_root = integer instead of string path → figment Kind::InvalidType.
    std::fs::write(
        &bad_toml,
        "cache_root = 42\nbar_cache_root = \"/b\"\noutput = \"stdout\"\n",
    )
    .expect("write bad toml");

    let mut cmd = assert_cmd::Command::cargo_bin("miner").expect("cargo_bin miner");
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", tmp.path())
        .env("HOME", tmp.path())
        .args([
            "--config",
            bad_toml.to_str().expect("toml path utf-8"),
            "emit-fixture",
        ]);
    let out = cmd.output().expect("spawn miner emit-fixture --config bad.toml");

    assert_eq!(
        out.status.code(),
        Some(1),
        "invalid-toml preflight must exit 1; stderr was: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "stdout must be empty on preflight failure (T-01-03); got: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );

    let stderr = String::from_utf8(out.stderr).expect("stderr utf-8");
    let wire_line = stderr
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON-object line on stderr; got:\n{stderr}"));
    let wire: serde_json::Value =
        serde_json::from_str(wire_line).expect("stderr WireError must be valid JSON");
    assert_eq!(
        wire["code"], "invalid_config",
        "InvalidType figment error MUST map to 'invalid_config' (Plan 05 mapper regression gate); got: {wire}"
    );
    assert_ne!(
        wire["code"], "missing_required_config",
        "InvalidType must NOT be mis-classified as missing_required_config (Plan 05 mapper regression gate)"
    );
}

// ---------------------------------------------------------------------------
// Test 7 — emit_fixture_byte_identical_when_volatile_fields_masked (OUT-03 closure)
//
// Run emit-fixture TWICE with identical env, mask the four KNOWN-volatile fields
// (run_id, started_at_utc, ended_at_utc, wall_clock_ms), assert the masked bytes
// match across runs. This is the FULL closure of OUT-03 for Phase 1's envelope
// determinism contract — there is NO 'partial deferral'.
// ---------------------------------------------------------------------------

/// Recursively mask the four known-volatile envelope fields. Operates on the
/// parsed `serde_json::Value` tree so masking is robust against future schema
/// shape changes (e.g., if a future `request` echo wraps a nested ULID/timestamp).
fn mask_volatile_fields(v: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for key in ["run_id", "started_at_utc", "ended_at_utc"] {
            if map.contains_key(key) {
                map.insert(
                    key.to_string(),
                    serde_json::Value::String(format!("<masked_{key}>")),
                );
            }
        }
        if map.contains_key("wall_clock_ms") {
            map.insert(
                "wall_clock_ms".to_string(),
                serde_json::Value::from(0i64),
            );
        }
        for (_, child) in map.iter_mut() {
            mask_volatile_fields(child);
        }
    } else if let serde_json::Value::Array(arr) = v {
        for child in arr.iter_mut() {
            mask_volatile_fields(child);
        }
    }
}

/// Parse `raw` as `\n`-delimited JSON envelopes, mask the volatile fields in each,
/// and re-serialise with `serde_json::to_string` (NOT `to_string_pretty` — compact
/// form is what the byte-equality assertion is over). Returns one string per line.
fn mask_emit_fixture_stdout(raw: &str) -> Vec<String> {
    raw.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut v: serde_json::Value = serde_json::from_str(line)
                .expect("emit-fixture stdout line is valid JSON");
            mask_volatile_fields(&mut v);
            serde_json::to_string(&v).expect("masked JSON re-serialises")
        })
        .collect()
}

#[test]
#[serial_test::serial]
fn emit_fixture_byte_identical_when_volatile_fields_masked() {
    let (out1, _, status1) = run_emit_fixture_happy();
    assert_eq!(status1.code(), Some(0), "run 1 must exit 0");
    let (out2, _, status2) = run_emit_fixture_happy();
    assert_eq!(status2.code(), Some(0), "run 2 must exit 0");

    let masked1 = mask_emit_fixture_stdout(&out1);
    let masked2 = mask_emit_fixture_stdout(&out2);

    assert_eq!(masked1.len(), 2, "expected 2 envelope lines per run");
    assert_eq!(masked2.len(), 2, "expected 2 envelope lines per run (run 2)");
    assert_eq!(
        masked1, masked2,
        "OUT-03 closure: masked envelopes from two emit-fixture runs differ.\n\
         Run 1:\n{}\n\
         Run 2:\n{}",
        masked1.join("\n"),
        masked2.join("\n"),
    );
}

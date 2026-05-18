//! Phase 3 integration test — `miner scan` subcommand happy + sad paths (OP-01 / OP-05 / OP-08 / D3-24).
//!
//! Spawns the actual `miner` binary (built by `cargo test`) via
//! `assert_cmd::Command::cargo_bin("miner")` and asserts the documented
//! contract for each of the five named scenarios from VALIDATION.md's
//! Per-Task Verification Map:
//!
//! - `scan_emits_run_start_result_run_end` (OP-01 / SC-1)
//! - `unknown_scan_emits_wireerror_exit_1` (OP-08 / SC-2b)
//! - `invalid_params_emits_wireerror_exit_1` (OP-08 / SC-2c)
//! - `dry_run_emits_dry_run_finding_only` (OP-05 / SC-4)
//! - `exit_code_routing_zero_one_two` (D3-24)

#![allow(clippy::cast_possible_wrap)]

use std::path::Path;
use std::process::ExitStatus;

use chrono::NaiveDate;
use miner_core::Side;
use tempfile::TempDir;

mod fixtures;
use fixtures::SyntheticCache;

/// Path to the committed envelope schema, relative to this crate's manifest dir.
fn schema_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/findings-v1.schema.json")
}

/// Spawn `miner scan ...` with the four required env vars + the supplied
/// additional CLI args. Returns (stdout, stderr, status).
fn run_miner(cache: &SyntheticCache, extra_args: &[&str]) -> (String, String, ExitStatus) {
    let mut cmd = assert_cmd::Command::cargo_bin("miner").expect("cargo_bin miner");
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MINER_CACHE_ROOT", cache.cache_root())
        .env("MINER_BAR_CACHE_ROOT", cache.bar_cache_root())
        .env("MINER_OUTPUT", "stdout")
        .current_dir(cache.tempdir().path())
        .arg("scan");
    cmd.args(extra_args);
    let out = cmd.output().expect("spawn miner");
    (
        String::from_utf8(out.stdout).expect("stdout utf-8"),
        String::from_utf8(out.stderr).expect("stderr utf-8"),
        out.status,
    )
}

/// Parse a stdout buffer of newline-terminated JSON lines into a Vec<Value>.
fn parse_stdout_lines(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("stdout line not JSON: {e}; line: {line}"))
        })
        .collect()
}

fn happy_path_cache() -> SyntheticCache {
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).unwrap();
    SyntheticCache::new().with_deterministic_day("EURUSD", Side::Bid, day, 0xDEAD_BEEF)
}

// ---------------------------------------------------------------------------
// Test 1 — happy path: kinds [run_start, result, run_end], exit 0
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn scan_emits_run_start_result_run_end() {
    let cache = happy_path_cache();
    let (stdout, stderr, status) = run_miner(
        &cache,
        &[
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD",
            "--side",
            "bid",
            "--timeframe",
            "15m",
            "--window",
            "2024-06-12:2024-06-13",
        ],
    );
    assert_eq!(
        status.code(),
        Some(0),
        "exit 0 required; stderr was: {stderr}\nstdout was: {stdout}"
    );
    let lines = parse_stdout_lines(&stdout);
    assert_eq!(
        lines.len(),
        3,
        "expected exactly 3 envelopes [run_start, result, run_end]; got {}: {:?}",
        lines.len(),
        lines.iter().map(|l| l["kind"].clone()).collect::<Vec<_>>(),
    );
    assert_eq!(lines[0]["kind"], "run_start");
    assert_eq!(lines[1]["kind"], "result");
    assert_eq!(lines[2]["kind"], "run_end");

    // OP-08 — resolved params echoed into RunStart.request.
    assert!(
        lines[0]["request"].get("resolved_params").is_some(),
        "RunStart.request must carry resolved_params",
    );
}

// ---------------------------------------------------------------------------
// Test 2 — unknown scan → exit 1, stdout empty, stderr WireError
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn unknown_scan_emits_wireerror_exit_1() {
    let cache = happy_path_cache();
    let (stdout, stderr, status) = run_miner(
        &cache,
        &[
            "nonexistent.scan@99",
            "--instrument",
            "EURUSD",
            "--timeframe",
            "15m",
            "--window",
            "2024-06-12:2024-06-13",
        ],
    );
    assert_eq!(
        status.code(),
        Some(1),
        "exit 1 required; stdout: {stdout:?}; stderr: {stderr:?}",
    );
    // Stdout discipline: empty (T-01-03 mitigation — preflight rejection
    // writes nothing to stdout).
    assert!(
        stdout.is_empty(),
        "stdout must be empty on preflight rejection; got: {stdout:?}",
    );
    // Stderr carries the WireError JSON line.
    let wire_line = stderr
        .lines()
        .find(|l| l.starts_with('{'))
        .unwrap_or_else(|| panic!("stderr WireError line missing; got: {stderr}"));
    let wire: serde_json::Value = serde_json::from_str(wire_line).expect("WireError parses");
    assert_eq!(
        wire["code"], "unknown_scan",
        "unknown scan must classify as unknown_scan; got: {wire}",
    );
}

// ---------------------------------------------------------------------------
// Test 3 — invalid params → exit 1, stderr WireError(invalid_parameter)
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn invalid_params_emits_wireerror_exit_1() {
    // Supply --side with an invalid value so preflight rejects with
    // invalid_parameter (the malformed-KEY=VAL params path is preempted by
    // clap because clap accepts the string verbatim before preflight runs;
    // an invalid --side value tests the same boundary code path).
    let cache = happy_path_cache();
    let (stdout, stderr, status) = run_miner(
        &cache,
        &[
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD",
            "--side",
            "middle",
            "--timeframe",
            "15m",
            "--window",
            "2024-06-12:2024-06-13",
        ],
    );
    assert_eq!(
        status.code(),
        Some(1),
        "exit 1 required; stdout: {stdout:?}; stderr: {stderr:?}",
    );
    assert!(
        stdout.is_empty(),
        "stdout must be empty on preflight rejection; got: {stdout:?}",
    );
    let wire_line = stderr
        .lines()
        .find(|l| l.starts_with('{'))
        .unwrap_or_else(|| panic!("stderr WireError line missing; got: {stderr}"));
    let wire: serde_json::Value = serde_json::from_str(wire_line).expect("WireError parses");
    assert_eq!(
        wire["code"], "invalid_parameter",
        "invalid --side must classify as invalid_parameter; got: {wire}",
    );
}

// ---------------------------------------------------------------------------
// Test 4 — --dry-run emits exactly [run_start, dry_run, run_end]
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn dry_run_emits_dry_run_finding_only() {
    let cache = happy_path_cache();
    let (stdout, stderr, status) = run_miner(
        &cache,
        &[
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD",
            "--timeframe",
            "15m",
            "--window",
            "2024-06-12:2024-06-13",
            "--dry-run",
        ],
    );
    assert_eq!(status.code(), Some(0), "dry-run exit 0; stderr: {stderr}",);
    let lines = parse_stdout_lines(&stdout);
    assert_eq!(
        lines.len(),
        3,
        "expected 3 envelopes [run_start, dry_run, run_end]; got: {:?}",
        lines.iter().map(|l| l["kind"].clone()).collect::<Vec<_>>(),
    );
    assert_eq!(lines[0]["kind"], "run_start");
    assert_eq!(lines[1]["kind"], "dry_run");
    assert_eq!(lines[2]["kind"], "run_end");
    // No Result envelope (Pitfall 3).
    assert!(
        lines.iter().all(|l| l["kind"] != "result"),
        "no Result must be emitted in a dry run",
    );
}

// ---------------------------------------------------------------------------
// Test 5 — Four-tier exit code routing (0 / 1 / 2 per D3-24)
//
// Sub-scenarios share the same SyntheticCache, run in series:
//   (a) happy path → exit 0;
//   (b) preflight reject (unknown scan) → exit 1;
//   (c) mid-stream scan_error injected via lags=100000 (n ~95 at 15m -> kernel
//       rejects with ScanError::Kernel; engine emits Finding::ScanError, exit 2).
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn exit_code_routing_zero_one_two() {
    let cache = happy_path_cache();

    // (a) Happy path → exit 0.
    let (stdout_a, _stderr_a, status_a) = run_miner(
        &cache,
        &[
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD",
            "--timeframe",
            "15m",
            "--window",
            "2024-06-12:2024-06-13",
        ],
    );
    assert_eq!(
        status_a.code(),
        Some(0),
        "happy path exit 0: stdout: {stdout_a}"
    );

    // (b) Unknown scan → exit 1.
    let (_, _, status_b) = run_miner(
        &cache,
        &[
            "no.such.scan@1",
            "--instrument",
            "EURUSD",
            "--timeframe",
            "15m",
            "--window",
            "2024-06-12:2024-06-13",
        ],
    );
    assert_eq!(status_b.code(), Some(1), "unknown scan exit 1");

    // (c) Mid-run ScanError → exit 2. Force `lags >= n` to trigger
    // ScanError::Kernel inside LjungBoxScan::run. One 15m day yields ~95
    // bars / ~94 returns; lags=999 > 94 so the kernel rejects.
    let (stdout_c, stderr_c, status_c) = run_miner(
        &cache,
        &[
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD",
            "--timeframe",
            "15m",
            "--window",
            "2024-06-12:2024-06-13",
            "--params",
            "lags=999",
        ],
    );
    assert_eq!(
        status_c.code(),
        Some(2),
        "mid-stream ScanError exit 2; stdout: {stdout_c}; stderr: {stderr_c}",
    );
    let lines_c = parse_stdout_lines(&stdout_c);
    // The stream must contain a scan_error envelope.
    assert!(
        lines_c.iter().any(|l| l["kind"] == "scan_error"),
        "exit 2 path must emit at least one Finding::ScanError; got: {:?}",
        lines_c
            .iter()
            .map(|l| l["kind"].clone())
            .collect::<Vec<_>>(),
    );
}

#[allow(dead_code)]
fn _ensure_schema_path(_p: &Path) {
    // Plan 06 keeps the schema path resolver around for future use.
    let _ = schema_path();
    let _ = TempDir::new();
}

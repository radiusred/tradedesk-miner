//! Phase 5 integration test — `miner sweep` subcommand smoke + dry-run paths
//! (OP-04 / D5-04 / Plan 05-05 Task 3).
//!
//! Spawns the actual `miner` binary built by `cargo test` via
//! `assert_cmd::Command::cargo_bin("miner")` and asserts:
//!
//! 1. `miner sweep <manifest>` against a synthetic Dukascopy cache produces
//!    the documented envelope sequence — `run_start` → N×`result` →
//!    `sweep_summary` → `run_end`. Exit 0.
//! 2. `miner sweep <manifest> --dry-run` produces `run_start` → `dry_run`
//!    (with `planned_job_count`) → `run_end`. NO `result` / `sweep_summary`
//!    envelopes. Exit 0.

#![allow(
    clippy::cast_possible_wrap,
    clippy::doc_markdown,
    reason = "test docstrings are descriptive prose, not API identifiers"
)]

use chrono::NaiveDate;
use miner_core::Side;

mod fixtures;
use fixtures::SyntheticCache;

/// Path to the committed sweep-manifest schema, relative to this crate's
/// manifest dir.
#[allow(dead_code)]
fn sweep_manifest_schema_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/sweep-manifest-v1.schema.json")
}

/// Build a synthetic Dukascopy cache with two instruments × one day each.
/// Both instruments use the same date so the bar cache builds happen against
/// non-empty source files.
fn happy_path_two_instrument_cache() -> SyntheticCache {
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).unwrap();
    SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 0xDEAD_BEEF)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 0xCAFE_F00D)
}

/// Build a synthetic two-job manifest in the supplied tempdir's path.
/// Returns the manifest path.
fn write_smoke_manifest(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("manifest.toml");
    std::fs::write(
        &path,
        r#"
[sweep]
seed = 3735928559
max_jobs = 100

[fdr]
family = "scan_id"
alpha = 0.05

[[jobs]]
scan = "stats.autocorr.ljung_box@1"
instruments = ["EURUSD:bid", "GBPUSD:bid"]
timeframes = ["15m"]
windows = ["2024-06-12:2024-06-13"]
params = { lags = [5] }
"#,
    )
    .expect("write manifest");
    path
}

/// Spawn `miner sweep <manifest>` with the four required env vars and
/// optional extra args. Returns (stdout, stderr, status).
fn run_miner_sweep(
    cache: &SyntheticCache,
    manifest_path: &std::path::Path,
    extra_args: &[&str],
) -> (String, String, std::process::ExitStatus) {
    let mut cmd = assert_cmd::Command::cargo_bin("miner").expect("cargo_bin miner");
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MINER_CACHE_ROOT", cache.cache_root())
        .env("MINER_BAR_CACHE_ROOT", cache.bar_cache_root())
        .env("MINER_OUTPUT", "stdout")
        .current_dir(cache.tempdir().path())
        .arg("sweep")
        .arg(manifest_path);
    cmd.args(extra_args);
    let out = cmd.output().expect("spawn miner");
    (
        String::from_utf8(out.stdout).expect("stdout utf-8"),
        String::from_utf8(out.stderr).expect("stderr utf-8"),
        out.status,
    )
}

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

// ---------------------------------------------------------------------------
// Test 1 — happy path: kinds [run_start, result, result, sweep_summary, run_end]
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn sweep_subcommand_smoke() {
    let cache = happy_path_two_instrument_cache();
    let manifest_path = write_smoke_manifest(cache.tempdir().path());

    let (stdout, stderr, status) = run_miner_sweep(&cache, &manifest_path, &[]);
    assert_eq!(
        status.code(),
        Some(0),
        "exit 0 required; stderr: {stderr}\nstdout: {stdout}"
    );
    let lines = parse_stdout_lines(&stdout);

    // Exactly: 1 run_start + 2 result + 1 sweep_summary + 1 run_end = 5.
    let kinds: Vec<String> = lines
        .iter()
        .map(|v| v["kind"].as_str().unwrap_or("?").to_string())
        .collect();
    let run_start_count = kinds.iter().filter(|k| *k == "run_start").count();
    let result_count = kinds.iter().filter(|k| *k == "result").count();
    let sweep_summary_count = kinds.iter().filter(|k| *k == "sweep_summary").count();
    let run_end_count = kinds.iter().filter(|k| *k == "run_end").count();
    assert_eq!(run_start_count, 1, "exactly one RunStart; kinds: {kinds:?}");
    assert_eq!(
        result_count, 2,
        "exactly two Result envelopes (one per instrument); kinds: {kinds:?}"
    );
    assert_eq!(
        sweep_summary_count, 1,
        "exactly one SweepSummary; kinds: {kinds:?}"
    );
    assert_eq!(run_end_count, 1, "exactly one RunEnd; kinds: {kinds:?}");

    // Ordering: run_start MUST be first, run_end MUST be last, sweep_summary
    // MUST appear AFTER the last result and BEFORE run_end.
    assert_eq!(kinds.first().map(String::as_str), Some("run_start"));
    assert_eq!(kinds.last().map(String::as_str), Some("run_end"));
    let last_result_idx = kinds.iter().rposition(|k| k == "result");
    let sweep_summary_idx = kinds.iter().position(|k| k == "sweep_summary");
    let run_end_idx = kinds.iter().position(|k| k == "run_end");
    assert!(
        last_result_idx.unwrap() < sweep_summary_idx.unwrap(),
        "SweepSummary must follow the last Result"
    );
    assert!(
        sweep_summary_idx.unwrap() < run_end_idx.unwrap(),
        "SweepSummary must precede RunEnd"
    );

    // SweepSummary contains a non-empty fdr_by_family map (the default
    // [fdr].family = "scan_id" produces one family for the one scan_id used).
    let sweep_summary = &lines[sweep_summary_idx.unwrap()];
    assert!(
        sweep_summary["fdr_by_family"].is_object(),
        "fdr_by_family must be an object; got: {sweep_summary}"
    );
    let fdr_obj = sweep_summary["fdr_by_family"].as_object().unwrap();
    assert_eq!(
        fdr_obj.len(),
        1,
        "expected one FDR family (scan_id scope); got: {fdr_obj:?}"
    );
    assert!(
        fdr_obj.contains_key("stats.autocorr.ljung_box@1"),
        "missing family key for stats.autocorr.ljung_box@1; got: {fdr_obj:?}"
    );

    // SweepTotals — jobs_run=2, results_emitted=2.
    let totals = &sweep_summary["totals"];
    assert_eq!(totals["jobs_run"], 2, "jobs_run; totals: {totals}");
    assert_eq!(
        totals["results_emitted"], 2,
        "results_emitted; totals: {totals}"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — --dry-run emits [run_start, dry_run, run_end]; no result or
// sweep_summary envelopes; dry_run carries planned_job_count == 2.
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn sweep_subcommand_smoke_dry_run() {
    let cache = happy_path_two_instrument_cache();
    let manifest_path = write_smoke_manifest(cache.tempdir().path());

    let (stdout, stderr, status) = run_miner_sweep(&cache, &manifest_path, &["--dry-run"]);
    assert_eq!(
        status.code(),
        Some(0),
        "dry-run exit 0 required; stderr: {stderr}\nstdout: {stdout}"
    );
    let lines = parse_stdout_lines(&stdout);

    let kinds: Vec<String> = lines
        .iter()
        .map(|v| v["kind"].as_str().unwrap_or("?").to_string())
        .collect();
    assert_eq!(
        kinds,
        vec!["run_start", "dry_run", "run_end"],
        "expected exactly [run_start, dry_run, run_end]; got: {kinds:?}"
    );

    // NO result / sweep_summary envelopes.
    assert!(
        !kinds.iter().any(|k| k == "result"),
        "no Result envelope on dry-run"
    );
    assert!(
        !kinds.iter().any(|k| k == "sweep_summary"),
        "no SweepSummary on dry-run"
    );

    // DryRunFinding.planned_job_count populated with the cartesian-expanded
    // count = 2 (1 scan × 2 instruments × 1 timeframe × 1 window × 1 param).
    let dry_run = &lines[1];
    assert_eq!(
        dry_run["planned_job_count"], 2,
        "planned_job_count must equal cartesian-expanded count; got: {dry_run}"
    );
}

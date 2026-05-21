//! Phase 5 integration test — SIGINT mid-sweep preserves already-streamed
//! findings + suppresses SweepSummary + exits 130 (OP-04 / D5-04 / D3-22 /
//! Plan 05-05 Task 3).
//!
//! `#![cfg(unix)]` — unix-only because `nix::sys::signal::kill` is a
//! POSIX-specific API.
//!
//! Structurally mirrors `sigint_preserves_stream.rs` (the single-shot
//! `miner scan` SIGINT regression). Differences:
//! - Spawns `miner sweep <manifest> --sleep-after-first-finding-ms 5000`
//!   (the manifest TOML carries two jobs so the first Result is emitted
//!   before SIGINT lands, then the per-job sleep loop pauses for the race
//!   window).
//! - Asserts the SweepSummary envelope is NOT in the captured stdout
//!   (HYG-05 / D5-04 — SweepSummary is suppressed when cancel is set).
//!
//! ## Build prerequisite
//!
//! `--sleep-after-first-finding-ms` is gated on `cfg(any(test, feature =
//! "test-internal"))`. The integration test builds the `miner` binary with
//! `--features test-internal` before spawning it.

#![cfg(unix)]
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    reason = "test docstrings are descriptive prose, not API identifiers"
)]

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use chrono::NaiveDate;
use miner_core::Side;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

mod fixtures;
use fixtures::SyntheticCache;

/// Locate the workspace-root `target/debug/miner` produced by
/// `cargo build -p miner-cli --features test-internal --bin miner`.
fn target_miner_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("crate parent")
        .parent()
        .expect("workspace root")
        .join("target")
        .join("debug")
        .join("miner")
}

/// Rebuild the `miner` binary with `--features test-internal` so the
/// cfg-gated `--sleep-after-first-finding-ms` CLI flag is reachable.
fn build_with_test_internal_feature() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .parent()
        .expect("crate parent")
        .parent()
        .expect("workspace root");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "-p",
            "miner-cli",
            "--features",
            "test-internal",
            "--bin",
            "miner",
        ])
        .current_dir(workspace_root)
        .status()
        .expect("cargo build invocation");
    assert!(
        status.success(),
        "cargo build -p miner-cli --features test-internal --bin miner failed",
    );
}

/// Write a synthetic two-job sweep manifest with the same instrument layout
/// the SIGINT test exercises against the SyntheticCache fixture.
fn write_sigint_sweep_manifest(dir: &std::path::Path) -> PathBuf {
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

#[test]
#[serial_test::file_serial(miner_bin_test_internal)]
fn sigint_mid_sweep_preserves_streamed_findings() {
    // Step 0 — rebuild with --features test-internal so the cfg-gated CLI
    // flag is reachable end-to-end.
    build_with_test_internal_feature();
    let bin = target_miner_path();
    assert!(
        bin.exists(),
        "miner binary missing at {} after --features test-internal build",
        bin.display(),
    );

    // Step 1 — build a synthetic cache with two instruments. Both jobs read
    // the same day so the bar-cache builds are quick.
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).unwrap();
    let cache = SyntheticCache::new()
        .with_deterministic_day("EURUSD", Side::Bid, day, 0xDEAD_BEEF)
        .with_deterministic_day("GBPUSD", Side::Bid, day, 0xCAFE_F00D);
    let manifest_path = write_sigint_sweep_manifest(cache.tempdir().path());

    // Step 2 — spawn `miner sweep ... --sleep-after-first-finding-ms 5000`.
    // The 5s sleep lives INSIDE the LjungBox kernel after the first Result;
    // we deliver SIGINT during that pause so cancel observable BEFORE the
    // sweep reaches its end-of-sweep BH-FDR + SweepSummary stage.
    let mut child = Command::new(&bin)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MINER_CACHE_ROOT", cache.cache_root())
        .env("MINER_BAR_CACHE_ROOT", cache.bar_cache_root())
        .env("MINER_OUTPUT", "stdout")
        .current_dir(cache.tempdir().path())
        .args([
            "sweep",
            manifest_path.to_str().expect("manifest path utf-8"),
            "--sleep-after-first-finding-ms",
            "5000",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn miner");

    let child_pid = child.id();

    // Step 3 — read stdout line-by-line until we see the first Result
    // envelope. The sweep emits one RunStart immediately, then per-job
    // Results in order; the kernel's cancel-aware sleep loop kicks in
    // after each per-job Result, giving us a 5s window to deliver SIGINT.
    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);
    let mut buf = String::new();
    let mut saw_result = false;
    let mut captured = String::new();
    while reader.read_line(&mut buf).expect("read child stdout line") > 0 {
        captured.push_str(&buf);
        let parsed: serde_json::Value = serde_json::from_str(buf.trim())
            .unwrap_or_else(|e| panic!("child stdout line not JSON: {e}; line: {buf}"));
        if parsed["kind"] == "result" {
            saw_result = true;
            buf.clear();
            break;
        }
        buf.clear();
    }
    assert!(
        saw_result,
        "child must emit at least one Result before SIGINT"
    );

    // Step 4 — deliver SIGINT to the child.
    kill(Pid::from_raw(child_pid as i32), Signal::SIGINT).expect("kill SIGINT");

    // Step 5 — wait for the child to exit; assert exit code 130 per D3-24.
    let status = child.wait().expect("child wait");
    let code = status.code();
    assert_eq!(
        code,
        Some(130),
        "SIGINT must yield exit 130; got {code:?}; captured stdout so far:\n{captured}",
    );

    // Step 6 — drain remaining stdout; assert RunStart + at least one Result
    // persisted, NO SweepSummary envelope.
    let mut remaining = String::new();
    reader
        .into_inner()
        .read_to_string(&mut remaining)
        .expect("drain stdout");
    captured.push_str(&remaining);

    let lines: Vec<serde_json::Value> = captured
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .unwrap_or_else(|e| panic!("captured line not JSON: {e}; line: {l}"))
        })
        .collect();
    let kinds: Vec<&str> = lines
        .iter()
        .map(|v| v["kind"].as_str().unwrap_or("?"))
        .collect();
    assert!(
        kinds.iter().any(|k| *k == "run_start"),
        "RunStart must persist after SIGINT; kinds: {kinds:?}",
    );
    assert!(
        kinds.iter().any(|k| *k == "result"),
        "the first Result must persist after SIGINT (per-envelope flush); kinds: {kinds:?}",
    );
    assert!(
        !kinds.iter().any(|k| *k == "sweep_summary"),
        "SweepSummary MUST be suppressed when SIGINT lands mid-sweep \
         (HYG-05 / D5-04); kinds: {kinds:?}",
    );
}

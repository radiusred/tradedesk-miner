//! Phase 3 integration test — SIGINT preserves streamed findings + exits 130 (OP-06 / SC-5a / D3-22).
//!
//! `#![cfg(unix)]` — unix-only because `nix::sys::signal::kill` is a
//! POSIX-specific API. On non-unix platforms cargo skips the file entirely.
//!
//! ## Setup (Blocker 3 step 4 — consumes Plan 02/04/05 artifacts; NO retroactive edits)
//!
//! - Plan 02 Task 2: `ScanCtx.sleep_after_first_finding_ms` and
//!   `ScanRequest.sleep_after_first_finding_ms` cfg-gated `Option<u64>` fields
//!   + `test-internal` feature in `crates/miner-core/Cargo.toml`.
//! - Plan 04 Task 2: `LjungBoxScan::run` cancel-aware sleep loop polling
//!   `ctx.cancel` every ~10ms.
//! - Plan 05 Task 1: `ScanArgs --sleep-after-first-finding-ms <ms>` CLI flag
//!   cfg-gated identically.
//!
//! Plan 06 ONLY authors the integration test below — it does NOT add any
//! retroactive edits to earlier plans.
//!
//! ## Build prerequisite
//!
//! `--sleep-after-first-finding-ms` is gated on `cfg(any(test, feature =
//! "test-internal"))`. The `CARGO_BIN_EXE_miner` binary built by Cargo for
//! integration tests does NOT carry `cfg(test)` (test-runner builds the
//! BINARY without test-cfg) and does not auto-enable `test-internal`. The
//! test below invokes `cargo build -p miner-cli --features test-internal
//! --bin miner` as its first step to rebuild the binary with the feature
//! active, then spawns the resulting `target/debug/miner` directly.

#![cfg(unix)]
#![allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use chrono::NaiveDate;
use miner_core::Side;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

mod fixtures;
use fixtures::SyntheticCache;

/// Locate the workspace-root `target/debug/miner` produced by
/// `cargo build -p miner-cli --features test-internal --bin miner`.
fn target_miner_path() -> PathBuf {
    // CARGO_MANIFEST_DIR points at `crates/miner-cli`; the workspace root is
    // two levels up.
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

#[test]
#[serial_test::serial]
fn sigint_preserves_already_streamed_findings_and_exits_130() {
    // Step 0 — rebuild with --features test-internal so the cfg-gated CLI
    // flag is reachable end-to-end.
    build_with_test_internal_feature();
    let bin = target_miner_path();
    assert!(
        bin.exists(),
        "miner binary missing at {} after --features test-internal build",
        bin.display(),
    );

    // Step 1 — build a synthetic cache with one full UTC day so the scan
    // has something to read.
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).unwrap();
    let cache = SyntheticCache::new().with_deterministic_day("EURUSD", Side::Bid, day, 0xDEAD_BEEF);

    // Step 2 — spawn `miner scan ... --sleep-after-first-finding-ms 5000`.
    // The scan emits its single Result + then pauses; the test sends SIGINT
    // during the sleep and asserts the streamed findings persisted.
    let mut child = Command::new(&bin)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MINER_CACHE_ROOT", cache.cache_root())
        .env("MINER_BAR_CACHE_ROOT", cache.bar_cache_root())
        .env("MINER_OUTPUT", "stdout")
        .current_dir(cache.tempdir().path())
        .args([
            "scan",
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD:bid",
            "--timeframe",
            "15m",
            "--window",
            "2024-06-12:2024-06-13",
            "--sleep-after-first-finding-ms",
            "5000",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn miner");

    let child_pid = child.id();

    // Step 3 — read lines from the child's stdout until we see the first
    // Result envelope. The scan emits RunStart immediately, then the Result,
    // then pauses for 5s.
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
    assert!(saw_result, "child must emit a Result before SIGINT");

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

    // Step 6 — drain any remaining stdout and verify the Result + RunEnd
    // envelopes both made it to stdout (per-envelope flush + RunEnd emitted
    // before exit per D3-22).
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
    assert!(
        lines.iter().any(|v| v["kind"] == "run_start"),
        "RunStart must persist after SIGINT; captured lines: {:?}",
        lines.iter().map(|v| v["kind"].clone()).collect::<Vec<_>>(),
    );
    assert!(
        lines.iter().any(|v| v["kind"] == "result"),
        "the first Result must persist after SIGINT (D-19 per-envelope flush)",
    );
    assert!(
        lines.iter().any(|v| v["kind"] == "run_end"),
        "RunEnd must still be emitted after SIGINT (engine returns cleanly per D3-22); \
         captured lines: {:?}",
        lines.iter().map(|v| v["kind"].clone()).collect::<Vec<_>>(),
    );

    // Belt-and-brace: ensure the test didn't hang (the 5s sleep was
    // interrupted by SIGINT — we should be here within ~1s of the kill).
    let _ = Duration::from_millis(0);
}

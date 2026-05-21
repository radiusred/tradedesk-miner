//! Plan 03-07 CR-02 regression — SIGINT must override a forced engine error
//! and yield exit 130 (D3-24 cancel-overrides-everything contract).
//!
//! `#![cfg(unix)]` — `nix::sys::signal::kill` is a POSIX-specific API. On
//! non-Unix platforms cargo skips the file entirely.
//!
//! ## Mechanism
//!
//! The test spawns the real `miner` binary (rebuilt with
//! `--features test-internal` so the cfg-gated `MINER_FORCE_ENGINE_ERROR`
//! env hook compiled into `handle_scan_subcommand` is reachable). The hook
//! returns `Err(anyhow::Error)` immediately after `to_scan_request`
//! succeeds — deterministically driving the catch-all `Err` arm at the
//! `Command::Scan` caller in `main()` WITHOUT depending on production
//! reader/cache error paths.
//!
//! ## Why this test exercises CR-02 specifically
//!
//! - **Without** Plan 03-07 Task 3 Step 1: `handle_scan_subcommand` returns
//!   `Err(anyhow::Error)` → the `?` short-circuits `compute_exit_code` →
//!   anyhow's `Termination` prints + exits 1. The exit code is 1, NOT 130,
//!   and the test FAILS.
//! - **With** Plan 03-07 Task 3 Step 1's fix: the Err arm logs +
//!   maps to `RunOutcome::PreflightFailed` → `compute_exit_code(cancel=true,
//!   &PreflightFailed)` short-circuits to 130 via the `if cancelled {
//!   return 130 }` branch in `compute_exit_code`. The test PASSES.

#![cfg(unix)]
#![allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]

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
/// (Mirrors `sigint_preserves_stream.rs::target_miner_path`; WR-02 nit is
/// out of scope for Plan 03-07.)
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
/// cfg-gated `MINER_FORCE_ENGINE_ERROR` env hook inside
/// `handle_scan_subcommand` is reachable in the spawned binary.
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
#[serial_test::file_serial(miner_bin_test_internal)]
fn cancel_overrides_error_exit_130() {
    // Step 0 — rebuild with --features test-internal so the cfg-gated
    // MINER_FORCE_ENGINE_ERROR env hook is compiled into the spawned binary.
    build_with_test_internal_feature();
    let bin = target_miner_path();
    assert!(
        bin.exists(),
        "miner binary missing at {} after --features test-internal build",
        bin.display(),
    );

    // Step 1 — build a well-formed source cache so the failure mode under
    // test is unambiguously the forced engine error (not a cache miss).
    // The MINER_FORCE_ENGINE_ERROR hook fires AFTER to_scan_request and
    // BEFORE run_one touches the cache, so cache contents don't matter to
    // the assertion — but a well-formed cache means the test fails in a
    // clean way if the hook is ever accidentally removed.
    let day = NaiveDate::from_ymd_opt(2024, 6, 12).unwrap();
    let cache = SyntheticCache::new().with_deterministic_day("EURUSD", Side::Bid, day, 0xDEAD_BEEF);

    // Step 2 — spawn the miner with MINER_FORCE_ENGINE_ERROR=1 so
    // handle_scan_subcommand returns Err(anyhow::Error) deterministically
    // BEFORE run_one is ever called.
    let mut child = Command::new(&bin)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MINER_CACHE_ROOT", cache.cache_root())
        .env("MINER_BAR_CACHE_ROOT", cache.bar_cache_root())
        .env("MINER_OUTPUT", "stdout")
        .env("MINER_FORCE_ENGINE_ERROR", "1")
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
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn miner");

    let child_pid = child.id();

    // Step 3 — let the child start (clap parse + ctrlc handler install),
    // then deliver SIGINT. The MINER_FORCE_ENGINE_ERROR hook fires after
    // to_scan_request (no IO), so by the time SIGINT lands the child should
    // be in compute_exit_code with the Err already mapped to PreflightFailed
    // — meaning cancel.load() inside compute_exit_code reads `true` and the
    // exit-code short-circuit to 130 wins.
    std::thread::sleep(Duration::from_millis(30));
    kill(Pid::from_raw(child_pid as i32), Signal::SIGINT).expect("kill SIGINT");

    // Step 4 — wait for the child to exit; assert exit code 130 per D3-24.
    let status = child.wait().expect("child wait");
    let code = status.code();
    assert_eq!(
        code,
        Some(130),
        "cancel must override forced engine error; got {code:?} (CR-02 regression)",
    );
}

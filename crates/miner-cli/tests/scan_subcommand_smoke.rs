//! Phase 3 integration test — `miner scan` subcommand happy + sad paths.
//!
//! Pattern analog: `tests/cli_streams.rs` (whole file) — `assert_cmd::Command::cargo_bin("miner")`
//! + `env_clear()` + custom `MINER_*` env vars + `#[serial_test::serial]`.
//!
//! ## Five tests covered (per VALIDATION.md Per-Task Verification Map)
//!
//! - `scan_emits_run_start_result_run_end` — happy path: kinds [run_start, result, run_end] in order.
//! - `unknown_scan_emits_wireerror_exit_1` — preflight rejection: stdout empty, stderr WireError.
//! - `invalid_params_emits_wireerror_exit_1` — preflight rejection: stdout empty, stderr WireError.
//! - `dry_run_emits_dry_run_finding_only` — D3-21 envelope shape via subprocess.
//! - `exit_code_routing_zero_one_two` — D3-24 four-tier exit code routing (0 / 1 / 2).
//!
//! Wave 0 scaffold: every test `#[ignore]`. Plan 03-06 fills the bodies.

#![allow(dead_code, unused_imports)]

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
#[serial_test::serial]
fn scan_emits_run_start_result_run_end() {
    unimplemented!(
        "Plan 03-06 implements per VALIDATION OP-01/SC-1 — spawn `miner scan stats.autocorr.ljung_box@1 \
         --instrument EURUSD --side bid --timeframe 15m --window ...`, parse stdout JSONL, \
         assert lines[0].kind == run_start, lines[1].kind == result, lines[2].kind == run_end"
    )
}

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
#[serial_test::serial]
fn unknown_scan_emits_wireerror_exit_1() {
    unimplemented!(
        "Plan 03-06 implements per VALIDATION OP-08/SC-4a — spawn `miner scan nonexistent.scan@99 ...`, \
         assert status.code() == Some(1), stdout empty (T-01-03), stderr WireError.code == \"unknown_scan\""
    )
}

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
#[serial_test::serial]
fn invalid_params_emits_wireerror_exit_1() {
    unimplemented!(
        "Plan 03-06 implements per VALIDATION OP-08/SC-4b — spawn `miner scan stats.autocorr.ljung_box@1 \
         --params lags=banana ...`, assert exit 1 + stderr WireError.code == \"invalid_parameter\""
    )
}

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
#[serial_test::serial]
fn dry_run_emits_dry_run_finding_only() {
    unimplemented!(
        "Plan 03-06 implements per VALIDATION OP-05/SC-5 — spawn `miner scan ... --dry-run`, \
         parse stdout, assert kinds [run_start, dry_run, run_end] — NO result"
    )
}

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
#[serial_test::serial]
fn exit_code_routing_zero_one_two() {
    unimplemented!(
        "Plan 03-06 implements per VALIDATION OP-08 + D3-24 — three sub-scenarios in one test: \
         (a) happy run → exit 0; (b) preflight reject → exit 1; (c) scan_error mid-stream → exit 2"
    )
}

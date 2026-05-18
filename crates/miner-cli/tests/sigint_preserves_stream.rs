//! Phase 3 integration test — SIGINT preserves streamed findings + exits 130 (OP-06 / D3-22).
//!
//! Pattern analog: `tests/cli_streams.rs::run_emit_fixture_happy` (assert_cmd subprocess
//! shape) + 03-RESEARCH §"Code Examples" lines 693-742 (the `nix::sys::signal::kill`
//! snippet that delivers SIGINT to the spawned child process).
//!
//! ## Unix-only
//!
//! `#![cfg(unix)]` at the file level so non-unix platforms skip compilation
//! entirely. `nix` is a unix-only dev-dep (declared in `crates/miner-cli/Cargo.toml`
//! with `default-features = false, features = ["signal"]`).
//!
//! ## D3-22 contract
//!
//! 1. Spawn `miner scan` with a scan that streams a Result then sleeps.
//! 2. Wait for the first Result line on stdout.
//! 3. `nix::sys::signal::kill(Pid::from_raw(child.id() as i32), Signal::SIGINT)`.
//! 4. Wait for exit; assert `status.code() == Some(130)`.
//! 5. Assert the captured stdout already contains the streamed Result + RunEnd
//!    (the SIGINT did NOT truncate the in-flight envelope — D-19 per-envelope flush).
//!
//! Wave 0 scaffold: signature-only `#[test] #[ignore]` stub. Plan 03-06 fills
//! the body verbatim per 03-RESEARCH §"Code Examples".

#![cfg(unix)]
#![allow(dead_code, unused_imports)]

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
#[serial_test::serial]
fn sigint_preserves_already_streamed_findings_and_exits_130() {
    // Plan 03-06 fills per 03-RESEARCH §"Code Examples" lines 693-742:
    // 1. Spawn `miner scan ...` with a test-only SleepScan registered under cfg(test).
    // 2. Wait for the first Result line on stdout (read until newline).
    // 3. nix::sys::signal::kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).
    // 4. child.wait() — assert exit code 130.
    // 5. Drain stdout — assert it contains Result + RunEnd kinds.
    unimplemented!(
        "Plan 03-06 implements sigint_preserves_already_streamed_findings_and_exits_130 \
         per OP-06 / D3-22 / 03-RESEARCH §Code Examples lines 693-742"
    )
}

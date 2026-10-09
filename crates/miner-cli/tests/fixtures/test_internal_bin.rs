//! The `miner` binary rebuilt with `--features test-internal`, for the SIGINT
//! integration tests (`sigint_preserves_stream.rs`, `sigint_mid_sweep.rs`,
//! `cancel_overrides_error_exit_130.rs`).
//!
//! Those tests need the cfg-gated `--sleep-after-first-finding-ms` flag and the
//! `MINER_FORCE_ENGINE_ERROR` hook, which the `CARGO_BIN_EXE_miner` binary Cargo
//! builds for integration tests does not carry (no `cfg(test)`, and miner-cli's
//! own `test-internal` feature is off). So each test rebuilds the binary with the
//! feature, in place, and spawns it.
//!
//! The path comes from `CARGO_BIN_EXE_miner` and the rebuild is pointed at that
//! binary's own target directory and profile, so the two agree wherever the
//! target directory lives: the workspace default, `CARGO_TARGET_DIR`,
//! `CARGO_BUILD_TARGET_DIR`, `build.target-dir` in a Cargo config, or
//! `--target-dir`. Callers serialise on
//! `#[serial_test::file_serial(miner_bin_test_internal)]`, because the rebuild
//! rewrites a file every test process shares.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Rebuild `miner` with `--features test-internal` over the binary Cargo built
/// for this test run, and return its path.
///
/// # Panics
///
/// If the binary path has no `<target-dir>/<profile-dir>/` parents, if the
/// `cargo build` invocation fails, or if the binary is missing afterwards.
pub fn miner_with_test_internal() -> PathBuf {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_miner"));
    let profile_dir = bin.parent().expect("binary has a profile directory");
    let target_dir = profile_dir
        .parent()
        .expect("profile directory has a target directory");
    let profile = cargo_profile(profile_dir);

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root two levels above crates/miner-cli");
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
        .arg("--profile")
        .arg(&profile)
        .arg("--target-dir")
        .arg(target_dir)
        .current_dir(workspace_root)
        .status()
        .expect("cargo build invocation");
    assert!(
        status.success(),
        "cargo build -p miner-cli --features test-internal --bin miner \
         --profile {profile} --target-dir {} failed",
        target_dir.display(),
    );
    assert!(
        bin.exists(),
        "miner binary missing at {} after --features test-internal build",
        bin.display(),
    );
    bin
}

/// The Cargo profile whose output directory is `profile_dir`: `dev` builds into
/// `debug/`, every other profile into a directory of its own name.
fn cargo_profile(profile_dir: &Path) -> String {
    match profile_dir.file_name().and_then(|n| n.to_str()) {
        Some("debug") => "dev".to_owned(),
        Some(other) => other.to_owned(),
        None => panic!("profile directory {} has no name", profile_dir.display()),
    }
}

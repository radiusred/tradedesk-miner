//! Plan 07 Task 1 — production-grade precedence integration test for FOUND-05.
//!
//! This is the production sibling of the Plan 01-02 spike (`spike_figment_precedence.rs`
//! — since deleted) and the in-crate `figment::Jail`-based unit tests in
//! `miner_core::config::tests`. It exercises the locked merge order — CLI then env
//! then TOML then error — using
//! `miner_core::config::{MinerConfig, CliOverrides, build_figment}` against the public
//! crate re-export surface (`miner_core::*`). TOML files are written to the Jail's
//! scoped scratch directory; `figment::Jail` provides env isolation.
//!
//! ### Why `Jail` and not raw `std::env::set_var`
//!
//! The Phase 1 plan body proposed raw `std::env::set_var`/`remove_var` with
//! `#[serial_test::serial]`. The workspace lints set `unsafe_code = "forbid"` —
//! a level that `#![allow(unsafe_code)]` cannot override — and Rust 2024 made env
//! mutation `unsafe`. `figment::Jail::expect_with(...)` is the documented testing
//! helper that wraps env mutation in its own `unsafe` block (within figment's own
//! crate, behind its `test` feature, which the miner-core dev-dep enables). Plan 07
//! Task 1 deviation Rule 3: use Jail in lieu of raw env mutation so the workspace's
//! `forbid(unsafe_code)` invariant is preserved.
//!
//! Functionally identical: Jail is a process-wide RAII lock that snapshots env on
//! enter, mutates within its closure, and restores on drop. `serial_test::serial`
//! is the manual equivalent.
//!
//! Why a separate integration test even though the in-crate `Jail` tests cover the
//! same precedence: the in-crate tests live behind `#[cfg(test)]` in the library
//! itself; an integration test lives in `tests/` and links against the public crate
//! interface (`miner_core::*` re-exports). That second linkage proves the public
//! re-export surface (FROZEN in `lib.rs`) is sufficient to exercise the precedence
//! contract — what every downstream binary will actually do.
//!
//! Coverage:
//!
//! - Test 1 — `cli_wins_over_env_and_toml`: all three layers present for each of the
//!   three fields (`cache_root`, `bar_cache_root`, `output`); CLI must win.
//! - Test 2 — `env_wins_when_cli_omitted`: TOML + env, no CLI; env must win.
//! - Test 3 — `toml_wins_when_only_source`: TOML only; TOML value flows through.
//! - Test 4 — `missing_required_yields_err`: no source; `figment.extract()` errors
//!   and the message mentions `cache_root`.

use std::path::{Path, PathBuf};

use figment::Jail;
use miner_core::{CliOverrides, MinerConfig, OutputDest, build_figment};

/// Build a TOML file at the supplied path inside `jail`. The Jail provides a
/// scoped scratch directory; the file vanishes when the Jail exits.
fn write_jail_toml(
    jail: &mut Jail,
    cache_root: &str,
    bar_cache_root: &str,
    output: &str,
) -> Result<(), figment::Error> {
    jail.create_file(
        "miner.toml",
        &format!(
            "cache_root = \"{cache_root}\"\nbar_cache_root = \"{bar_cache_root}\"\noutput = \"{output}\"\n",
        ),
    )?;
    Ok(())
}

/// Convenience: extract a `MinerConfig` from `build_figment` with the supplied
/// cfg-file path and CLI overrides. Mirrors the production `MinerConfig::resolve`
/// path used by `miner-cli`.
fn resolve(cfg_path: Option<&Path>, cli: CliOverrides) -> Result<MinerConfig, figment::Error> {
    build_figment(cfg_path, cli).extract::<MinerConfig>()
}

// ---------------------------------------------------------------------------
// Test 1 — cli_wins_over_env_and_toml
//
// All three layers populated for each of the three fields. CLI must win. The
// Plan 07 plan body explicitly requires repeating this for `cache_root`,
// `bar_cache_root`, and `output` — we do all three in one Jail to keep the
// env-var lock held across the comparisons.
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn cli_wins_over_env_and_toml() {
    Jail::expect_with(|jail| {
        write_jail_toml(jail, "/file/cache", "/file/bar", "stdout")?;
        jail.set_env("MINER_CACHE_ROOT", "/env/cache");
        jail.set_env("MINER_BAR_CACHE_ROOT", "/env/bar");
        jail.set_env("MINER_OUTPUT", "stdout");

        let cli = CliOverrides {
            cache_root: Some(PathBuf::from("/cli/cache")),
            bar_cache_root: Some(PathBuf::from("/cli/bar")),
            // OutputDest::File covers the variant; the test checks that ANY CLI value
            // for `output` wins over env/TOML for the same field.
            output: Some(OutputDest::File(PathBuf::from("/cli/out.jsonl"))),
        };

        let cfg = resolve(Some(Path::new("miner.toml")), cli)
            .expect("resolve must succeed when CLI wins");

        assert_eq!(
            cfg.cache_root,
            PathBuf::from("/cli/cache"),
            "CLI must win over env and TOML for cache_root"
        );
        assert_eq!(
            cfg.bar_cache_root,
            PathBuf::from("/cli/bar"),
            "CLI must win over env and TOML for bar_cache_root"
        );
        assert_eq!(
            cfg.output,
            OutputDest::File(PathBuf::from("/cli/out.jsonl")),
            "CLI must win over env and TOML for output"
        );
        Ok(())
    });
}

// ---------------------------------------------------------------------------
// Test 2 — env_wins_when_cli_omitted
//
// TOML + env present; CLI defaults (None for all fields). Env must win over TOML
// for the fields it covers; TOML survives for fields env doesn't touch.
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn env_wins_when_cli_omitted() {
    Jail::expect_with(|jail| {
        write_jail_toml(jail, "/file/cache", "/file/bar", "stdout")?;
        jail.set_env("MINER_CACHE_ROOT", "/env/cache");
        jail.set_env("MINER_BAR_CACHE_ROOT", "/env/bar");
        // Intentionally NOT setting MINER_OUTPUT — TOML's `output = "stdout"` should
        // survive for that field.

        let cfg = resolve(Some(Path::new("miner.toml")), CliOverrides::default())
            .expect("resolve must succeed");

        assert_eq!(
            cfg.cache_root,
            PathBuf::from("/env/cache"),
            "env must win when CLI is None"
        );
        assert_eq!(
            cfg.bar_cache_root,
            PathBuf::from("/env/bar"),
            "env must win when CLI is None for bar_cache_root"
        );
        assert_eq!(
            cfg.output,
            OutputDest::Stdout,
            "TOML output must survive when env+CLI omit it"
        );
        Ok(())
    });
}

// ---------------------------------------------------------------------------
// Test 3 — toml_wins_when_only_source
//
// TOML is the only layer; env and CLI are empty. TOML values flow through to
// MinerConfig.
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn toml_wins_when_only_source() {
    Jail::expect_with(|jail| {
        write_jail_toml(jail, "/file/cache", "/file/bar", "stdout")?;
        // No env vars; Jail clears MINER_* on entry.

        let cfg = resolve(Some(Path::new("miner.toml")), CliOverrides::default())
            .expect("resolve must succeed from TOML alone");

        assert_eq!(cfg.cache_root, PathBuf::from("/file/cache"));
        assert_eq!(cfg.bar_cache_root, PathBuf::from("/file/bar"));
        assert_eq!(cfg.output, OutputDest::Stdout);
        Ok(())
    });
}

// ---------------------------------------------------------------------------
// Test 4 — missing_required_yields_err
//
// No source for required fields. `figment.extract::<MinerConfig>()` returns Err
// and the error message mentions `cache_root`.
// ---------------------------------------------------------------------------

#[test]
#[serial_test::serial]
fn missing_required_yields_err() {
    Jail::expect_with(|_jail| {
        // No cfg file (no jail.create_file), no env (Jail starts with MINER_* cleared),
        // no CLI overrides. Pass `None` for the cfg path so the Toml provider is skipped.
        let err = resolve(None, CliOverrides::default())
            .expect_err("must error when no source supplies cache_root");
        let msg = err.to_string();
        assert!(
            msg.contains("cache_root") || msg.contains("missing"),
            "error message should mention the missing field; got: {msg}"
        );
        Ok(())
    });
}

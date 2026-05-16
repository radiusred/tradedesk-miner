//! Spike test — Risk 5 / Assumption A4 verification (Plan 01-02).
//!
//! Closes the live-verification line in 01-RESEARCH §"Assumptions Log" A4: that
//! figment + clap with the `Option<T>` + `skip_serializing_if` pattern + CLI-last
//! merge order produces the precedence CLI > env > TOML > error.
//!
//! Uses `figment::Jail` (the canonical figment test fixture) rather than direct
//! `std::env::set_var` calls. Rationale: under Rust 1.85 + edition 2024 the
//! env-mutation calls are `unsafe fn`, and the workspace `unsafe_code = "forbid"`
//! lint correctly rejects the `unsafe` block needed to invoke them. `Jail`
//! serialises env-var access process-wide and scope-cleans on drop — both
//! correctness wins over a hand-rolled `set/unset` dance.
//!
//! This test (and the `spike_figment` module it exercises) will be DELETED by Plan 05.

use std::path::PathBuf;

use figment::Jail;
use miner_core::spike_figment::{
    SpikeCliOverrides, SpikeConfig, SpikeOutputDest, build_figment,
};

const TOML_BODY: &str = r#"
cache_root = "/file"
bar_cache_root = "/file/bar"
output = "stdout"
"#;

#[test]
fn spike_precedence_cli_wins_over_env_over_toml() {
    Jail::expect_with(|jail| {
        jail.create_file("miner.toml", TOML_BODY)?;
        let toml_path = jail.directory().join("miner.toml");

        // ===== Test 1: all three sources set, CLI wins =====
        jail.set_env("MINER_CACHE_ROOT", "/env");
        let cli = SpikeCliOverrides {
            cache_root: Some(PathBuf::from("/cli")),
            ..Default::default()
        };
        let cfg: SpikeConfig = build_figment(Some(&toml_path), cli).extract()?;
        assert_eq!(
            cfg.cache_root,
            PathBuf::from("/cli"),
            "Test 1: CLI must win over env+toml"
        );
        // bar_cache_root has no env / no cli → TOML survives
        assert_eq!(
            cfg.bar_cache_root,
            PathBuf::from("/file/bar"),
            "Test 1: bar_cache_root should fall through to TOML"
        );
        // output: TOML supplies stdout, no env, no cli
        assert_eq!(
            cfg.output,
            SpikeOutputDest::Stdout,
            "Test 1: output should be stdout from TOML"
        );

        // ===== Test 2: CLI omitted, env wins =====
        // (MINER_CACHE_ROOT still set to "/env" from Test 1.)
        let cli = SpikeCliOverrides::default();
        let cfg: SpikeConfig = build_figment(Some(&toml_path), cli).extract()?;
        assert_eq!(
            cfg.cache_root,
            PathBuf::from("/env"),
            "Test 2: env must win over TOML when CLI absent"
        );

        Ok(())
    });

    // Test 3 / 4 / 5 use fresh Jail instances so env state from Test 1-2 is fully
    // cleared. `Jail::expect_with` panics on `Err`, which is what we want for the
    // success cases.
    Jail::expect_with(|jail| {
        jail.create_file("miner.toml", TOML_BODY)?;
        let toml_path = jail.directory().join("miner.toml");

        // ===== Test 3: CLI and env omitted, TOML wins =====
        let cli = SpikeCliOverrides::default();
        let cfg: SpikeConfig = build_figment(Some(&toml_path), cli).extract()?;
        assert_eq!(
            cfg.cache_root,
            PathBuf::from("/file"),
            "Test 3: TOML should resolve when env+CLI absent"
        );

        Ok(())
    });

    // ===== Test 4: No source supplies → Err =====
    Jail::expect_with(|jail| {
        // Point at a TOML path that does NOT exist so figment skips the Toml
        // provider (build_figment honours `path.exists()`).
        let missing_toml = jail.directory().join("does-not-exist.toml");
        let cli = SpikeCliOverrides::default();
        let result = build_figment(Some(&missing_toml), cli).extract::<SpikeConfig>();
        assert!(
            result.is_err(),
            "Test 4: missing required fields must yield Err, got: {result:?}"
        );
        Ok(())
    });

    // ===== Test 5: bar_cache_root + output follow the same precedence =====
    Jail::expect_with(|jail| {
        jail.create_file("miner.toml", TOML_BODY)?;
        let toml_path = jail.directory().join("miner.toml");

        // 5a: CLI overrides bar_cache_root + output across all three layers.
        jail.set_env("MINER_BAR_CACHE_ROOT", "/env/bar");
        jail.set_env("MINER_OUTPUT", "stdout");
        jail.set_env("MINER_CACHE_ROOT", "/env");
        let cli = SpikeCliOverrides {
            bar_cache_root: Some(PathBuf::from("/cli/bar")),
            output: Some(SpikeOutputDest::File(PathBuf::from("/cli/out.jsonl"))),
            ..Default::default()
        };
        let cfg: SpikeConfig = build_figment(Some(&toml_path), cli).extract()?;
        assert_eq!(
            cfg.bar_cache_root,
            PathBuf::from("/cli/bar"),
            "Test 5a: CLI bar_cache_root must win"
        );
        assert_eq!(
            cfg.output,
            SpikeOutputDest::File(PathBuf::from("/cli/out.jsonl")),
            "Test 5a: CLI output must win"
        );

        // 5b: drop CLI, expect env to beat TOML for bar_cache_root.
        let cli = SpikeCliOverrides::default();
        let cfg: SpikeConfig = build_figment(Some(&toml_path), cli).extract()?;
        assert_eq!(
            cfg.bar_cache_root,
            PathBuf::from("/env/bar"),
            "Test 5b: env bar_cache_root must beat TOML"
        );

        Ok(())
    });
}

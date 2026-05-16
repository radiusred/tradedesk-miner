//! Configuration SCHEMA types + figment builder (D-16, FOUND-05).
//!
//! Plan 03 landed the type definitions ([`MinerConfig`], [`OutputDest`]).
//! Plan 05 (this file) adds the figment-based builder using the verified A4 pattern
//! from Plan 01-02 (CLI > env > TOML > error precedence):
//!
//! - [`CliOverrides`] — figment overlay struct with `Option<T>` fields + each carries
//!   `#[serde(skip_serializing_if = "Option::is_none")]` per RESEARCH §"Common
//!   Pitfalls" Pitfall 1 (the figment-docs precedence-inversion fix).
//! - [`build_figment`] — merges `Toml::file(path?) -> Env::prefixed("MINER_").split("__") ->
//!   Serialized::defaults(cli)` in that order. CLI is merged LAST so CLI wins.
//! - [`MinerConfig::resolve`] — thin convenience wrapper around `build_figment(..).extract()`.
//!
//! ## Env-mapping contract (regression-guarded by Test 6)
//!
//! The Env provider is configured with `.split("__")` — *double*-underscore.
//! Single-underscore would corrupt `cache_root` into `cache.root` (a nested table
//! that does not exist on `MinerConfig`), causing extraction to fail. See RESEARCH
//! §"Common Pitfalls" Pitfall 1.
//!
//! Locked mappings:
//! - `MINER_CACHE_ROOT` → field `cache_root`
//! - `MINER_BAR_CACHE_ROOT` → field `bar_cache_root`
//! - `MINER_OUTPUT` → field `output`
//!
//! Future nested config (e.g. `MinerConfig.telemetry.enabled`) maps via
//! `MINER_TELEMETRY__ENABLED` (the double underscore splits the path).
//!
//! ## Zero hardcoded paths (FOUND-05)
//!
//! This module contains NO path literals — see Test 5 for the enforced list of
//! forbidden tokens (platform-native paths, XDG-config references, and the
//! CWD-default config-file name). Path resolution (XDG / CWD fallback) is the
//! CLI's responsibility (see `miner-cli::cli::resolve_toml_path`).
//!
//! ## Threat T-01-01 mitigation
//!
//! The library never tilde-expands env-derived strings, never canonicalises paths,
//! never resolves symlinks. Figment receives whatever path the CLI passed; a missing
//! file produces no TOML values (figment semantics); a missing required field
//! surfaces as `figment::Error` which the CLI translates to a `PreflightCode::*`
//! `WireError` on stderr with NO stack-trace leakage.

use std::path::{Path, PathBuf};

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where the miner streams its JSONL findings output.
///
/// `Stdout` is the default for agent-operability (CLI / MCP / HTTP all consume the
/// same byte-stream). `File(PathBuf)` lets a wrapper redirect to disk for batch jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutputDest {
    Stdout,
    File(PathBuf),
}

/// Top-level miner configuration (D-16).
///
/// All three fields are NON-optional — `figment.extract()` must produce an error if
/// any field is missing from the merged config sources. This is the Plan 05
/// precedence contract verified by the Plan 01-02 spike.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MinerConfig {
    /// Root of the `tradedesk-dukascopy` cache (read-only consumer; Phase 2+).
    pub cache_root: PathBuf,
    /// Root of the derived bar cache (the only writable state miner owns).
    pub bar_cache_root: PathBuf,
    pub output: OutputDest,
}

impl MinerConfig {
    /// Convenience wrapper around `build_figment(cfg_file, cli).extract()`.
    ///
    /// # Errors
    ///
    /// Returns `figment::Error` if any of:
    /// - a required field is missing from every layer (`Kind::MissingField`)
    /// - a layer produced a value of the wrong type (`Kind::InvalidType` /
    ///   `Kind::InvalidValue`)
    /// - a TOML file fails to parse (`Kind::Message` or similar)
    ///
    /// Callers (`miner-cli::main`) should inspect `Error::kind` to classify the
    /// failure into the correct `PreflightCode` (`MissingRequiredConfig` vs
    /// `InvalidConfig`) and emit a structured stderr `WireError` before exiting 1.
    pub fn resolve(cfg_file: Option<&Path>, cli: CliOverrides) -> Result<Self, figment::Error> {
        build_figment(cfg_file, cli).extract()
    }
}

/// Figment overlay produced from CLI-parsed arguments (Plan 05).
///
/// Every field is `Option<T>` with `#[serde(skip_serializing_if = "Option::is_none")]`.
/// When a CLI arg is unset (`None`), it is OMITTED from the serialised representation —
/// which means `Serialized::defaults(cli)` produces no value for that key, and the
/// preceding `Env` / `Toml` layers' values survive. When a CLI arg IS set (`Some(_)`),
/// it appears in the serialised representation and — because `CliOverrides` is merged
/// LAST — overrides everything.
///
/// This is the "figment-docs precedence inversion fix" pattern from RESEARCH
/// §"Common Pitfalls" Pitfall 1. Without `skip_serializing_if`, `None` would
/// serialise as `null` and OVERWRITE the env/TOML values with `null`, which is the
/// opposite of the desired precedence.
///
/// The actual `clap::Parser`-derived struct that produces a `CliOverrides` lives in
/// `miner-cli::cli` — `miner-core` does not depend on `clap` (D-16).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CliOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bar_cache_root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputDest>,
}

/// Construct the figment with the LOCKED merge order (FOUND-05).
///
/// Merge order — earlier sources are overridden by later ones:
///   1. `Toml::file(cfg_file?)` — lowest priority (config file)
///   2. `Env::prefixed("MINER_").split("__")` — middle priority (environment)
///   3. `Serialized::defaults(cli)` — highest priority (CLI args; merged LAST)
///
/// Library carries NO defaults; a missing required field surfaces as
/// `figment::Error` at `.extract()` time.
///
/// # Locked env mapping (regression-tested in Test 6)
///
/// `Env::prefixed("MINER_").split("__")` splits ONLY on *double* underscore. So:
/// - `MINER_CACHE_ROOT` → lowercased `cache_root` (single `_` left in the field
///   name) which matches `MinerConfig::cache_root` directly. No nesting.
/// - `MINER_BAR_CACHE_ROOT` → `bar_cache_root` (still no nesting).
/// - `MINER_OUTPUT` → `output`.
///
/// If we ever switched to `.split("_")` (single underscore), figment would parse
/// `MINER_CACHE_ROOT` as a nested `cache.root` table — which `MinerConfig` does
/// not have — and extraction would fail. The `Env::prefixed("MINER_").split("__")`
/// configuration is therefore part of the public contract and is verified by
/// Test 6.
#[must_use]
pub fn build_figment(cfg_file: Option<&Path>, cli: CliOverrides) -> Figment {
    let mut fig = Figment::new();
    if let Some(path) = cfg_file {
        fig = fig.merge(Toml::file(path));
    }
    fig.merge(Env::prefixed("MINER_").split("__"))
        .merge(Serialized::defaults(cli))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use figment::Jail;

    /// Test 5 (legacy, from Plan 03) — `miner_config_type_shape`: confirms
    /// `MinerConfig` has the locked three-field shape and that `OutputDest` is
    /// the enum we expect. Compile-time + serde round-trip.
    #[test]
    fn miner_config_type_shape() {
        let cfg = MinerConfig {
            cache_root: PathBuf::from("/cache"),
            bar_cache_root: PathBuf::from("/bar-cache"),
            output: OutputDest::Stdout,
        };
        let json = serde_json::to_string(&cfg).expect("serialise");
        let parsed: MinerConfig = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(parsed, cfg);

        // The File(PathBuf) variant also round-trips.
        let cfg2 = MinerConfig {
            cache_root: PathBuf::from("/c"),
            bar_cache_root: PathBuf::from("/b"),
            output: OutputDest::File(PathBuf::from("/tmp/out.jsonl")),
        };
        let json2 = serde_json::to_string(&cfg2).expect("serialise");
        let parsed2: MinerConfig = serde_json::from_str(&json2).expect("deserialise");
        assert_eq!(parsed2, cfg2);
    }

    /// Test 1 — `build_figment_cli_wins_over_env_and_toml`: with all three layers
    /// present, CLI wins. Mirrors Plan 01-02 spike Test 1.
    #[test]
    fn build_figment_cli_wins_over_env_and_toml() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "config-fixture.toml",
                r#"
                cache_root = "/file/cache"
                bar_cache_root = "/file/bar"
                output = "stdout"
                "#,
            )?;
            jail.set_env("MINER_CACHE_ROOT", "/env/cache");

            let cli = CliOverrides {
                cache_root: Some(PathBuf::from("/cli/cache")),
                bar_cache_root: None,
                output: None,
            };
            let cfg = MinerConfig::resolve(Some(Path::new("config-fixture.toml")), cli)
                .expect("resolve must succeed with all layers");
            assert_eq!(
                cfg.cache_root,
                PathBuf::from("/cli/cache"),
                "CLI must win over env and TOML"
            );
            // bar_cache_root and output had no CLI value, no env value — TOML survives.
            assert_eq!(cfg.bar_cache_root, PathBuf::from("/file/bar"));
            assert_eq!(cfg.output, OutputDest::Stdout);
            Ok(())
        });
    }

    /// Test 2 — `build_figment_env_wins_when_cli_omitted`: TOML + env, CLI default
    /// (all None), env should win over TOML.
    #[test]
    fn build_figment_env_wins_when_cli_omitted() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "config-fixture.toml",
                r#"
                cache_root = "/file/cache"
                bar_cache_root = "/file/bar"
                output = "stdout"
                "#,
            )?;
            jail.set_env("MINER_CACHE_ROOT", "/env/cache");

            let cfg = MinerConfig::resolve(Some(Path::new("config-fixture.toml")), CliOverrides::default())
                .expect("resolve must succeed");
            assert_eq!(
                cfg.cache_root,
                PathBuf::from("/env/cache"),
                "env must win when CLI is None"
            );
            assert_eq!(cfg.bar_cache_root, PathBuf::from("/file/bar"));
            assert_eq!(cfg.output, OutputDest::Stdout);
            Ok(())
        });
    }

    /// Test 3 — `build_figment_toml_wins_when_only_source`: TOML only (no env, no
    /// CLI). The TOML value flows through.
    #[test]
    fn build_figment_toml_wins_when_only_source() {
        Jail::expect_with(|jail| {
            jail.create_file(
                "config-fixture.toml",
                r#"
                cache_root = "/file/cache"
                bar_cache_root = "/file/bar"
                output = "stdout"
                "#,
            )?;
            // No MINER_* env vars set inside this Jail.
            let cfg = MinerConfig::resolve(Some(Path::new("config-fixture.toml")), CliOverrides::default())
                .expect("resolve must succeed from TOML alone");
            assert_eq!(cfg.cache_root, PathBuf::from("/file/cache"));
            assert_eq!(cfg.bar_cache_root, PathBuf::from("/file/bar"));
            assert_eq!(cfg.output, OutputDest::Stdout);
            Ok(())
        });
    }

    /// Test 4 — `build_figment_missing_required_yields_err`: no source, expect
    /// `figment.extract::<MinerConfig>()` to return an Err whose first
    /// `Error::kind` is `MissingField`.
    #[test]
    fn build_figment_missing_required_yields_err() {
        Jail::expect_with(|jail| {
            // No file, no env. Use the Jail to make sure no MINER_* env vars leak in.
            let _ = jail; // silence unused
            let err = MinerConfig::resolve(None, CliOverrides::default())
                .expect_err("must error when no source supplies cache_root");
            let msg = err.to_string();
            assert!(
                msg.contains("cache_root") || msg.contains("missing"),
                "error message should mention the missing field, got: {msg}",
            );
            Ok(())
        });
    }

    /// Test 5 — `library_has_no_hardcoded_paths`: grep gate enforcing FOUND-05.
    /// The library code must not contain platform-native path literals; path
    /// resolution is the CLI's responsibility.
    ///
    /// Implementation: read the source, strip line comments (lines whose first
    /// non-whitespace bytes are `//`), then assert none of the gated patterns
    /// appear in the remaining (non-comment) source. The forbidden literals
    /// are constructed at runtime so the test body itself does not violate
    /// the gate.
    #[test]
    fn library_has_no_hardcoded_paths() {
        let src = include_str!("mod.rs");
        // Strip comment-only lines so doc-comment references to the gated
        // patterns are not themselves violations.
        let code_only: String = src
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        // Construct needles at runtime so this very test does not become a
        // violation by definition.
        let forbidden: [String; 6] = [
            format!("/{}/", "opt"),
            format!("/{}/", "home"),
            format!("{}{}", "$", "HOME"),
            format!("{}/", "~"),
            format!(".{}{}", "/miner.", "toml"),
            format!("XDG_{}_HOME", "CONFIG"),
        ];
        for needle in &forbidden {
            assert!(
                !code_only.contains(needle.as_str()),
                "FOUND-05 violation: library code contains forbidden literal {needle:?}",
            );
        }
    }

    /// Test 6 — `env_split_maps_uppercase_to_snake_case_fields`: regression gate
    /// for the `.split("__")` Env provider configuration. If the split string is
    /// wrong (`_` instead of `__`), `MINER_BAR_CACHE_ROOT` parses as nested
    /// `bar.cache.root` and extraction fails. This test catches that bug in the
    /// unit test, not the integration test.
    #[test]
    fn env_split_maps_uppercase_to_snake_case_fields() {
        Jail::expect_with(|jail| {
            jail.set_env("MINER_CACHE_ROOT", "/c");
            jail.set_env("MINER_BAR_CACHE_ROOT", "/b");
            jail.set_env("MINER_OUTPUT", "stdout");

            let cfg = MinerConfig::resolve(None, CliOverrides::default())
                .expect("env-only resolve must succeed with .split(\"__\")");
            assert_eq!(cfg.cache_root, PathBuf::from("/c"));
            assert_eq!(cfg.bar_cache_root, PathBuf::from("/b"));
            assert_eq!(cfg.output, OutputDest::Stdout);
            Ok(())
        });
    }

    /// Test 7 — `figment_error_kind_classification`: locks the contract that
    /// Task 2's preflight mapper depends on. Three sub-cases produce three
    /// different `figment::Error::Kind` discriminants:
    ///
    /// - (a) No source for required `cache_root` → `Kind::MissingField`.
    /// - (b) TOML with `cache_root = 42` (integer for a path) → `Kind::InvalidType`.
    /// - (c) TOML with malformed syntax → a TOML parse error surfaces as either
    ///       `Kind::Message` or another non-`MissingField` variant.
    ///
    /// Mapping every error to `MissingRequiredConfig` is FORBIDDEN — the CLI
    /// classifier must distinguish `MissingField` from everything else.
    #[test]
    fn figment_error_kind_classification() {
        use figment::error::Kind;

        // (a) MissingField — no sources at all.
        Jail::expect_with(|_jail| {
            let err = MinerConfig::resolve(None, CliOverrides::default()).unwrap_err();
            let first = err.into_iter().next().expect("at least one error");
            assert!(
                matches!(first.kind, Kind::MissingField(_)),
                "(a) expected MissingField, got {:?}",
                first.kind,
            );
            Ok(())
        });

        // (b) InvalidType — cache_root is an integer instead of a path string.
        Jail::expect_with(|jail| {
            jail.create_file(
                "bad-type.toml",
                r#"
                cache_root = 42
                bar_cache_root = "/b"
                output = "stdout"
                "#,
            )?;
            let err =
                MinerConfig::resolve(Some(Path::new("bad-type.toml")), CliOverrides::default())
                    .unwrap_err();
            let first = err.into_iter().next().expect("at least one error");
            assert!(
                matches!(first.kind, Kind::InvalidType(_, _)),
                "(b) expected InvalidType for cache_root=42, got {:?}",
                first.kind,
            );
            Ok(())
        });

        // (c) Parse error — malformed TOML. Figment surfaces parse errors as
        // `Kind::Message(...)` in 0.10.x (or another non-MissingField variant).
        // The CLI's classifier maps any non-MissingField variant to
        // `PreflightCode::InvalidConfig`, so the assertion here is the
        // negative — first.kind must NOT be MissingField.
        Jail::expect_with(|jail| {
            jail.create_file(
                "bad-syntax.toml",
                // Unclosed string literal — guaranteed parse error.
                "cache_root = \"unterminated\nbar_cache_root = \"/b\"\noutput = \"stdout\"\n",
            )?;
            let err =
                MinerConfig::resolve(Some(Path::new("bad-syntax.toml")), CliOverrides::default())
                    .unwrap_err();
            let first = err.into_iter().next().expect("at least one error");
            assert!(
                !matches!(first.kind, Kind::MissingField(_)),
                "(c) parse error must NOT be classified as MissingField, got {:?}",
                first.kind,
            );
            Ok(())
        });
    }
}

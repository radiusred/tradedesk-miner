//! `miner` CLI parser, XDG/CWD config-path resolution, and conversion from
//! clap-parsed args to the [`miner_core::config::CliOverrides`] figment overlay.
//!
//! Phase 1 ships only the `emit-fixture` subcommand plus the four global flags
//! (`--config`, `--cache-root`, `--bar-cache-root`, `--output`); future plans will
//! add `scan` / `sweep` / `cache` subcommands without changing the global flag
//! surface.
//!
//! ## clap × figment interplay
//!
//! Each override flag also carries `env = "MINER_..."` so clap captures the env
//! value at parse time (this is what makes `--help` show env-derived defaults).
//! Figment ALSO sees the env via `Env::prefixed("MINER_").split("__")` — both
//! paths converge on the same value.
//!
//! ## Why a separate `CliOverrides` lives in miner-core
//!
//! `miner-core` does not depend on `clap` (D-16). The struct that derives
//! `clap::Parser` lives here; `Cli::overrides()` converts it into the figment
//! overlay struct that miner-core consumes.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use miner_core::config::{CliOverrides, OutputDest};

use crate::scan_args::ScanArgs;
use crate::sweep_args::SweepArgs;

/// tradedesk-miner CLI.
#[derive(Debug, Parser)]
#[command(name = "miner", version, about)]
pub struct Cli {
    /// Explicit config file path (overrides XDG / CWD lookup).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Override the source cache root (CLI > env > TOML).
    #[arg(long, global = true, env = "MINER_CACHE_ROOT")]
    pub cache_root: Option<PathBuf>,

    /// Override the derived-bar cache root (CLI > env > TOML).
    #[arg(long, global = true, env = "MINER_BAR_CACHE_ROOT")]
    pub bar_cache_root: Option<PathBuf>,

    /// Override the output destination. `stdout` for streaming JSONL on stdout
    /// (the v1 default for agent-operability); any other string is treated as
    /// a file path.
    #[arg(long, global = true, env = "MINER_OUTPUT")]
    pub output: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

/// Phase 1 ships only `emit-fixture`; Phase 3 (Plan 05) adds `scan` + `scans`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Emit one `RunStart` + one `RunEnd` fixture to stdout (Phase 1 sanity check;
    /// no scans yet).
    EmitFixture,

    /// Execute one scan invocation end-to-end (Phase 3 — `engine::run_one`).
    ///
    /// Streams `RunStart` → per-finding envelopes (`Result` / `ScanError` /
    /// `GapAborted` / `DryRun`) → `RunEnd` as JSONL on stdout. Exit code
    /// routing per CONTEXT D3-24: 0 = clean run, 1 = preflight rejection,
    /// 2 = at least one mid-stream `ScanError`, 130 = SIGINT.
    Scan(ScanArgs),

    /// List every registered scan, one JSONL line per scan validating against
    /// `schemas/scans-catalogue-v1.schema.json` (D3-20 / RESEARCH Open
    /// Question 8 resolution). Used by MCP / HTTP wrappers (Phase 6) to render
    /// per-agent catalogues without running scans.
    Scans,

    /// Execute a TOML sweep manifest end-to-end (Phase 5 / OP-04 / D5-04).
    ///
    /// Streams `RunStart` → per-job `Result` / `ScanError` / `GapAborted`
    /// envelopes → `SweepSummary` → `RunEnd` as JSONL on stdout. Exit-code
    /// routing identical to `Scan`: `0` clean, `1` preflight rejection,
    /// `2` mid-stream `ScanError`, `130` SIGINT. With `--dry-run`, emits one
    /// `DryRunFinding` with `planned_job_count` and exits 0.
    Sweep(SweepArgs),
}

impl Cli {
    /// Convert the clap-parsed flag values to a [`CliOverrides`] suitable for
    /// `miner_core::config::build_figment(.., cli)`.
    ///
    /// `output`: clap cannot parse the `OutputDest` enum directly (it is not a
    /// simple stringly-typed enum from clap's perspective), so the flag takes a
    /// `String` and we discriminate here:
    /// - `"stdout"` (case-insensitive) → `OutputDest::Stdout`
    /// - anything else → `OutputDest::File(PathBuf::from(s))`
    #[must_use]
    pub fn overrides(&self) -> CliOverrides {
        CliOverrides {
            cache_root: self.cache_root.clone(),
            bar_cache_root: self.bar_cache_root.clone(),
            output: self.output.as_deref().map(|s| {
                if s.eq_ignore_ascii_case("stdout") {
                    OutputDest::Stdout
                } else {
                    OutputDest::File(PathBuf::from(s))
                }
            }),
        }
    }
}

/// Resolve the TOML config-file path using the documented precedence (D-16):
///
/// 1. `--config <path>` if provided — used verbatim, no canonicalisation.
/// 2. Platform-native config dir via `directories::ProjectDirs::from("", "", "miner")`
///    joined with `miner.toml` if the file exists.
/// 3. `./miner.toml` (CWD fallback) if it exists.
/// 4. `None` — figment's `Toml` provider is skipped; env + CLI still apply.
///
/// ### Cross-platform note
///
/// `ProjectDirs::from("", "", "miner")` resolves to:
/// - Linux: `$XDG_CONFIG_HOME/miner/miner.toml` (or `~/.config/miner/miner.toml`).
/// - macOS: `~/Library/Application Support/miner/miner.toml`.
/// - Windows: `%APPDATA%/miner/miner.toml`.
///
/// v1 is Linux-first (`RadiusRed` deployment target); the platform-native divergence
/// is a deliberate, documented acceptable deviation rather than a bug. Phase 7
/// hardening MAY revisit if a non-Linux deployment surfaces.
///
/// ### T-01-01 mitigation
///
/// No tilde-expansion, no env-string interpolation, no symlink resolution. The
/// CLI-supplied path is passed to figment verbatim — figment's `Toml::file`
/// silently produces no values when the file is missing.
#[must_use]
pub fn resolve_toml_path(cli_explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = cli_explicit {
        return Some(p.to_path_buf());
    }
    if let Some(proj) = directories::ProjectDirs::from("", "", "miner") {
        let xdg = proj.config_dir().join("miner.toml");
        if xdg.exists() {
            return Some(xdg);
        }
    }
    let cwd = Path::new("./miner.toml");
    if cwd.exists() {
        return Some(cwd.to_path_buf());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Plan 03-05 / 04-02 acceptance: when `--gap-policy` is not supplied on
    /// a `miner scan` invocation, clap defaults it to `continuous_only` per
    /// D3-19. The legacy `--side` default was removed in Plan 04-02; side
    /// now travels inside `--instrument SYMBOL:side`.
    #[test]
    fn scan_args_defaults_per_d3_19() {
        let cli = Cli::try_parse_from([
            "miner",
            "scan",
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD:bid",
            "--timeframe",
            "15m",
            "--window",
            "2024-01-01:2024-01-02",
        ])
        .expect("clap parse ok");
        match &cli.command {
            Command::Scan(args) => {
                assert_eq!(args.instruments.len(), 1);
                assert_eq!(args.instruments[0].symbol, "EURUSD");
                assert_eq!(args.gap_policy, "continuous_only", "D3-19 default");
            }
            other => panic!("expected Command::Scan; got {other:?}"),
        }
    }
}

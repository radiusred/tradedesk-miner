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

/// Phase 1 ships only `emit-fixture`; later phases extend this enum.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Emit one `RunStart` + one `RunEnd` fixture to stdout (Phase 1 sanity check;
    /// no scans yet).
    EmitFixture,
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

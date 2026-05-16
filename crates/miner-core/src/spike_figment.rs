//! Spike module — Risk 5 / Assumption A4 verification (Plan 01-02).
//!
//! Purpose: live-verify the figment + clap `Option<T>` + `skip_serializing_if` pattern
//! with CLI-wins precedence (CLI > env > TOML > error), before Plan 05 commits the
//! production `miner-core::config` module to it.
//!
//! See 01-RESEARCH §"Common Pitfalls" Pitfall 1 (the figment + clap precedence inversion):
//! the figment docs example merges CLI FIRST, which gives the wrong precedence. We merge
//! CLI LAST and rely on `Option<T>` + `#[serde(skip_serializing_if)]` so that unset CLI
//! fields are invisible to figment and let env/TOML survive.
//!
//! **Deletion target:** Plan 05 deletes this module and the `pub mod spike_figment;`
//! re-export in `lib.rs`. The production figment builder lives in
//! `crates/miner-core/src/config/mod.rs`.

use std::path::{Path, PathBuf};

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
};
use serde::{Deserialize, Serialize};

/// Spike config — every field non-optional so `figment.extract()` returns `Err`
/// when no source supplies a value (Test 4 below). The production `MinerConfig`
/// in Plan 05 will mirror this shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpikeConfig {
    pub cache_root: PathBuf,
    pub bar_cache_root: PathBuf,
    pub output: SpikeOutputDest,
}

/// Output destination — `stdout` or a file path. Mirrors the production `OutputDest`
/// enum that Plan 05 will land in `miner-core::config::OutputDest`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpikeOutputDest {
    Stdout,
    File(PathBuf),
}

/// CLI overlay — every field `Option<T>` + `skip_serializing_if = "Option::is_none"`
/// so an absent flag is INVISIBLE to figment when this struct is fed in as the
/// final merge layer. This is the "steezeburger pattern" referenced in 01-RESEARCH
/// §Common Pitfalls Pitfall 1.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpikeCliOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bar_cache_root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<SpikeOutputDest>,
}

/// Assemble the layered figment per 01-CONTEXT D-16:
///   defaults < TOML < env (`MINER_*`) < CLI
///
/// Critical: `Serialized::defaults(cli)` is merged LAST so that CLI wins. The
/// `skip_serializing_if` on `SpikeCliOverrides` ensures unset CLI fields do not
/// shadow env/TOML.
///
/// Returns the unresolved figment so the caller can choose between `.extract()`
/// (Test 1-3, expects success) and `.extract::<SpikeConfig>()` on an empty fig
/// (Test 4, expects Err).
#[must_use]
pub fn build_figment(toml_path: Option<&Path>, cli: SpikeCliOverrides) -> Figment {
    let mut fig = Figment::new();
    if let Some(path) = toml_path {
        if path.exists() {
            fig = fig.merge(Toml::file(path));
        }
    }
    fig.merge(Env::prefixed("MINER_"))
        .merge(Serialized::defaults(cli))
}

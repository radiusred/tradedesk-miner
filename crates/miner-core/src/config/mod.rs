//! Configuration SCHEMA types (D-16).
//!
//! Plan 03 lands the type definitions only:
//! - [`MinerConfig`] — non-optional fields (`cache_root`, `bar_cache_root`, `output`).
//! - [`OutputDest`] — `Stdout` or `File(PathBuf)`.
//!
//! The figment-based config builder (`build_figment`) and `CliOverrides` overlay
//! struct land in Plan 05 using the precedence pattern verified by the Plan 01-02
//! spike (`CLI > env > TOML > error`).
//!
//! No `Default` impls. The library carries ZERO hardcoded paths — every value MUST
//! come from a config source (TOML / env / CLI) per D-16.

use std::path::PathBuf;

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 5 — `miner_config_type_shape`: confirms `MinerConfig` has the locked
    /// three-field shape and that `OutputDest` is the enum we expect. Compile-time
    /// + serde round-trip.
    #[test]
    fn miner_config_type_shape() {
        let cfg = MinerConfig {
            cache_root: PathBuf::from("/cache"),
            bar_cache_root: PathBuf::from("/bar-cache"),
            output: OutputDest::Stdout,
        };
        let json = serde_json::to_string(&cfg).expect("serialise");
        let parsed: MinerConfig =
            serde_json::from_str(&json).expect("deserialise");
        assert_eq!(parsed, cfg);

        // The File(PathBuf) variant also round-trips.
        let cfg2 = MinerConfig {
            cache_root: PathBuf::from("/c"),
            bar_cache_root: PathBuf::from("/b"),
            output: OutputDest::File(PathBuf::from("/tmp/out.jsonl")),
        };
        let json2 = serde_json::to_string(&cfg2).expect("serialise");
        let parsed2: MinerConfig =
            serde_json::from_str(&json2).expect("deserialise");
        assert_eq!(parsed2, cfg2);
    }
}

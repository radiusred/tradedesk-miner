//! [`ScanFindingShape`] — declarative `effect.extra` + `raw.series` key list.
//!
//! Pattern analog: `findings/mod.rs:200-205` ([`crate::findings::PerScanCounts`]) —
//! a tiny `Copy`-able declarative struct with `Default` for catalogue
//! introspection. `miner scans` reads one per registered scan and emits a JSONL
//! line so MCP/HTTP wrappers can render the catalogue without executing a scan.
//!
//! Plan 03-02 added `Serialize` / `Deserialize` / `JsonSchema` derives so the
//! struct can be embedded directly inside the `miner scans` catalogue JSONL line
//! AND can be the root type of the sibling `schemas/scans-catalogue-v1.schema.json`
//! (per CONTEXT D3-20 + RESEARCH Open Question 8 resolution).

use schemars::JsonSchema;
use serde::Serialize;

/// Declarative shape declaration: the `effect.extra` keys and `raw.series`
/// keys a scan WILL emit on success.
///
/// Used by:
/// - `miner scans` catalogue introspection (one JSONL line per registered scan).
/// - The per-scan integration tests (compile-time gate that the production scan
///   actually emits every declared key).
///
/// `&'static [&'static str]` because every scan's emitted-key set is a
/// compile-time constant — there is no dynamic dispatch on these values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ScanFindingShape {
    /// Names of the `effect.extra.<key>` arrays the scan emits (e.g.,
    /// `["lags", "q_stats", "p_values", "acf"]` for Ljung-Box per D3-04).
    pub effect_extra_keys: &'static [&'static str],

    /// Names of the `raw.series.<key>` arrays the scan emits (e.g.,
    /// `["returns", "timestamps_ms"]` for Ljung-Box). `timestamps_ms` MUST
    /// appear whenever `raw` is present (D-03).
    pub raw_series_keys: &'static [&'static str],
}

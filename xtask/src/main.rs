//! xtask: dev-only commands for tradedesk-miner.
//!
//! xtask is dev-only and never produces findings, so it is exempted from the
//! workspace stdout discipline (`clippy::disallowed_macros`).
//!
//! Per RESEARCH §Schema Derivation Strategy ("lint scoping nuance"): xtask runs
//! interactively on a developer's machine (or in CI for the schema-sync gate),
//! never inside a production wrapper binary, so its stderr diagnostic output
//! is allowed. The exemption is crate-level — the lint still fires on every
//! other source file in the workspace.
//!
//! Plan 06 lands the first real subcommand: `gen-schema`.
//!
//! Plan 03-02 Task 3 extends `gen-schema` to emit a SECOND schema artifact —
//! `schemas/scans-catalogue-v1.schema.json` — alongside the existing
//! `schemas/findings-v1.schema.json`. The sibling schema documents the
//! `miner scans` catalogue-line shape (per CONTEXT D3-20 + RESEARCH Open
//! Question 8). The two schemas regenerate idempotently — running gen-schema
//! twice produces no diff (the CI `git diff --exit-code schemas/` gate).
#![allow(clippy::disallowed_macros)]

use clap::{Parser, Subcommand};
use miner_core::Finding;
use miner_core::scan::ScanFindingShape;
use schemars::JsonSchema;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "xtask", about = "dev-only tasks for tradedesk-miner")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Regenerate the committed JSON Schema artifacts from miner-core types.
    ///
    /// Emits two files:
    /// 1. `schemas/findings-v1.schema.json` — the locked envelope schema (root
    ///    type `miner_core::Finding`).
    /// 2. `schemas/scans-catalogue-v1.schema.json` — the sibling schema for
    ///    `miner scans` introspection lines (root type
    ///    `ScansCatalogueEntry`, an xtask-local shim wrapping the
    ///    `scan_id`/`version`/`params`/`finding_fields` shape per CONTEXT D3-20).
    GenSchema {
        /// Directory to write schema files into (default: schemas/).
        ///
        /// Both `findings-v1.schema.json` and `scans-catalogue-v1.schema.json`
        /// land here. Override only when running against a non-standard layout
        /// (e.g., a test fixture).
        #[arg(default_value = "schemas")]
        out_dir: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::GenSchema { out_dir } => gen_schema(&out_dir),
    }
}

/// xtask-local shim describing one line of `miner scans` introspection output
/// per CONTEXT D3-20:
///
/// ```text
/// {"scan_id":"stats.autocorr.ljung_box","version":1,
///  "params":{...JSON Schema fragment...},
///  "finding_fields":{"effect_extra_keys":[...],"raw_series_keys":[...]}}
/// ```
///
/// Lives inside xtask (not `miner-core::scan`) because nothing in the runtime
/// engine constructs `ScansCatalogueEntry` — it is purely the wire shape the
/// `miner scans` subcommand emits via `FindingSink::write_raw_json` (Plan 05
/// wires the subcommand body). Per RESEARCH Open Question 8 resolution, the
/// sibling schema documents this shape so MCP/HTTP wrappers in Phase 6 can
/// validate catalogue lines without coupling to `miner-core`'s internal types.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[allow(dead_code)]
struct ScansCatalogueEntry {
    /// Stable scan id — `<family>.<subfamily>.<scan_name>` (D3-17).
    scan_id: String,
    /// Major version of the scan's output shape.
    version: u32,
    /// JSON Schema fragment for the scan's `--params` (the
    /// `Scan::param_schema()` output, embedded verbatim).
    params: serde_json::Value,
    /// Declarative `effect.extra` + `raw.series` key list (the
    /// `Scan::finding_fields()` output, embedded verbatim).
    finding_fields: ScanFindingShape,
}

/// Regenerate the committed JSON Schema artifacts from `miner_core::Finding`
/// + `ScansCatalogueEntry`.
///
/// Determinism pipeline (`PLAN 06` `must_haves` — three compounding guarantees,
/// inherited unchanged from Plan 06):
///
/// 1. `schemars 1.x`'s `schema_for!` walks derive types in a stable order
///    (verified by Plan 01-02 spike A1).
/// 2. The intermediate `serde_json::Value` is `BTreeMap`-backed because the
///    workspace pins `serde_json = "1"` without the `preserve_order` feature
///    (see workspace `Cargo.toml` determinism note + Plan 01-01).
/// 3. `to_string_pretty` emits keys in `BTreeMap` (alphabetic) order, collapsing
///    any residual non-determinism from schemars' internal map.
///
/// Running this function twice in succession MUST produce byte-identical
/// output across BOTH artifacts; Task 3's `<verify>` block enforces this via a
/// `git diff --exit-code schemas/` gate after the second invocation. If the diff
/// is non-zero on either artifact, the determinism pipeline has failed and Plan
/// 03-02 Task 3 is incomplete — investigate which step is reintroducing
/// non-determinism.
fn gen_schema(out_dir: &Path) -> anyhow::Result<()> {
    if !out_dir.as_os_str().is_empty() {
        std::fs::create_dir_all(out_dir)?;
    }

    write_schema::<Finding>(out_dir.join("findings-v1.schema.json").as_path())?;
    write_schema::<ScansCatalogueEntry>(
        out_dir.join("scans-catalogue-v1.schema.json").as_path(),
    )?;
    Ok(())
}

/// Emit `schemars::schema_for!(T)` to `path`, pretty-printed with stable key
/// ordering (BTreeMap-backed serde_json::Map per workspace Cargo.toml).
fn write_schema<T: JsonSchema>(path: &Path) -> anyhow::Result<()> {
    let schema = schemars::schema_for!(T);
    let as_value: serde_json::Value = serde_json::to_value(&schema)?;
    let pretty = serde_json::to_string_pretty(&as_value)?;
    std::fs::write(path, format!("{pretty}\n"))?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

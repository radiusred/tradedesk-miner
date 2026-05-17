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
#![allow(clippy::disallowed_macros)]

use clap::{Parser, Subcommand};
use miner_core::Finding;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "xtask", about = "dev-only tasks for tradedesk-miner")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Regenerate `schemas/findings-v1.schema.json` from miner-core types.
    GenSchema {
        /// Path to write the generated schema file (default: schemas/findings-v1.schema.json).
        #[arg(default_value = "schemas/findings-v1.schema.json")]
        out: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::GenSchema { out } => gen_schema(&out),
    }
}

/// Regenerate the committed JSON Schema artifact from `miner_core::Finding`.
///
/// Determinism pipeline (`PLAN 06` `must_haves` — three compounding guarantees):
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
/// output; Task 1's `<verify>` block enforces this via a `cmp` against a
/// second invocation. If the diff is non-zero, the determinism pipeline has
/// failed and Plan 06 is incomplete — investigate which step is reintroducing
/// non-determinism.
fn gen_schema(out: &PathBuf) -> anyhow::Result<()> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let schema = schemars::schema_for!(Finding);
    let as_value: serde_json::Value = serde_json::to_value(&schema)?;
    let pretty = serde_json::to_string_pretty(&as_value)?;
    std::fs::write(out, format!("{pretty}\n"))?;
    eprintln!("wrote {}", out.display());
    Ok(())
}

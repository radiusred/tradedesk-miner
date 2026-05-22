// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Radius Red Ltd.

//! Recipe runner for the Phase 7 bench harness (Plan 07-08).
//!
//! Reads a TOML `SweepManifest` from `--recipe <path>`, executes it
//! in-process via [`miner_core::sweep::run_sweep`], and emits exactly ONE
//! JSON timing object on stdout. Tracing logs go to stderr — the stdout
//! discipline (workspace `clippy::disallowed_macros` ban on `print` /
//! `eprint` / `dbg` macros) is enforced at build time.
//!
//! Stdout shape (one line, JSONL):
//!
//! ```json
//! {"recipe":"benches/recipes/single-job.toml",
//!  "wall_clock_ms":123,
//!  "total_findings":42,
//!  "scan_errors":0,
//!  "warmup":0,"runs":1}
//! ```
//!
//! The `--warmup` / `--runs` knobs are surfaced for `hyperfine`
//! self-documentation (`miner-bench --help` is the discovery surface for
//! reproduction) but are no-ops inside the binary itself — hyperfine
//! handles warmup + multi-run statistics externally
//! (RESEARCH §"Open Question 3").
//!
//! The `dhat` Cargo feature (off by default) installs `dhat::Alloc` as the
//! global allocator and writes `dhat-heap.json` to CWD at process exit
//! (RESEARCH §"Pattern 6"). FOUND-04 is preserved because the feature
//! lives ONLY on this crate — `miner-core` stays dhat-free and tokio-free.
//!
//! ## Environment
//!
//! The runner constructs its `MinerConfig` via `MinerConfig::resolve(None,
//! CliOverrides::default())`, which means values must come from environment
//! variables (the figment precedence is TOML < env < CLI, and the runner
//! does NOT carry a TOML path or CLI override surface — only `--recipe`).
//! Required env vars:
//!
//! | Var                    | Field           |
//! |------------------------|-----------------|
//! | `MINER_CACHE_ROOT`     | `cache_root`    |
//! | `MINER_BAR_CACHE_ROOT` | `bar_cache_root`|
//! | `MINER_OUTPUT`         | `output`        |
//!
//! For local fixture-cache runs:
//!
//! ```sh
//! MINER_CACHE_ROOT=./tests/fixtures/cache \
//! MINER_BAR_CACHE_ROOT=/tmp/bar \
//! MINER_OUTPUT=stdout \
//! cargo run --release -p miner-bench --bin miner-bench -- \
//!     --recipe benches/recipes/single-job.toml
//! ```

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use miner_core::cache::BarCache;
use miner_core::config::MinerConfig;
use miner_core::error::MinerError;
use miner_core::findings::{Finding, FindingSink};
use miner_core::sweep::{SweepOptions, manifest::parse_manifest_str, run_sweep};
use miner_reader_dukascopy::DukascopyReader;

// ---------------------------------------------------------------------------
// dhat global allocator — feature-gated.
//
// Per RESEARCH §"Pattern 6": ONLY active under `--features dhat`. Default
// release builds (cargo install, distribution, CI's `cargo test --workspace`)
// use the system allocator. This preserves FOUND-04: miner-core is dhat-free
// + tokio-free in every build mode.
// ---------------------------------------------------------------------------
#[cfg(feature = "dhat")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// CLI surface for the recipe runner.
///
/// `--warmup` / `--runs` are documented no-ops at the binary level —
/// hyperfine's `--warmup N --runs N` flags drive repetition externally. We
/// surface them on the binary so that `miner-bench --help` is the canonical
/// reproduction documentation and so future in-binary repetition (if ever
/// added) doesn't require a CLI-shape change.
#[derive(Parser, Debug)]
#[command(name = "miner-bench", about = "Recipe runner for the Phase 7 bench harness")]
struct Args {
    /// Path to a TOML `SweepManifest` (e.g. `benches/recipes/single-job.toml`).
    #[arg(long)]
    recipe: PathBuf,

    /// Documentation-only hyperfine warmup count. The binary ignores this
    /// value; hyperfine handles warmup externally via its own `--warmup` flag.
    #[arg(long, default_value_t = 0)]
    warmup: u32,

    /// Documentation-only hyperfine run count. The binary executes exactly
    /// once per invocation; hyperfine handles repetition externally.
    #[arg(long, default_value_t = 1)]
    runs: u32,
}

/// In-process counting sink that tallies findings by variant for the
/// stdout JSON summary. Mirrors `StdoutSink`'s framing semantics
/// (per-envelope flush is a no-op for in-memory) but discards the bytes —
/// we want the timing summary on stdout, not a JSONL stream of every
/// finding the sweep emits.
///
/// Counters tracked:
/// - `total_findings`: count of `Finding::Result` envelopes (the
///   value-producing variant per `crates/miner-core/src/findings/mod.rs`).
/// - `scan_errors`: count of `Finding::ScanError` envelopes.
///
/// Other variants (`RunStart`, `RunEnd`, `GapAborted`, `DryRun`,
/// `SweepSummary`) are observed but not exposed in the summary —
/// they're framing / metadata, not "work product".
#[derive(Default)]
struct CountingSink {
    total_findings: u64,
    scan_errors: u64,
}

impl FindingSink for CountingSink {
    fn write_envelope(&mut self, finding: &Finding) -> std::result::Result<(), MinerError> {
        match finding {
            Finding::Result(_) => self.total_findings += 1,
            Finding::ScanError(_) => self.scan_errors += 1,
            // Framing + metadata variants are silently counted out — the
            // sweep emits exactly one RunStart + one RunEnd + (optionally)
            // one SweepSummary; bench timings are about scan throughput,
            // not framing overhead.
            _ => {}
        }
        Ok(())
    }

    fn write_raw_json(&mut self, _v: &serde_json::Value) -> std::io::Result<()> {
        // No raw-json passthrough in the sweep path — the engine only ever
        // writes typed envelopes through `write_envelope` during a sweep
        // (`miner scans` is the only `write_raw_json` consumer, not relevant
        // here).
        Ok(())
    }

    fn flush(&mut self) -> std::result::Result<(), MinerError> {
        Ok(())
    }
}

fn main() -> Result<()> {
    // dhat profiler lifetime: drop on Ok-exit writes `dhat-heap.json` to CWD.
    // Bound to `_profiler` so it lives for the whole `main`; per RESEARCH
    // §"Pattern 6", the destructor on drop is what triggers the JSON write,
    // so `let _ = dhat::Profiler::new_heap()` would be WRONG (drops
    // immediately).
    #[cfg(feature = "dhat")]
    let _profiler = dhat::Profiler::new_heap();

    // Per D-15 / Pattern 5: tracing → stderr ALWAYS, before any other IO.
    // Stdout is reserved for the single JSON timing line emitted at the end.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    tracing::debug!(?args, "miner-bench args parsed");

    // ----------------------------------------------------------------------
    // Step 1 — Read + parse the recipe TOML.
    // ----------------------------------------------------------------------
    let toml_str = std::fs::read_to_string(&args.recipe)
        .with_context(|| format!("read recipe: {}", args.recipe.display()))?;
    let manifest = parse_manifest_str(&toml_str)
        .with_context(|| format!("parse recipe TOML: {}", args.recipe.display()))?;
    tracing::info!(
        recipe = %args.recipe.display(),
        jobs = manifest.jobs.len(),
        "recipe loaded",
    );

    // ----------------------------------------------------------------------
    // Step 2 — Construct MinerConfig from env (TOML/CLI not exposed at this
    // binary's surface; figment's env layer picks up MINER_CACHE_ROOT /
    // MINER_BAR_CACHE_ROOT / MINER_OUTPUT).
    // ----------------------------------------------------------------------
    let cfg = MinerConfig::resolve(None, miner_core::config::CliOverrides::default())
        .context("resolve MinerConfig from environment (need MINER_CACHE_ROOT, MINER_BAR_CACHE_ROOT, MINER_OUTPUT)")?;
    tracing::debug!(?cfg, "miner config resolved");

    // ----------------------------------------------------------------------
    // Step 3 — Construct reader + bar cache at the binary edge (RESEARCH
    // Open Question 6 — readers are NEVER carried by MinerConfig; the
    // binary owns construction).
    // ----------------------------------------------------------------------
    let reader = DukascopyReader::new(cfg.cache_root.clone());
    let bar_cache = BarCache::new(cfg.bar_cache_root.clone());

    // ----------------------------------------------------------------------
    // Step 4 — SIGINT handler installed BEFORE invoking the sweep (Pitfall 2
    // / D3-22 — same convention the CLI follows). The handler closure logs
    // via tracing::warn; clippy bans the convenience stderr macros.
    // ----------------------------------------------------------------------
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        // ctrlc::set_handler returns Err if a handler was already set in
        // this process — ignore (.ok()) so re-spawning the binary inside a
        // host that already installed one doesn't crash. The downstream
        // run_sweep polls the same `cancel` flag.
        ctrlc::set_handler(move || {
            cancel.store(true, Ordering::SeqCst);
            tracing::warn!("SIGINT received; cooperative shutdown requested");
        })
        .ok();
    }

    // ----------------------------------------------------------------------
    // Step 5 — Time the sweep. Sweep options default (no dry-run, no
    // sleep-hook). The counting sink tallies findings without polluting
    // stdout.
    // ----------------------------------------------------------------------
    let mut sink = CountingSink::default();
    let opts = SweepOptions::default();
    let start = Instant::now();
    run_sweep(
        manifest,
        opts,
        &cfg,
        &reader,
        &bar_cache,
        &mut sink,
        Arc::clone(&cancel),
    )
    .context("run_sweep")?;
    let elapsed_ms = start.elapsed().as_millis();

    // ----------------------------------------------------------------------
    // Step 6 — Emit ONE JSON timing line to stdout. Pattern I discipline:
    // `serde_json::to_writer(io::stdout().lock(), ...)` + manual `\n` —
    // NEVER the `print` macro. The lock is held for both writes so they appear as
    // a single line.
    // ----------------------------------------------------------------------
    let summary = serde_json::json!({
        "recipe": args.recipe.to_string_lossy(),
        "wall_clock_ms": elapsed_ms,
        "total_findings": sink.total_findings,
        "scan_errors": sink.scan_errors,
        "warmup": args.warmup,
        "runs": args.runs,
    });
    {
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        serde_json::to_writer(&mut out, &summary).context("serialize summary to stdout")?;
        out.write_all(b"\n").context("trailing newline to stdout")?;
        out.flush().context("flush stdout")?;
    }

    Ok(())
}

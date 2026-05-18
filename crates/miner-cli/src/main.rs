//! `miner` CLI binary — Plan 03-05 Task 2.
//!
//! Wires the full Phase 3 contract surface end-to-end:
//!
//! 1. **SIGINT handler installed BEFORE `Cli::parse()`** (Pitfall 2 / D3-22).
//!    `ctrlc::set_handler` registers a closure that flips a shared
//!    `Arc<AtomicBool>` cancellation flag the facade polls between findings.
//!    The closure logs via `tracing::warn` — NEVER via the convenience stderr
//!    macros (banned by the workspace clippy gate).
//! 2. `tracing-subscriber` → `io::stderr()` with `EnvFilter` honouring `RUST_LOG`
//!    (default `info`). Findings go to stdout; logs go to stderr (D-15, D-19).
//! 3. `clap::Parser` parses the CLI; `--config` / `--cache-root` /
//!    `--bar-cache-root` / `--output` are global flags; subcommands are
//!    `emit-fixture`, `scan <args>`, `scans`.
//! 4. `resolve_toml_path` + `MinerConfig::resolve` chain produce the typed
//!    config; preflight errors (figment) emit a structured `WireError` JSON
//!    line to stderr and exit 1 (D-06).
//! 5. `make_sink(&cfg.output)` dispatches on the resolved `OutputDest`.
//! 6. The `Scan(args)` arm constructs a `DukascopyReader` at the binary edge
//!    (RESEARCH Open Question 6 — readers are NOT carried by `MinerConfig`,
//!    they are constructed at the CLI boundary), calls
//!    `engine::run_one(req, cfg, &reader, sink, cancel)`, and routes the
//!    returned `RunOutcome` through `compute_exit_code` (CONTEXT D3-24):
//!    `Ok→0`, `PreflightFailed→1`, `HadScanErrors→2`, `cancel→130`.
//! 7. The `Scans` arm enumerates the `miner_core::scan::bootstrap()` registry
//!    and emits one JSONL line per scan via `FindingSink::write_raw_json`
//!    (CONTEXT D3-20 / RESEARCH Open Question 8 resolution).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use miner_core::config::{MinerConfig, OutputDest};
use miner_core::engine::{RunOutcome, run_one};
use miner_core::error::stderr_emit::emit_to_stderr;
use miner_core::error::{PreflightCode, WireError};
use miner_core::findings::sink::{FileSink, StdoutSink};
use miner_core::findings::{Finding, FindingSink, RunEnd, RunId, RunStart, RunSummary};
use miner_reader_dukascopy::DukascopyReader;
use tracing_subscriber::EnvFilter;

mod cli;
mod scan_args;

use cli::{Cli, Command, resolve_toml_path};
use scan_args::ScanArgs;

fn main() -> anyhow::Result<()> {
    // -----------------------------------------------------------------------
    // Step 1 — ctrlc handler installed BEFORE Cli::parse (Pitfall 2 / D3-22).
    //
    // The handler closure logs via tracing::warn (NOT the banned convenience
    // stderr macros — workspace clippy.toml bans them). The Arc<AtomicBool>
    // is the cooperative cancellation flag the facade polls. Install order is
    // critical: a SIGINT that arrives before the handler is registered would
    // hit Rust's default signal disposition (immediate exit). With the
    // handler in place first, the flag flips and the facade exits cleanly
    // through RunEnd framing.
    // -----------------------------------------------------------------------
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        ctrlc::set_handler(move || {
            cancel.store(true, Ordering::SeqCst);
            tracing::warn!("SIGINT received; cooperative shutdown requested");
        })
        .expect("ctrlc handler install");
    }

    // Per Pattern 5 / D-15: tracing → stderr ALWAYS, BEFORE any other I/O.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let parsed = Cli::parse();
    tracing::debug!(?parsed, "cli parsed");

    // Resolve TOML config path: --config > XDG > CWD > None.
    let toml_path = resolve_toml_path(parsed.config.as_deref());
    tracing::debug!(?toml_path, "config file path resolved");

    // Build figment + extract MinerConfig; on failure, emit structured WireError
    // to stderr with the correctly-classified PreflightCode and exit 1 (D-06).
    let cfg: MinerConfig = match MinerConfig::resolve(toml_path.as_deref(), parsed.overrides()) {
        Ok(c) => c,
        Err(e) => {
            let code = classify_figment_error(&e);
            let err = WireError::preflight(code, e.to_string());
            // Best-effort emit; if stderr is broken, we still exit 1.
            let _ = emit_to_stderr(&err);
            std::process::exit(1);
        }
    };

    // Construct the resolved sink from `cfg.output`. Failure to open a file sink
    // is a runtime IO error, not a preflight config error — we surface it via the
    // anyhow chain so the operator gets a non-zero exit and a readable stderr line
    // (tracing layer captures it).
    let mut sink: Box<dyn FindingSink> = make_sink(&cfg.output)?;

    match parsed.command {
        Command::EmitFixture => emit_fixture(&mut *sink)?,
        Command::Scans => handle_scans_subcommand(&mut *sink)?,
        Command::Scan(scan_args) => {
            let outcome = handle_scan_subcommand(scan_args, &cfg, &mut *sink, Arc::clone(&cancel))?;
            let code = compute_exit_code(cancel.load(Ordering::SeqCst), &outcome);
            std::process::exit(code);
        }
    }

    Ok(())
}

/// Dispatch on the resolved [`OutputDest`] to construct the production sink.
///
/// `OutputDest::Stdout` → [`StdoutSink`] (the v1 default; D-19 single sanctioned
/// stdout writer). `OutputDest::File(path)` → [`FileSink`] opened in
/// `create + append` mode at `path`. Both implementations share JSONL framing
/// and per-envelope flush semantics so the wire output is byte-identical across
/// destinations (modulo the volatile `run_id` / timestamp fields).
///
/// # Errors
/// Returns [`anyhow::Error`] wrapping the underlying IO failure if the file path
/// cannot be opened (missing parent directory, permission denied, etc.).
fn make_sink(dest: &OutputDest) -> anyhow::Result<Box<dyn FindingSink>> {
    match dest {
        OutputDest::Stdout => Ok(Box::new(StdoutSink::new())),
        OutputDest::File(path) => {
            let sink = FileSink::create(path)
                .map_err(|e| anyhow::anyhow!("opening output file {}: {e}", path.display()))?;
            Ok(Box::new(sink))
        }
    }
}

/// Inspect a `figment::Error` and return the appropriate [`PreflightCode`].
///
/// `MissingField` → [`PreflightCode::MissingRequiredConfig`]; every other variant
/// (type / value mismatches, parse errors, unknown fields/variants, OOR integers,
/// unsupported types) → [`PreflightCode::InvalidConfig`].
///
/// Mapping every error to `MissingRequiredConfig` is FORBIDDEN — downstream
/// agents would mis-classify the failure. Plan 05 Task 1 Test 7 locks the
/// contract this function depends on.
fn classify_figment_error(err: &figment::Error) -> PreflightCode {
    use figment::error::Kind;
    // `figment::Error` is iterable over potentially multiple inner errors;
    // we classify on the FIRST one (the proximate cause).
    let first_kind = err
        .clone()
        .into_iter()
        .next()
        .map_or(Kind::Message(String::new()), |e| e.kind);
    match first_kind {
        Kind::MissingField(_) => PreflightCode::MissingRequiredConfig,
        Kind::InvalidType(_, _)
        | Kind::InvalidValue(_, _)
        | Kind::InvalidLength(_, _)
        | Kind::Message(_)
        | Kind::UnknownVariant(_, _)
        | Kind::UnknownField(_, _)
        | Kind::DuplicateField(_)
        | Kind::ISizeOutOfRange(_)
        | Kind::USizeOutOfRange(_)
        | Kind::Unsupported(_)
        | Kind::UnsupportedKey(_, _) => PreflightCode::InvalidConfig,
    }
}

/// `emit-fixture` subcommand: one `RunStart` + one `RunEnd` written through the
/// resolved sink (constructed by [`make_sink`] from the post-precedence
/// `MinerConfig::output`). Both records share the same `RunId` (relies on `Copy`).
///
/// The sink is passed by `&mut dyn FindingSink` so the same code path handles
/// `StdoutSink` and `FileSink` (and any future sink) identically — the JSONL
/// framing + per-envelope flush guarantees come from the sink implementation, not
/// from this function.
fn emit_fixture(sink: &mut dyn FindingSink) -> anyhow::Result<()> {
    tracing::info!("emitting fixture");
    let run_id = RunId::new();
    let started = chrono::Utc::now();

    let start = Finding::RunStart(RunStart {
        run_id, // RunId: Copy — moved into RunStart here ...
        started_at_utc: started,
        miner_version: env!("CARGO_PKG_VERSION").to_string(),
        code_revision: miner_core::CODE_REVISION.to_string(),
        request: serde_json::json!({ "command": "emit-fixture" }),
    });
    sink.write_envelope(&start)?;

    let ended = chrono::Utc::now();
    let end = Finding::RunEnd(RunEnd {
        run_id, // ... and again into RunEnd: only legal because RunId is Copy.
        ended_at_utc: ended,
        wall_clock_ms: ended.signed_duration_since(started).num_milliseconds(),
        summary: RunSummary::default(),
    });
    sink.write_envelope(&end)?;

    sink.flush()?;
    Ok(())
}

/// `miner scans` subcommand — emits one JSONL catalogue line per registered
/// scan (CONTEXT D3-20 / RESEARCH Open Question 8 resolution).
///
/// Each line carries `scan_id`, `version`, `params` (the scan's JSON Schema
/// fragment), and `finding_fields` (the declarative emit-shape). Lines
/// validate against `schemas/scans-catalogue-v1.schema.json` — NOT against
/// `schemas/findings-v1.schema.json` (the lines are NOT findings; they bypass
/// the Finding-envelope discipline via `FindingSink::write_raw_json`, per
/// PATTERNS line 1183 and RESEARCH Pitfall 7).
///
/// MCP / HTTP wrappers (Phase 6) render their per-agent catalogues by calling
/// the same code path so the wire output is byte-identical across transports.
///
/// # Errors
/// Returns the underlying `std::io::Error` if the sink fails (broken pipe,
/// disk full, etc.).
fn handle_scans_subcommand(sink: &mut dyn FindingSink) -> std::io::Result<()> {
    let registry = miner_core::scan::bootstrap();
    for scan in registry.iter() {
        let line = serde_json::json!({
            "scan_id": scan.id(),
            "version": scan.version(),
            "params": scan.param_schema(),
            "finding_fields": {
                "effect_extra_keys": scan.finding_fields().effect_extra_keys,
                "raw_series_keys": scan.finding_fields().raw_series_keys,
            }
        });
        sink.write_raw_json(&line)?;
    }
    // Final flush — the per-envelope StdoutSink/FileSink flush already
    // happened inside write_raw_json, but this is the contract close.
    sink.flush()
        .map_err(|e| std::io::Error::other(format!("sink flush: {e}")))?;
    Ok(())
}

/// `miner scan <args>` subcommand — translate clap-parsed `ScanArgs` into a
/// typed `ScanRequest`, construct a `DukascopyReader` at the binary edge, and
/// hand off to `engine::run_one`.
///
/// **Pitfall 8 wiring (Blocker 1):** The cfg-gated
/// `--sleep-after-first-finding-ms` value forwarded by
/// `ScanArgs::to_scan_request` already lives on
/// `ScanRequest.sleep_after_first_finding_ms` by the time `run_one` is called;
/// no additional cfg-divergent forwarding is required here. The wiring is
/// verified by `handle_scan_subcommand_forwards_sleep_hook_to_scan_request`
/// below (test-only seam: `build_scan_request_for_tests`).
///
/// # Errors
/// Returns the underlying `WireError`-wrapped failure on preflight rejection
/// (mapped to `RunOutcome::PreflightFailed`) or an `anyhow::Error` on
/// reader / engine I/O failure.
#[allow(
    clippy::needless_pass_by_value,
    reason = "ScanArgs is built by clap and consumed once at the call site; passing by value matches the Subcommand variant's owned `ScanArgs` shape and keeps the binary `main()` ergonomic"
)]
fn handle_scan_subcommand(
    args: ScanArgs,
    cfg: &MinerConfig,
    sink: &mut dyn FindingSink,
    cancel: Arc<AtomicBool>,
) -> anyhow::Result<RunOutcome> {
    // Step 1 — boundary preflight (typed ScanArgs → typed ScanRequest). On
    // WireError, emit the structured JSON line to stderr and signal
    // PreflightFailed to the exit-code router.
    let req = match args.to_scan_request(miner_core::CODE_REVISION) {
        Ok(r) => r,
        Err(wire_err) => {
            let _ = emit_to_stderr(&wire_err);
            return Ok(RunOutcome::PreflightFailed);
        }
    };

    // Step 2 — build a DukascopyReader at the binary edge (RESEARCH Open
    // Question 6 — readers are NEVER carried by MinerConfig; the CLI owns
    // construction).
    let reader = DukascopyReader::new(cfg.cache_root.clone());

    // Step 3 — call the facade. The Result<RunOutcome, MinerError> propagates
    // through anyhow; preflight unknown-scan errors arrive here as
    // Err(MinerError::Scan(_)) per Plan 04's run_one contract, which we
    // demote to PreflightFailed.
    match run_one(&req, cfg, &reader, sink, cancel) {
        Ok(outcome) => Ok(outcome),
        Err(miner_core::error::MinerError::Scan(msg)) if msg.starts_with("unknown scan:") => {
            // Map the run_one preflight rejection into a structured WireError
            // on stderr; stdout stays empty (T-01-03 stdout discipline).
            let err = WireError::preflight(PreflightCode::UnknownScan, msg);
            let _ = emit_to_stderr(&err);
            Ok(RunOutcome::PreflightFailed)
        }
        Err(e) => Err(anyhow::anyhow!("engine::run_one: {e}")),
    }
}

/// Compute the POSIX exit code from the cancellation flag + the facade's
/// returned `RunOutcome` per CONTEXT D3-24.
///
/// | Cancelled? | `RunOutcome`        | Exit |
/// |-----------:|--------------------|-----:|
/// | true       | _any_              |  130 |
/// | false      | `PreflightFailed`  |    1 |
/// | false      | `HadScanErrors`    |    2 |
/// | false      | `Ok`               |    0 |
#[must_use]
fn compute_exit_code(cancelled: bool, outcome: &RunOutcome) -> i32 {
    if cancelled {
        return 130;
    }
    match outcome {
        RunOutcome::PreflightFailed => 1,
        RunOutcome::HadScanErrors => 2,
        RunOutcome::Ok => 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use miner_core::error::MinerError;

    /// Local test-only sink (miner-core's `VecSink` is `cfg(test)`-gated to
    /// that crate, so it's unreachable from this binary's tests). Mirrors
    /// `StdoutSink`'s byte-level framing exactly: one JSON object per
    /// `write_envelope` / `write_raw_json` call followed by `\n` (no
    /// per-call flush needed for in-memory).
    #[derive(Default)]
    struct TestSink(Vec<u8>);

    impl FindingSink for TestSink {
        fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError> {
            let bytes = serde_json::to_vec(finding).map_err(MinerError::Serialize)?;
            self.0.extend_from_slice(&bytes);
            self.0.push(b'\n');
            Ok(())
        }
        fn write_raw_json(&mut self, v: &serde_json::Value) -> std::io::Result<()> {
            let bytes = serde_json::to_vec(v).map_err(std::io::Error::other)?;
            self.0.extend_from_slice(&bytes);
            self.0.push(b'\n');
            Ok(())
        }
        fn flush(&mut self) -> Result<(), MinerError> {
            Ok(())
        }
    }

    /// Pitfall 2 source-inspection gate: the `ctrlc::set_handler` CALL site
    /// MUST appear in this file BEFORE the `Cli::parse()` CALL site. We read
    /// this very file at compile time via `include_str!` and compare byte
    /// offsets, anchoring the searches on the canonical call expressions
    /// (`ctrlc::set_handler(` and `let parsed = Cli::parse()`) so doc-comment
    /// occurrences don't skew the result.
    #[test]
    fn main_installs_ctrlc_before_parse() {
        let src = include_str!("main.rs");
        let ctrlc_idx = src
            .find("ctrlc::set_handler(")
            .expect("ctrlc::set_handler call present");
        let parse_idx = src
            .find("let parsed = Cli::parse()")
            .expect("Cli::parse() call site present");
        assert!(
            ctrlc_idx < parse_idx,
            "ctrlc::set_handler (at byte {ctrlc_idx}) must appear BEFORE Cli::parse() (at byte {parse_idx}) — Pitfall 2 / D3-22"
        );
    }

    /// Exit-code routing covers the four-tier matrix from CONTEXT D3-24.
    #[test]
    fn exit_code_routing_all_four_tiers() {
        // SIGINT overrides every outcome → 130.
        assert_eq!(compute_exit_code(true, &RunOutcome::Ok), 130);
        assert_eq!(compute_exit_code(true, &RunOutcome::HadScanErrors), 130);
        assert_eq!(compute_exit_code(true, &RunOutcome::PreflightFailed), 130);
        // No cancel: outcome maps to code.
        assert_eq!(compute_exit_code(false, &RunOutcome::Ok), 0);
        assert_eq!(compute_exit_code(false, &RunOutcome::HadScanErrors), 2);
        assert_eq!(compute_exit_code(false, &RunOutcome::PreflightFailed), 1);
    }

    /// `miner scans` emits ONE JSONL line per registered scan; for Phase 3
    /// that is exactly one line (`stats.autocorr.ljung_box@1`). Validates the
    /// catalogue-line shape carries the four required properties (`scan_id`,
    /// `version`, `params`, `finding_fields`).
    #[test]
    fn handle_scans_subcommand_emits_one_line_per_registered_scan_via_vec_sink() {
        let mut sink = TestSink::default();
        handle_scans_subcommand(&mut sink).expect("scans handler ok");
        // Split into JSONL lines.
        let lines: Vec<&[u8]> = sink
            .0
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .collect();
        // Phase 3 ships exactly one registered scan via Registry::bootstrap.
        let expected = miner_core::scan::bootstrap().iter().count();
        assert_eq!(
            lines.len(),
            expected,
            "expected one JSONL line per registered scan; got {} for {} scans",
            lines.len(),
            expected
        );
        assert!(expected >= 1, "Phase 3 ships at least one scan");
        // Parse each line and assert the four required properties.
        for line in &lines {
            let v: serde_json::Value = serde_json::from_slice(line).expect("line parses as JSON");
            for key in ["scan_id", "version", "params", "finding_fields"] {
                assert!(
                    v.get(key).is_some(),
                    "catalogue line missing required key {key:?}: {v}"
                );
            }
            // Spot-check the Phase 3 catalogue line's scan_id.
            assert_eq!(v["scan_id"], "stats.autocorr.ljung_box");
            assert_eq!(v["version"], 1);
        }
    }

    /// Test-only seam — Blocker 1 / Pitfall 8 ingress sanity check.
    ///
    /// Confirms that under `cfg(test)` (which makes the cfg-gated field
    /// reachable end-to-end via the dev-dep `miner-core/test-internal`
    /// feature) the value supplied via `--sleep-after-first-finding-ms` lands
    /// on `ScanRequest.sleep_after_first_finding_ms` by the time the engine
    /// boundary sees it. We do NOT depend on the engine actually performing
    /// the sleep — Plan 03-06's `sigint_preserves_stream` integration test is
    /// the end-to-end gate.
    #[test]
    #[cfg(any(test, feature = "test-internal"))]
    fn handle_scan_subcommand_forwards_sleep_hook_to_scan_request() {
        use clap::Parser;
        // Build the same ScanArgs handle_scan_subcommand would receive.
        let cli = Cli::try_parse_from([
            "miner",
            "scan",
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD",
            "--timeframe",
            "15m",
            "--window",
            "2024-01-01:2024-01-02",
            "--sleep-after-first-finding-ms",
            "2000",
        ])
        .expect("clap parse ok");
        let args = match cli.command {
            Command::Scan(a) => a,
            other => panic!("expected Command::Scan; got {other:?}"),
        };
        // The test-only seam: directly call `to_scan_request` (which
        // `handle_scan_subcommand` invokes as its first step) and inspect the
        // constructed ScanRequest. This is the exact path the engine boundary
        // sees — no decoder magic.
        let req = args
            .to_scan_request(miner_core::CODE_REVISION)
            .expect("preflight ok");
        assert_eq!(
            req.sleep_after_first_finding_ms,
            Some(2000),
            "sleep hook must reach ScanRequest (Blocker 1 — Pitfall 8 ingress wiring)"
        );
    }
}

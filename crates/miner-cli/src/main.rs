//! `miner` CLI binary — Plan 05.
//!
//! Wires the full Phase 1 contract surface end-to-end:
//!
//! 1. `tracing-subscriber` → `io::stderr()` with `EnvFilter` honouring `RUST_LOG`
//!    (default `info`). Findings go to stdout; logs go to stderr (D-15, D-19).
//! 2. `clap::Parser` parses the CLI; `--config` / `--cache-root` / `--bar-cache-root` /
//!    `--output` are global flags; subcommands are `emit-fixture` for now.
//! 3. `resolve_toml_path` resolves the config file via `--config` > XDG > CWD > None.
//! 4. `MinerConfig::resolve(toml_path, cli.overrides())` builds the figment and
//!    extracts the typed config. CLI > env > TOML > error precedence holds.
//! 5. On `figment::Error`, `classify_figment_error` maps `Kind::MissingField` →
//!    `PreflightCode::MissingRequiredConfig` and everything else →
//!    `PreflightCode::InvalidConfig`. `emit_to_stderr` writes a single `WireError`
//!    JSON line to stderr and we `std::process::exit(1)` (D-06, D-07).
//! 6. On success, the `EmitFixture` handler creates one `RunStart` + one `RunEnd`,
//!    sharing the same `RunId` (relies on `Copy` derive from Plan 03), and writes
//!    them via `StdoutSink`. Exit code 0.

use clap::Parser;
use miner_core::config::MinerConfig;
use miner_core::error::PreflightCode;
use miner_core::error::WireError;
use miner_core::findings::{Finding, FindingSink, RunEnd, RunId, RunStart, RunSummary};
use tracing_subscriber::EnvFilter;

mod cli;
mod stderr_emit;
mod stdout_sink;

use cli::{Cli, Command, resolve_toml_path};
use stdout_sink::StdoutSink;

fn main() -> anyhow::Result<()> {
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
    let _cfg: MinerConfig = match MinerConfig::resolve(toml_path.as_deref(), parsed.overrides()) {
        Ok(c) => c,
        Err(e) => {
            let code = classify_figment_error(&e);
            let err = WireError::preflight(code, e.to_string());
            // Best-effort emit; if stderr is broken, we still exit 1.
            let _ = stderr_emit::emit_to_stderr(&err);
            std::process::exit(1);
        }
    };

    match parsed.command {
        Command::EmitFixture => emit_fixture()?,
    }

    Ok(())
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

/// `emit-fixture` subcommand: one `RunStart` + one `RunEnd` to stdout via
/// `StdoutSink`. Both records share the same `RunId` (relies on `Copy`).
fn emit_fixture() -> anyhow::Result<()> {
    tracing::info!("emitting fixture");
    let mut sink = StdoutSink::new();
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

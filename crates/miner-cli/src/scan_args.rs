//! `ScanArgs` — clap-derive struct + window parser + `to_scan_request` conversion.
//!
//! Pattern analog: `cli.rs:28-84` (`Cli` clap-Parser derive + `Cli::overrides()`
//! conversion). `ScanArgs` follows the same shape but as a clap `Args` substruct
//! that the `Command::Scan(ScanArgs)` variant wraps.
//!
//! ## D3-19 CLI surface
//!
//! ```text
//! miner scan <scan_id@version> --instrument <SYM> --side <bid|ask> \
//!     --timeframe <15m|1h|1d> --window <ISO_FROM>:<ISO_TO> \
//!     [--gap-policy <strict|continuous_only>] [--dry-run] [--params <KEY=VAL>...]
//! ```
//!
//! Defaults:
//! - `--side`: `bid` (D3-19 — conservative FX default).
//! - `--gap-policy`: `continuous_only` (D3-19 — the policy that "does
//!   something" by default; `strict` is opt-in).
//!
//! Wave 0 scaffold: signature only. Plan 03-05 fills `parse_window` + Plan 03-02
//! fills `to_scan_request`.

#![allow(dead_code, unused_variables)]

use clap::Args;
use miner_core::error::WireError;
use miner_core::reader::ClosedRangeUtc;
use miner_core::scan::ScanRequest;

/// `miner scan` subcommand arguments.
///
/// Pattern: `cli.rs:28-51` (clap `Parser` derive + `#[arg]` flags + global flags
/// in the parent `Cli` struct). The four global flags (`--config`,
/// `--cache-root`, `--bar-cache-root`, `--output`) stay on `Cli` and are
/// inherited by every subcommand.
#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Positional `<scan_id@version>` — e.g., `"stats.autocorr.ljung_box@1"`.
    pub scan_id_at_version: String,

    /// Instrument symbol — single-shot per invocation (D3-18). Phase 5's
    /// sweep manifest is the only fanout entry point.
    #[arg(long)]
    pub instrument: String,

    /// Bid/ask side. Default `bid` per D3-19 (FX-conservative).
    #[arg(long, default_value = "bid")]
    pub side: String,

    /// Timeframe (`15m` / `1h` / `1d`). Drives `BarCache::get_or_build`.
    #[arg(long)]
    pub timeframe: String,

    /// `START:END` half-open ISO 8601 window, UTC-only (D3-07).
    ///
    /// Plan 03-05 wires `parse_window`; Wave 0 ships an `unimplemented!()` body
    /// so cargo `--help` shows the flag.
    #[arg(long, value_parser = parse_window)]
    pub window: ClosedRangeUtc,

    /// `strict` (one `GapAborted` finding on any gap, no `Result`) or
    /// `continuous_only` (partition into gap-free sub-ranges). Default
    /// `continuous_only` per D3-19.
    #[arg(long, default_value = "continuous_only")]
    pub gap_policy: String,

    /// Emit one `Finding::DryRun` envelope and exit 0 (D3-21). The facade
    /// still wraps in `RunStart`/`RunEnd` framing so the dry-run output is
    /// structurally indistinguishable from a normal run except for the
    /// envelope kind.
    #[arg(long)]
    pub dry_run: bool,

    /// Repeatable `KEY=VAL` typed scan parameters. Plan 03-02 parses RHS as
    /// JSON via `parse_params_kv` (so `lags=20` → `{"lags": 20}`).
    #[arg(long = "params", action = clap::ArgAction::Append)]
    pub params: Vec<String>,
}

impl ScanArgs {
    /// Convert clap-parsed args into a typed [`ScanRequest`] (the post-preflight
    /// resolved request the facade consumes).
    ///
    /// Pattern: `cli.rs:70-83` (`Cli::overrides` — clap struct → typed-domain
    /// struct via a `#[must_use]` method).
    ///
    /// `code_revision` is `miner_core::CODE_REVISION` at the call site (the
    /// CLI's `main()` injects it so tests can substitute a stable string).
    ///
    /// # Errors
    /// Returns a [`WireError`] with code
    /// [`miner_core::error::PreflightCode::InvalidParameter`] on any
    /// `--params KEY=VAL` parse failure, unknown `--side`, unknown
    /// `--timeframe`, or unknown `--gap-policy` value.
    ///
    /// Wave 0 scaffold: signature only. Plan 03-02 fills the body.
    pub fn to_scan_request(&self, code_revision: &str) -> Result<ScanRequest, WireError> {
        unimplemented!(
            "Plan 03-02 wires to_scan_request; will: \
             1) split scan_id_at_version on '@'; 2) parse side via Side::try_from_str; \
             3) parse gap_policy via GapPolicyKind::try_from_str; \
             4) call engine::preflight::parse_params_kv(&self.params); \
             5) return ScanRequest {{ ... }}"
        )
    }
}

/// Parse a `START:END` half-open ISO 8601 window string into a [`ClosedRangeUtc`].
///
/// Per D3-07: dates and datetimes, `Z`-suffix only (no other timezone tags),
/// half-open semantics. Examples:
///
/// - `"2024-01-01:2024-12-31"` → `[2024-01-01T00:00:00Z, 2024-12-31T00:00:00Z)`.
/// - `"2024-01-01T00:00:00Z:2024-12-31T00:00:00Z"` → same as above (explicit
///   datetime form).
///
/// # Errors
///
/// Returns a user-facing `String` error (clap renders it to stderr at parse
/// time) when:
/// - The argument lacks a `:` separator.
/// - Either side fails ISO 8601 parsing.
/// - End is not strictly after start.
///
/// Wave 0 scaffold: signature only. Plan 03-05 fills the body verbatim per
/// 03-RESEARCH §Pattern 5 lines 466-489.
fn parse_window(s: &str) -> Result<ClosedRangeUtc, String> {
    unimplemented!(
        "Plan 03-05 wires parse_window per 03-RESEARCH §Pattern 5 lines 466-489"
    )
}

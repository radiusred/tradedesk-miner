//! `ScanArgs` — clap-derive struct + window parser + `to_scan_request` conversion.
//!
//! Pattern analog: `cli.rs:28-84` (`Cli` clap-Parser derive + `Cli::overrides()`
//! conversion). `ScanArgs` follows the same shape but as a clap `Args` substruct
//! that the `Command::Scan(ScanArgs)` variant wraps.
//!
//! ## D3-19 / D4-02 CLI surface
//!
//! ```text
//! miner scan <scan_id@version> --instrument SYMBOL:side [--instrument SYMBOL:side]... \
//!     --timeframe <15m|1h|1d> --window <ISO_FROM>:<ISO_TO> \
//!     [--gap-policy <strict|continuous_only>] [--dry-run] [--params <KEY=VAL>...]
//! ```
//!
//! Plan 04-02 (D4-02 / PATTERNS.md Pattern K): the legacy `--instrument SYMBOL`
//! + `--side bid|ask` pair is replaced by a REPEATABLE `--instrument SYMBOL:side`
//! flag. Single-leg scans pass `--instrument EURUSD:bid` once; two-leg CROSS
//! scans pass it twice (e.g. `--instrument EURUSD:bid --instrument GBPUSD:bid`).
//! The engine's `validate_arity` preflight (Plan 04-02) rejects mismatched
//! arity with `PreflightCode::WrongInstrumentArity`.
//!
//! Defaults:
//! - `--gap-policy`: `continuous_only` (D3-19 — the policy that "does
//!   something" by default; `strict` is opt-in).
//!
//! ## Cfg-gated test-only hook (Blocker 1 — Pitfall 8)
//!
//! Under `#[cfg(any(test, feature = "test-internal"))]` `ScanArgs` gains a
//! `--sleep-after-first-finding-ms <ms>` flag (`hide = true` on `--help`) that
//! Plan 03-06's `sigint_preserves_stream` integration test uses to make the
//! SIGINT race deterministic. The value is forwarded into
//! `ScanRequest.sleep_after_first_finding_ms` (also cfg-gated) via
//! `ScanRequest::with_sleep_after_first_finding_ms`. Release builds (default
//! features, no `test-internal`) do NOT compile the flag in.

use clap::Args;
use miner_core::aggregator::Timeframe;
use miner_core::engine::gap_policy::GapPolicyKind;
use miner_core::engine::preflight;
use miner_core::error::{PreflightCode, WireError};
use miner_core::findings::TimeRange;
use miner_core::reader::{ClosedRangeUtc, InstrumentSpec, Side};
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

    /// Repeatable instrument flag — `--instrument SYMBOL:side` (Plan 04-02 /
    /// D4-02 / PATTERNS.md Pattern K). Length must match the scan's declared
    /// `arity()`. Single-leg scans (ANOM / SEAS) take ONE flag; Pair-arity
    /// scans (CROSS, Plan 04-07) take TWO flags in leg order. Engine
    /// preflight rejects mismatches with `PreflightCode::WrongInstrumentArity`.
    #[arg(long = "instrument", action = clap::ArgAction::Append, value_parser = parse_instrument_spec)]
    pub instruments: Vec<InstrumentSpec>,

    /// Timeframe (`15m` / `1h` / `1d`). Drives `BarCache::get_or_build`.
    #[arg(long)]
    pub timeframe: String,

    /// `START:END` half-open ISO 8601 window, UTC-only (D3-07).
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

    /// Repeatable `KEY=VAL` typed scan parameters. Parsed via
    /// `engine::preflight::parse_params_kv` with A9 typed-fallback (so
    /// `lags=20` resolves to `{"lags": 20}` without quoting).
    #[arg(long = "params", action = clap::ArgAction::Append)]
    pub params: Vec<String>,

    /// **Test-only Pitfall 8 hook** (Blocker 1 — Plan 03-06 SIGINT integration
    /// test ingress). When `Some(ms)`, `LjungBoxScan::run` performs a
    /// cancel-aware sleep loop after emitting the first `Finding::Result`
    /// envelope, making the SIGINT race deterministic in
    /// `sigint_preserves_stream`. Hidden from `--help`; gated to `cfg(test)` or
    /// `feature = "test-internal"`. NEVER reachable in release production
    /// builds — confirmed by Plan 03-06's release-binary `--help` inspection
    /// gate (threat T-03-06-03).
    ///
    /// Mirrors the cfg-gated `ScanCtx.sleep_after_first_finding_ms` and
    /// `ScanRequest.sleep_after_first_finding_ms` fields declared by Plan
    /// 03-02 in `crates/miner-core/src/scan/mod.rs`.
    #[cfg(any(test, feature = "test-internal"))]
    #[arg(long = "sleep-after-first-finding-ms", hide = true)]
    pub sleep_after_first_finding_ms: Option<u64>,
}

impl ScanArgs {
    /// Convert clap-parsed args into a typed [`ScanRequest`] (the post-preflight
    /// resolved request the facade consumes).
    ///
    /// Pattern: `cli.rs:70-83` (`Cli::overrides` — clap struct → typed-domain
    /// struct via a `#[must_use]` method). The cfg-gated
    /// `sleep_after_first_finding_ms` field is forwarded via the chained
    /// constructor pattern `ScanRequest::new(...).with_sleep_after_first_finding_ms(...)`
    /// so the call-site struct literal stays cfg-free (Warning 1 polish) and the
    /// cfg gate lives on the chained method itself (Plan 03-05 step 4 —
    /// recommended route, Blocker 1).
    ///
    /// `code_revision` is `miner_core::CODE_REVISION` at the call site (the
    /// CLI's `main()` injects it so tests can substitute a stable string).
    ///
    /// # Errors
    /// Returns a [`WireError`] with code
    /// [`miner_core::error::PreflightCode::InvalidParameter`] on any
    /// `--params KEY=VAL` parse failure, unknown `--side`, unknown
    /// `--timeframe`, unknown `--gap-policy` value, or unknown
    /// `--scan-id@version` form.
    pub fn to_scan_request(&self, _code_revision: &str) -> Result<ScanRequest, WireError> {
        // 1. id@version split (engine::preflight is the canonical helper —
        // mirrors Plan 03-03's symmetric MCP/HTTP usage in Phase 6).
        let (scan_id, version) = preflight::resolve_scan_id_at_version(&self.scan_id_at_version)?;

        // 2. timeframe / gap_policy enum parses. (Plan 04-02 / D4-02:
        // `--side` was removed from the CLI surface — every leg's side
        // travels INSIDE the `--instrument SYMBOL:side` value, parsed
        // by `parse_instrument_spec`.)
        let timeframe = Timeframe::from_str(&self.timeframe).map_err(|bad| {
            WireError::preflight(
                PreflightCode::InvalidParameter,
                format!("timeframe must be one of \"15m\" / \"1h\" / \"1d\"; got {bad:?}"),
            )
            .with_context(
                "timeframe",
                serde_json::Value::String(self.timeframe.clone()),
            )
        })?;
        let gap_policy = GapPolicyKind::from_str(&self.gap_policy).map_err(|bad| {
            WireError::preflight(
                PreflightCode::InvalidParameter,
                format!("gap_policy must be one of \"strict\" / \"continuous_only\"; got {bad:?}"),
            )
            .with_context(
                "gap_policy",
                serde_json::Value::String(self.gap_policy.clone()),
            )
        })?;

        // 3. params KEY=VAL with A9 typed-fallback.
        let resolved_params = preflight::parse_params_kv(&self.params)?;

        // 4. param_hash over canonical resolved-params blob (D3-13).
        let param_hash =
            miner_core::engine::param_hash::param_hash(&resolved_params).map_err(|e| {
                WireError::preflight(
                    PreflightCode::InvalidParameter,
                    format!("param hash failed: {e}"),
                )
            })?;

        // 5. Construct ScanRequest via the chained-constructor pattern (Plan
        // 03-05 step 4 — recommended route). Single-shot today (D3-18); Phase
        // 5's sweep manifest fans out at a higher layer.
        let sub_range = TimeRange {
            start_utc: self.window.start,
            end_utc: self.window.end,
        };
        // Phase 4 (Plan 04-02 / D4-02): clap already parsed `--instrument
        // SYMBOL:side` (repeatable) into `Vec<InstrumentSpec>` via the
        // `parse_instrument_spec` value-parser. Engine preflight
        // (`validate_arity`) rejects mismatched arity post-CLI.
        let instruments = self.instruments.clone();
        let req = ScanRequest::new(
            scan_id,
            version,
            instruments,
            timeframe,
            self.window,
            sub_range,
            gap_policy,
            self.dry_run,
            resolved_params,
            param_hash,
        );

        // 6. Forward the cfg-gated --sleep-after-first-finding-ms hook into
        // ScanRequest.sleep_after_first_finding_ms (Blocker 1 — Pitfall 8
        // ingress wiring). The chained method is cfg-gated to match the field
        // gate; release builds skip this line entirely.
        #[cfg(any(test, feature = "test-internal"))]
        let req = req.with_sleep_after_first_finding_ms(self.sleep_after_first_finding_ms);

        Ok(req)
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
/// Delegates to [`miner_core::engine::preflight::parse_iso_utc_window`] so
/// MCP / HTTP wrappers in Phase 6 share the same window parser; on failure the
/// inner `WireError.message` is surfaced as a user-facing `String` (clap's
/// `value_parser` expects `Result<T, String>` and renders the error to stderr
/// at parse time).
///
/// # Errors
///
/// Returns a user-facing `String` error when:
/// - The argument lacks a `:` separator.
/// - Either side fails ISO 8601 parsing.
/// - Either side carries a non-`Z` timezone suffix (A3).
/// - End is not strictly after start.
pub fn parse_window(s: &str) -> Result<ClosedRangeUtc, String> {
    preflight::parse_iso_utc_window(s).map_err(|wire_err| wire_err.message)
}

/// Parse a single `--instrument SYMBOL:side` flag value into an
/// [`InstrumentSpec`]. Plan 04-02 / D4-02 / PATTERNS.md Pattern K — used
/// as the clap `value_parser` for the repeatable `--instrument` flag.
///
/// Splits the input on the FIRST `:` separator; the left part is the
/// symbol (uppercase-normalised by the consumer; this parser preserves
/// the input case), the right part parses via [`Side::from_str`]
/// (`bid` / `ask` lowercase wire form).
///
/// # Errors
/// Returns a user-facing `String` error when:
/// - The input lacks a `:` separator.
/// - The symbol leg is empty.
/// - The side leg is not one of `"bid"` / `"ask"`.
pub fn parse_instrument_spec(s: &str) -> Result<InstrumentSpec, String> {
    let (symbol, side) = s.split_once(':').ok_or_else(|| {
        format!(
            "--instrument value must be of the form SYMBOL:side (e.g., EURUSD:bid); got {s:?}"
        )
    })?;
    if symbol.is_empty() {
        return Err(format!(
            "--instrument SYMBOL:side has empty symbol; got {s:?}"
        ));
    }
    let side = Side::from_str(side).map_err(|bad| {
        format!(
            "--instrument SYMBOL:side has unknown side {bad:?} (expected bid|ask); got {s:?}"
        )
    })?;
    Ok(InstrumentSpec {
        symbol: symbol.to_string(),
        side,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use clap::Parser;
    use miner_core::aggregator::Timeframe;
    use miner_core::engine::gap_policy::GapPolicyKind;
    use miner_core::reader::Side;

    /// Parse a complete `miner scan ...` argv vector for use in tests. Plan
    /// 04-02 / D4-02: the `--instrument` flag now takes `SYMBOL:side` form
    /// (the legacy `--side bid|ask` flag was removed).
    fn parse_argv(extra: &[&str]) -> Cli {
        // Always provide the four required base args; `extra` appends.
        let mut argv: Vec<&str> = vec![
            "miner",
            "scan",
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD:bid",
            "--timeframe",
            "15m",
            "--window",
            "2024-01-01:2024-01-02",
        ];
        argv.extend_from_slice(extra);
        Cli::try_parse_from(argv).expect("parse ok")
    }

    fn unwrap_scan_args(cli: &Cli) -> &ScanArgs {
        match &cli.command {
            Command::Scan(args) => args,
            other => panic!("expected Command::Scan; got {other:?}"),
        }
    }

    /// ScanArgs_defaults: --gap-policy defaults to continuous_only,
    /// --dry-run false, --params empty (D3-19). Plan 04-02 removed
    /// `--side`; side travels inside `--instrument SYMBOL:side`.
    #[test]
    fn scan_args_defaults_per_d3_19_local() {
        let cli = parse_argv(&[]);
        let args = unwrap_scan_args(&cli);
        assert_eq!(args.gap_policy, "continuous_only");
        assert!(!args.dry_run);
        assert!(args.params.is_empty());
        assert_eq!(args.timeframe, "15m");
        assert_eq!(args.instruments.len(), 1);
        assert_eq!(args.instruments[0].symbol, "EURUSD");
        assert_eq!(args.instruments[0].side, Side::Bid);
        assert_eq!(args.scan_id_at_version, "stats.autocorr.ljung_box@1");
    }

    #[test]
    fn scan_args_to_scan_request_happy() {
        let cli = parse_argv(&[]);
        let args = unwrap_scan_args(&cli);
        let req = args
            .to_scan_request("test-rev")
            .expect("happy-path conversion ok");
        assert_eq!(req.scan_id, "stats.autocorr.ljung_box");
        assert_eq!(req.version, 1);
        // D4-01: instruments Vec replaces the singleton `instrument`+`side`.
        assert_eq!(req.instruments.len(), 1);
        assert_eq!(req.instruments[0].symbol, "EURUSD");
        assert_eq!(req.instruments[0].side, Side::Bid);
        assert_eq!(req.timeframe, Timeframe::Tf15m);
        assert_eq!(req.gap_policy, GapPolicyKind::ContinuousOnly);
        assert!(!req.dry_run);
        // Empty params -> resolved_params is an empty Object.
        assert!(matches!(&req.resolved_params, serde_json::Value::Object(m) if m.is_empty()));
        // param_hash is the 64-char blake3 of `{}`.
        assert_eq!(req.param_hash.as_str().len(), 64);
    }

    /// Plan 04-02 Task 3 — Behavior Tests 1-4: `parse_instrument_spec`.
    #[test]
    fn parse_instrument_spec_basic() {
        let spec = parse_instrument_spec("EURUSD:bid").expect("ok");
        assert_eq!(spec.symbol, "EURUSD");
        assert!(matches!(spec.side, Side::Bid));
    }

    #[test]
    fn parse_instrument_spec_ask() {
        let spec = parse_instrument_spec("GBPUSD:ask").expect("ok");
        assert_eq!(spec.symbol, "GBPUSD");
        assert!(matches!(spec.side, Side::Ask));
    }

    #[test]
    fn parse_instrument_spec_invalid_side() {
        let err = parse_instrument_spec("EURUSD:both").expect_err("must reject");
        // Message must mention side / bid|ask.
        assert!(
            err.to_lowercase().contains("side") || err.contains("bid|ask"),
            "error must mention side; got {err:?}"
        );
    }

    #[test]
    fn parse_instrument_spec_missing_colon() {
        let err = parse_instrument_spec("EURUSD").expect_err("must reject");
        assert!(
            err.contains("SYMBOL:side"),
            "error must mention SYMBOL:side format; got {err:?}"
        );
    }

    /// Plan 04-02 Task 3 — Behavior Test 5: repeatable `--instrument`
    /// (two-leg case) lands a `Vec<InstrumentSpec>` of length 2 in leg
    /// order.
    #[test]
    fn scan_args_round_trip_two_instruments() {
        let cli = Cli::try_parse_from([
            "miner",
            "scan",
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD:bid",
            "--instrument",
            "GBPUSD:bid",
            "--timeframe",
            "15m",
            "--window",
            "2024-01-01:2024-12-31",
        ])
        .expect("parse ok");
        let args = unwrap_scan_args(&cli);
        assert_eq!(args.instruments.len(), 2, "two-leg Vec");
        assert_eq!(args.instruments[0].symbol, "EURUSD");
        assert!(matches!(args.instruments[0].side, Side::Bid));
        assert_eq!(args.instruments[1].symbol, "GBPUSD");
        assert!(matches!(args.instruments[1].side, Side::Bid));
        // Sanity: to_scan_request preserves leg order.
        let req = args.to_scan_request("rev").expect("ok");
        assert_eq!(req.instruments.len(), 2);
        assert_eq!(req.instruments[0].symbol, "EURUSD");
        assert_eq!(req.instruments[1].symbol, "GBPUSD");
    }

    #[test]
    fn scan_args_to_scan_request_with_params() {
        let cli = parse_argv(&["--params", "lags=20"]);
        let args = unwrap_scan_args(&cli);
        let req = args.to_scan_request("rev").expect("conversion ok");
        // A9 typed-fallback: lags=20 -> {"lags": 20} (i64 inferred).
        match &req.resolved_params {
            serde_json::Value::Object(m) => {
                let v = m.get("lags").expect("lags key present");
                assert_eq!(v, &serde_json::json!(20));
            }
            other => panic!("expected Object; got {other:?}"),
        }
        // param_hash matches engine::param_hash::param_hash over the same blob.
        let expected =
            miner_core::engine::param_hash::param_hash(&req.resolved_params).expect("hash ok");
        assert_eq!(req.param_hash.as_str(), expected.as_str());
    }

    #[test]
    fn scan_args_invalid_scan_id() {
        // scan_id without '@' must reject at to_scan_request.
        let cli = Cli::try_parse_from([
            "miner",
            "scan",
            "bad-no-at",
            "--instrument",
            "EURUSD:bid",
            "--timeframe",
            "15m",
            "--window",
            "2024-01-01:2024-01-02",
        ])
        .expect("clap parse ok");
        let args = unwrap_scan_args(&cli);
        let err = args.to_scan_request("rev").expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
    }

    /// Plan 04-02 / D4-02 — invalid side leg inside an `--instrument SYMBOL:side`
    /// value is rejected at CLI parse time (clap value-parser returns a String
    /// error; clap exits before `to_scan_request` is reachable).
    #[test]
    fn scan_args_invalid_side_in_instrument() {
        let res = Cli::try_parse_from([
            "miner",
            "scan",
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD:middle",
            "--timeframe",
            "15m",
            "--window",
            "2024-01-01:2024-01-02",
        ]);
        assert!(
            res.is_err(),
            "clap value-parser must reject EURUSD:middle at parse time"
        );
    }

    #[test]
    fn scan_args_invalid_timeframe() {
        let cli = Cli::try_parse_from([
            "miner",
            "scan",
            "stats.autocorr.ljung_box@1",
            "--instrument",
            "EURUSD:bid",
            "--timeframe",
            "2h",
            "--window",
            "2024-01-01:2024-01-02",
        ])
        .expect("clap ok");
        let args = unwrap_scan_args(&cli);
        let err = args.to_scan_request("rev").expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
    }

    #[test]
    fn scan_args_invalid_gap_policy() {
        let cli = parse_argv(&["--gap-policy", "lax"]);
        let args = unwrap_scan_args(&cli);
        let err = args.to_scan_request("rev").expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
    }

    #[test]
    fn parse_window_strict_z() {
        // +02:00 offset rejected (A3 strict-Z enforcement). The
        // engine::preflight::parse_iso_utc_window splitter recognises the
        // datetime only via the `Z` terminator, so any non-`Z`-terminated
        // form is rejected (either by the splitter or by the per-side
        // strict-Z check). We only assert rejection occurred.
        assert!(
            parse_window("2024-01-01T00:00:00+02:00:2024-12-31T00:00:00+02:00").is_err(),
            "non-Z timezone must reject (A3)"
        );
        // Strict-Z UTC accepted.
        let r =
            parse_window("2024-01-01T00:00:00Z:2024-12-31T00:00:00Z").expect("strict-Z accepted");
        assert!(r.start < r.end);
    }

    #[test]
    fn parse_window_date_only() {
        let r = parse_window("2024-01-01:2024-12-31").expect("date-only accepted");
        // Date-only forms midnight UTC bounds.
        assert_eq!(r.start.format("%H:%M:%S").to_string(), "00:00:00");
        assert_eq!(r.end.format("%H:%M:%S").to_string(), "00:00:00");
    }

    #[test]
    fn parse_window_invalid_returns_err() {
        assert!(parse_window("not-a-window").is_err());
    }

    /// Blocker 1 — Pitfall 8 ingress: under `cfg(test)` the cfg-gated
    /// `--sleep-after-first-finding-ms <ms>` flag is parseable by clap and the
    /// value lands on `ScanArgs.sleep_after_first_finding_ms` as `Some(ms)`.
    #[test]
    #[cfg(any(test, feature = "test-internal"))]
    fn scan_args_sleep_after_first_finding_ms_present_under_test_cfg() {
        let cli = parse_argv(&["--sleep-after-first-finding-ms", "2000"]);
        let args = unwrap_scan_args(&cli);
        assert_eq!(args.sleep_after_first_finding_ms, Some(2000));
    }

    /// Blocker 1 — `to_scan_request` forwards the cfg-gated sleep-hook value
    /// into `ScanRequest.sleep_after_first_finding_ms` via the chained
    /// constructor pattern, so Plan 03-06's SIGINT integration test does NOT
    /// need to re-derive the wiring.
    #[test]
    #[cfg(any(test, feature = "test-internal"))]
    fn scan_args_to_scan_request_forwards_sleep_hook() {
        let cli = parse_argv(&["--sleep-after-first-finding-ms", "2000"]);
        let args = unwrap_scan_args(&cli);
        let req = args.to_scan_request("rev").expect("conversion ok");
        assert_eq!(req.sleep_after_first_finding_ms, Some(2000));
    }

    /// Sleep-hook value defaults to `None` when the flag is absent (still
    /// under `cfg(test)` — the field exists but no value was supplied).
    #[test]
    #[cfg(any(test, feature = "test-internal"))]
    fn scan_args_sleep_hook_defaults_to_none() {
        let cli = parse_argv(&[]);
        let args = unwrap_scan_args(&cli);
        assert!(args.sleep_after_first_finding_ms.is_none());
        let req = args.to_scan_request("rev").expect("conversion ok");
        assert!(req.sleep_after_first_finding_ms.is_none());
    }
}

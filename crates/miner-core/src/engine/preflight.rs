//! Preflight: scan resolver, `--params KEY=VAL` parser, ISO 8601 window
//! parser, and error classifiers.
//!
//! Pattern analog: `miner-cli/src/main.rs::classify_figment_error` (lines
//! 107-130) — typed-figment-error → typed `PreflightCode` mapper. Plan 03-01
//! shipped signature-only stubs; this plan (03-03) fills the bodies with the
//! four preflight helpers Plan 04's `run_one` chains together at the
//! facade boundary.
//!
//! ## Boundary contract (D-06)
//!
//! Pre-flight rejections write a single `WireError` JSON line to stderr and
//! exit 1. Stdout stays empty (T-01-03 stdout discipline). Every helper here
//! returns a typed `PreflightCode` (NEVER a stringly-typed code) so the
//! `WireError::preflight` boundary constructor (in `error/codes.rs`) is the
//! single point of code-to-string conversion.
//!
//! ## A9 — typed-fallback for `--params`
//!
//! `parse_params_kv("k=20")` returns `{"k": 20}` (i64 inferred); `"k=3.14"`
//! returns `{"k": 3.14}` (f64); `"k=true"` returns `{"k": true}`; `"k=foo"`
//! falls back to `{"k": "foo"}` (string). This mirrors how CLI users
//! actually pass scan parameters — `lags=20` should NOT require quoting.
//!
//! ## A3 — strict-Z UTC enforcement for `--window`
//!
//! [`parse_iso_utc_window`] rejects any datetime form whose timezone suffix
//! is not exactly `Z`. RFC 3339 with `+00:00` or `+02:00` would let the
//! operator silently express non-UTC ranges; UTC-only is the safer default
//! (D3-07).

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};

use crate::error::{PreflightCode, WireError};
use crate::scan::{BootstrapMethod, NullMethod, Registry, Scan, ScanArity};
use tradedesk_data::aggregator::Timeframe;
use tradedesk_data::reader::{ClosedRangeUtc, InstrumentSpec};

// ---------------------------------------------------------------------------
// resolve_scan_id_at_version — "id@version" → (String, u32)
// ---------------------------------------------------------------------------

/// Split `"stats.autocorr.ljung_box@1"` into `("stats.autocorr.ljung_box", 1)`.
///
/// # Errors
/// Returns [`WireError::preflight`] with [`PreflightCode::InvalidParameter`]
/// when the input lacks an `@` separator OR when the `@`-suffix is not a
/// valid `u32`. The context key `"scan_id_at_version"` carries the raw input
/// so the audit-trail line in stderr surfaces the offending value.
pub fn resolve_scan_id_at_version(s: &str) -> Result<(String, u32), WireError> {
    let Some((id, version_str)) = s.split_once('@') else {
        return Err(WireError::preflight(
            PreflightCode::InvalidParameter,
            "scan_id must be of the form 'id@version'",
        )
        .with_context(
            "scan_id_at_version",
            serde_json::Value::String(s.to_string()),
        ));
    };
    let version: u32 = version_str.parse().map_err(|_| {
        WireError::preflight(
            PreflightCode::InvalidParameter,
            format!("scan version is not a valid u32: {version_str:?}"),
        )
        .with_context(
            "scan_id_at_version",
            serde_json::Value::String(s.to_string()),
        )
    })?;
    Ok((id.to_string(), version))
}

// ---------------------------------------------------------------------------
// resolve_scan — registry lookup
// ---------------------------------------------------------------------------

/// Resolve `"id@version"` to a `&dyn Scan` borrowed from the registry.
///
/// # Errors
/// - `InvalidParameter` if the input is malformed (per
///   [`resolve_scan_id_at_version`]).
/// - `UnknownScan` if either the id OR the version is not registered
///   (registry `get()` returns `None` for both cases — D3-17).
pub fn resolve_scan<'a>(s: &str, registry: &'a Registry) -> Result<&'a dyn Scan, WireError> {
    let (id, version) = resolve_scan_id_at_version(s)?;
    registry.get(&id, version).ok_or_else(|| {
        WireError::preflight(
            PreflightCode::UnknownScan,
            format!("no such scan: {id}@{version}"),
        )
        .with_context(
            "scan_id_at_version",
            serde_json::Value::String(s.to_string()),
        )
    })
}

// ---------------------------------------------------------------------------
// validate_arity — Phase 4 (Plan 04-02 / D4-02). Reject any request whose
// `instruments.len()` does not match the scan's declared arity.
// ---------------------------------------------------------------------------

/// Validate that `instruments.len()` matches `scan.arity().expected_len()`.
/// Returns `Ok(())` when the lengths match; otherwise returns a
/// [`WireError`] carrying [`PreflightCode::WrongInstrumentArity`] plus the
/// three context keys consumers rely on (`scan_id`, `expected_arity`,
/// `supplied_arity`).
///
/// Pattern analog: `resolve_scan_id_at_version` + `resolve_scan` — both
/// return `Result<_, WireError>` and use `WireError::preflight(...)` +
/// `with_context(...)`. The engine call site in `run_one_with_registry`
/// adapts the error via `MinerError::Preflight(_)` exactly the same way the
/// existing `resolve_scan` chain does.
///
/// # Errors
/// Returns [`WireError::preflight`] with [`PreflightCode::WrongInstrumentArity`]
/// when `instruments.len() != scan.arity().expected_len()`. The
/// context map carries the scan id and both arity values for the CLI
/// audit-trail line on stderr.
pub fn validate_arity(scan: &dyn Scan, instruments: &[InstrumentSpec]) -> Result<(), WireError> {
    let expected = match scan.arity() {
        ScanArity::Single => 1,
        ScanArity::Pair => 2,
    };
    if instruments.len() != expected {
        let supplied = instruments.len();
        return Err(WireError::preflight(
            PreflightCode::WrongInstrumentArity,
            format!(
                "{} expects {} instrument(s); got {}",
                scan.id(),
                expected,
                supplied
            ),
        )
        .with_context("scan_id", serde_json::Value::String(scan.id().to_string()))
        .with_context(
            "expected_arity",
            serde_json::Value::Number(serde_json::Number::from(expected)),
        )
        .with_context(
            "supplied_arity",
            serde_json::Value::Number(serde_json::Number::from(supplied)),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// validate_hygiene_support — Plan 05-03 / D5-04 / HYG-03 + HYG-04
// ---------------------------------------------------------------------------

/// Phase 5 (Plan 05-03) — reject hygiene requests targeting scans that have
/// not opted into the requested method via [`Scan::supports_bootstrap`] /
/// [`Scan::supports_null_method`].
///
/// Returns `Ok(())` when the caller's request is compatible with the scan's
/// opt-in matrix (i.e., none of bootstrap / null requested, OR the scan
/// supports each method that was requested). Returns
/// [`WireError::preflight`] with [`PreflightCode::HygieneNotSupported`] on
/// the first mismatch — bootstrap is checked before null so the diagnostic
/// names the bootstrap method when both are unsupported.
///
/// Pattern analog: [`validate_arity`] — same `Result<(), WireError>`
/// signature; the engine call site in `run_one_with_registry` adapts via
/// `MinerError::Preflight(_)`.
///
/// # Errors
/// Returns [`WireError::preflight`] with [`PreflightCode::HygieneNotSupported`]
/// when the caller requests a bootstrap or null method whose corresponding
/// `Scan::supports_*` returns `false`. Context map: `scan_id`,
/// `requested_method` (verb-form: `"bootstrap"` or `"null"`),
/// `method` (the unsupported variant's `snake_case` wire form).
pub fn validate_hygiene_support(
    scan: &dyn Scan,
    bootstrap_method: Option<BootstrapMethod>,
    null_method: Option<NullMethod>,
) -> Result<(), WireError> {
    if let Some(bm) = bootstrap_method {
        if !scan.supports_bootstrap() {
            return Err(WireError::preflight(
                PreflightCode::HygieneNotSupported,
                format!(
                    "{} does not support bootstrap method {:?}",
                    scan.id(),
                    bm.as_str()
                ),
            )
            .with_context("scan_id", serde_json::Value::String(scan.id().to_string()))
            .with_context(
                "requested_method",
                serde_json::Value::String("bootstrap".to_string()),
            )
            .with_context("method", serde_json::Value::String(bm.as_str().to_string())));
        }
    }
    if let Some(nm) = null_method {
        if !scan.supports_null_method(nm) {
            return Err(WireError::preflight(
                PreflightCode::HygieneNotSupported,
                format!(
                    "{} does not support null method {:?}",
                    scan.id(),
                    nm.as_str()
                ),
            )
            .with_context("scan_id", serde_json::Value::String(scan.id().to_string()))
            .with_context(
                "requested_method",
                serde_json::Value::String("null".to_string()),
            )
            .with_context("method", serde_json::Value::String(nm.as_str().to_string())));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// parse_params_kv — `--params KEY=VAL...` → serde_json::Value::Object
// ---------------------------------------------------------------------------

/// Parse a list of `--params KEY=VAL` items into a single
/// `serde_json::Value::Object`.
///
/// Each item is split on the FIRST `'='` (so values may contain `'='`). The
/// right-hand-side is JSON-parsed first; on parse failure it falls back to a
/// string (A9). The resulting `Map` is `BTreeMap`-backed by default (no
/// insertion-order preservation feature enabled on `serde_json`) so
/// iteration order is deterministic (OUT-03 / Pitfall 1).
///
/// # Errors
/// Returns [`WireError::preflight`] with [`PreflightCode::InvalidParameter`]
/// when:
/// - An item lacks an `=` separator.
/// - A duplicate key is supplied (last-wins would silently corrupt the
///   resolved-params blob; explicit rejection is safer — RESEARCH §A9).
pub fn parse_params_kv(items: &[String]) -> Result<serde_json::Value, WireError> {
    let mut map = serde_json::Map::new();
    for item in items {
        let Some((key, value_str)) = item.split_once('=') else {
            return Err(WireError::preflight(
                PreflightCode::InvalidParameter,
                format!("param must be of the form KEY=VAL: {item:?}"),
            )
            .with_context("param", serde_json::Value::String(item.clone())));
        };
        // A9 typed-fallback: try parsing as JSON first; on failure, treat as
        // a bare string. `serde_json::from_str` parses numbers, booleans,
        // null, strings, arrays, objects — the full JSON literal grammar.
        let value = serde_json::from_str::<serde_json::Value>(value_str)
            .unwrap_or_else(|_| serde_json::Value::String(value_str.to_string()));
        if map.contains_key(key) {
            return Err(WireError::preflight(
                PreflightCode::InvalidParameter,
                format!("duplicate param key: {key:?}"),
            )
            .with_context("param", serde_json::Value::String(item.clone()))
            .with_context("key", serde_json::Value::String(key.to_string())));
        }
        map.insert(key.to_string(), value);
    }
    Ok(serde_json::Value::Object(map))
}

// ---------------------------------------------------------------------------
// parse_iso_utc_window — "START:END" → ClosedRangeUtc
// ---------------------------------------------------------------------------

/// Parse a `START:END` ISO 8601 half-open UTC window per D3-07.
///
/// Accepts two forms per side:
/// - Date-only `YYYY-MM-DD` — midnight UTC.
/// - Full datetime `YYYY-MM-DDTHH:MM:SSZ` — strict-Z (A3); non-`Z` suffixes
///   (`+02:00`, naive datetimes) are rejected.
///
/// The split point between `START` and `END` is the FIRST top-level `':'`
/// that is not part of an ISO 8601 datetime's `HH:MM:SS` block. We
/// implement this by scanning for the date-or-datetime termination on the
/// LHS: a date-only LHS terminates at position 10 (`YYYY-MM-DD`); a
/// full-datetime LHS terminates at the `Z` suffix.
///
/// # Errors
/// Returns [`WireError::preflight`] with [`PreflightCode::InvalidParameter`]
/// on:
/// - missing `:` separator,
/// - either side failing to parse,
/// - non-`Z` timezone suffix (A3),
/// - empty window (`start >= end`).
pub fn parse_iso_utc_window(s: &str) -> Result<ClosedRangeUtc, WireError> {
    let (lhs, rhs) = split_window(s).ok_or_else(|| {
        WireError::preflight(PreflightCode::InvalidParameter, "window must be START:END")
            .with_context("window", serde_json::Value::String(s.to_string()))
    })?;
    let start = parse_iso_utc(lhs).map_err(|msg| {
        WireError::preflight(PreflightCode::InvalidParameter, msg)
            .with_context("window", serde_json::Value::String(s.to_string()))
    })?;
    let end = parse_iso_utc(rhs).map_err(|msg| {
        WireError::preflight(PreflightCode::InvalidParameter, msg)
            .with_context("window", serde_json::Value::String(s.to_string()))
    })?;
    if start >= end {
        return Err(WireError::preflight(
            PreflightCode::InvalidParameter,
            "empty window: start must be strictly less than end",
        )
        .with_context("window", serde_json::Value::String(s.to_string())));
    }
    Ok(ClosedRangeUtc { start, end })
}

/// Split a window string into `(lhs, rhs)` at the first top-level `':'`
/// that is not part of an ISO 8601 `HH:MM:SS` block.
///
/// Date-only LHS terminates at position 10 (`YYYY-MM-DD`); full-datetime
/// LHS terminates at the literal `Z` suffix.
fn split_window(s: &str) -> Option<(&str, &str)> {
    // Full-datetime LHS path: detect a `T` early in the string, then look
    // for the `Z` boundary.
    if let Some(t_idx) = s.find('T') {
        if t_idx >= 10 {
            if let Some(z_rel) = s[t_idx..].find('Z') {
                let z_abs = t_idx + z_rel + 1; // position immediately after Z
                let rest = s.get(z_abs..)?;
                // Expect a `:` immediately after the Z.
                let rest = rest.strip_prefix(':')?;
                let lhs = &s[..z_abs];
                return Some((lhs, rest));
            }
        }
    }
    // Date-only LHS path: split at position 10 (YYYY-MM-DD = 10 chars).
    if s.len() > 11 {
        let lhs = s.get(..10)?;
        // Confirm the LHS looks like YYYY-MM-DD (no T).
        if !lhs.contains('T') {
            let sep = s.get(10..11)?;
            if sep == ":" {
                let rhs = s.get(11..)?;
                return Some((lhs, rhs));
            }
        }
    }
    None
}

/// Parse one side of a window string as ISO 8601 UTC.
///
/// `YYYY-MM-DD` is accepted as midnight UTC. Full datetimes require an
/// explicit `Z` suffix (A3); RFC 3339 with `+HH:MM` is rejected.
fn parse_iso_utc(s: &str) -> Result<DateTime<Utc>, String> {
    // Date-only form: midnight UTC.
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(date
            .and_hms_opt(0, 0, 0)
            .expect("00:00:00 is always a valid time-of-day")
            .and_utc());
    }
    // Full datetime form: RFC 3339 with explicit Z (A3).
    if !s.ends_with('Z') {
        return Err(format!(
            "ISO 8601 datetime must end with 'Z' (strict UTC, A3): {s:?}"
        ));
    }
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("invalid ISO 8601 datetime {s:?}: {e}"))
}

/// Parse one strict-UTC instant in a `--window` side form: `YYYY-MM-DD`
/// (00:00 UTC) or `YYYY-MM-DDTHH:MM:SSZ`. Used for the `--not-after` date
/// fence so a fence and a window are written the same way.
///
/// # Errors
/// Returns a human-readable message when the value is in neither form or
/// carries a non-`Z` offset (A3).
pub fn parse_iso_utc_instant(s: &str) -> Result<DateTime<Utc>, String> {
    parse_iso_utc(s)
}

// ---------------------------------------------------------------------------
// Date fence — M33-R1
// ---------------------------------------------------------------------------

/// The last timestamp a request can use, in the form the fence compares.
///
/// The fence `F` is an instant. A bound *reaches* the fence when the request
/// can use a timestamp at or after `F`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceBound {
    /// The request uses timestamps strictly before this instant (the end of
    /// a half-open `scan --window`, or a span end). Reaches `F` when `> F`.
    Exclusive(DateTime<Utc>),
    /// The request uses this instant itself (the datetime end of a closed
    /// sweep window). Reaches `F` when `>= F`.
    Inclusive(DateTime<Utc>),
}

impl FenceBound {
    /// `true` when a request with this bound can use a timestamp at or after
    /// `fence`.
    #[must_use]
    pub fn reaches(self, fence: DateTime<Utc>) -> bool {
        match self {
            FenceBound::Exclusive(t) => t > fence,
            FenceBound::Inclusive(t) => t >= fence,
        }
    }

    fn instant(self) -> DateTime<Utc> {
        match self {
            FenceBound::Exclusive(t) | FenceBound::Inclusive(t) => t,
        }
    }
}

/// Refuse `bound` when it reaches `fence`. No fence → always `Ok`.
///
/// `reached_by` names what reached the fence (`"window"`,
/// `"event_window"`, …) for the error context.
///
/// # Errors
/// Returns [`WireError::preflight`] with [`PreflightCode::DateFence`]. The
/// context carries `not_after` (the fence), `reached_by`, `bound` (the
/// offending instant) and `bound_kind` (`"exclusive"` / `"inclusive"`).
pub fn check_fence(
    fence: Option<DateTime<Utc>>,
    reached_by: &str,
    bound: FenceBound,
) -> Result<(), WireError> {
    let Some(fence) = fence else {
        return Ok(());
    };
    if !bound.reaches(fence) {
        return Ok(());
    }
    let fmt = |t: DateTime<Utc>| t.to_rfc3339_opts(SecondsFormat::Secs, true);
    let bound_kind = match bound {
        FenceBound::Exclusive(_) => "exclusive",
        FenceBound::Inclusive(_) => "inclusive",
    };
    Err(WireError::preflight(
        PreflightCode::DateFence,
        format!(
            "{reached_by} reaches the date fence: it can use data up to {} ({bound_kind}); \
             not_after is {}",
            fmt(bound.instant()),
            fmt(fence)
        ),
    )
    .with_context("not_after", serde_json::Value::String(fmt(fence)))
    .with_context(
        "reached_by",
        serde_json::Value::String(reached_by.to_string()),
    )
    .with_context("bound", serde_json::Value::String(fmt(bound.instant())))
    .with_context(
        "bound_kind",
        serde_json::Value::String(bound_kind.to_string()),
    ))
}

/// Fence check for one scan request: the half-open window (`[start, end)`,
/// so the end is exclusive) and every span the scan's params name
/// ([`Scan::param_spans`]).
///
/// # Errors
/// [`PreflightCode::DateFence`] per [`check_fence`].
pub fn validate_scan_fence(
    scan: &dyn Scan,
    window: ClosedRangeUtc,
    timeframe: Timeframe,
    resolved_params: &serde_json::Value,
    fence: Option<DateTime<Utc>>,
) -> Result<(), WireError> {
    if fence.is_none() {
        return Ok(());
    }
    check_fence(fence, "window", FenceBound::Exclusive(window.end))?;
    validate_param_spans_fence(scan, timeframe, resolved_params, fence)
}

/// Fence check for the spans a scan's params name ([`Scan::param_spans`]).
///
/// # Errors
/// [`PreflightCode::DateFence`] per [`check_fence`].
pub fn validate_param_spans_fence(
    scan: &dyn Scan,
    timeframe: Timeframe,
    resolved_params: &serde_json::Value,
    fence: Option<DateTime<Utc>>,
) -> Result<(), WireError> {
    if fence.is_none() {
        return Ok(());
    }
    for span in scan.param_spans(resolved_params, timeframe) {
        check_fence(fence, span.kind, FenceBound::Exclusive(span.end_exclusive))?;
    }
    Ok(())
}

/// The fence bound of a sweep-manifest window string. Sweep `windows` are
/// documented closed at both ends (`docs/sweep_manifest.md`), so the end is
/// read as included: a date-only end `D` includes the whole day
/// (`Exclusive(D + 1 day)`); a datetime end includes that instant
/// (`Inclusive(end)`).
///
/// # Errors
/// [`PreflightCode::InvalidParameter`] when the string is not a valid
/// window (same rules as [`parse_iso_utc_window`]).
pub fn closed_window_fence_bound(s: &str) -> Result<FenceBound, WireError> {
    let range = parse_iso_utc_window(s)?;
    let date_only_end = split_window(s).is_some_and(|(_, rhs)| !rhs.contains('T'));
    if date_only_end {
        Ok(FenceBound::Exclusive(range.end + chrono::Duration::days(1)))
    } else {
        Ok(FenceBound::Inclusive(range.end))
    }
}

// ---------------------------------------------------------------------------
// classify_param_error — serde_json::Error → PreflightCode
// ---------------------------------------------------------------------------

/// Map a `serde_json::Error` from `--params KEY=VAL` parsing into a typed
/// [`PreflightCode`].
///
/// Mirror of `main.rs:112` `classify_figment_error`. Always returns
/// [`PreflightCode::InvalidParameter`] in v1 — every JSON-parse failure at
/// the params boundary is a user-input error, not a config or transient.
#[must_use]
pub fn classify_param_error(_err: &serde_json::Error) -> PreflightCode {
    PreflightCode::InvalidParameter
}

// ---------------------------------------------------------------------------
// BTreeMap import audit — `parse_params_kv` uses serde_json::Map, which is
// already BTreeMap-backed by default (no insertion-order feature). The
// explicit BTreeMap import below documents the discipline and surfaces a
// typo / signature drift regression at compile time.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
const _: fn() = || {
    // Compile-only: pin that `BTreeMap` is reachable from this module so a
    // future refactor that accidentally swaps to `HashMap` is loud.
    fn _take_btreemap(_m: BTreeMap<String, serde_json::Value>) {}
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::bootstrap;
    use chrono::TimeZone;

    // -----------------------------------------------------------------------
    // resolve_scan_id_at_version
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_scan_id_at_version_splits_id_and_version() {
        let (id, v) =
            resolve_scan_id_at_version("stats.autocorr.ljung_box@1").expect("valid id@version");
        assert_eq!(id, "stats.autocorr.ljung_box");
        assert_eq!(v, 1);
    }

    #[test]
    fn resolve_scan_id_at_version_rejects_missing_at() {
        let err = resolve_scan_id_at_version("no_at_symbol").expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
    }

    #[test]
    fn resolve_scan_id_at_version_rejects_non_u32_version() {
        let err =
            resolve_scan_id_at_version("stats.autocorr.ljung_box@vee").expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
    }

    // -----------------------------------------------------------------------
    // resolve_scan
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_scan_resolves_known_scan() {
        let reg = bootstrap();
        let scan = resolve_scan("stats.autocorr.ljung_box@1", &reg).expect("known scan");
        assert_eq!(scan.id(), "stats.autocorr.ljung_box");
        assert_eq!(scan.version(), 1);
    }

    #[test]
    fn resolve_scan_rejects_unknown_id() {
        let reg = bootstrap();
        let result = resolve_scan("nonexistent@1", &reg);
        // `dyn Scan` doesn't implement Debug, so we destructure manually.
        match result {
            Ok(_) => panic!("expected error for unknown scan id"),
            Err(err) => assert_eq!(err.code, "unknown_scan"),
        }
    }

    #[test]
    fn resolve_scan_rejects_unknown_version() {
        let reg = bootstrap();
        let result = resolve_scan("stats.autocorr.ljung_box@99", &reg);
        match result {
            Ok(_) => panic!("expected error for unknown scan version"),
            Err(err) => assert_eq!(err.code, "unknown_scan"),
        }
    }

    // -----------------------------------------------------------------------
    // validate_arity — Plan 04-02 Task 2 (Behavior Tests 1-4)
    // -----------------------------------------------------------------------

    use crate::scan::{ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};
    use tradedesk_data::reader::Side;

    struct StubSingle;
    impl crate::scan::Scan for StubSingle {
        fn id(&self) -> &'static str {
            "stub.single@1"
        }
        fn version(&self) -> u32 {
            1
        }
        fn arity(&self) -> ScanArity {
            ScanArity::Single
        }
        fn param_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn finding_fields(&self) -> ScanFindingShape {
            ScanFindingShape {
                effect_extra_keys: &[],
                raw_series_keys: &[],
            }
        }
        fn run(
            &self,
            _ctx: &ScanCtx<'_>,
            _req: &ScanRequest,
            _sink: &mut dyn crate::findings::FindingSink,
        ) -> Result<(), ScanError> {
            Ok(())
        }
    }

    struct StubPair;
    impl crate::scan::Scan for StubPair {
        fn id(&self) -> &'static str {
            "stub.pair@1"
        }
        fn version(&self) -> u32 {
            1
        }
        fn arity(&self) -> ScanArity {
            ScanArity::Pair
        }
        fn param_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn finding_fields(&self) -> ScanFindingShape {
            ScanFindingShape {
                effect_extra_keys: &[],
                raw_series_keys: &[],
            }
        }
        fn run(
            &self,
            _ctx: &ScanCtx<'_>,
            _req: &ScanRequest,
            _sink: &mut dyn crate::findings::FindingSink,
        ) -> Result<(), ScanError> {
            Ok(())
        }
    }

    fn spec(sym: &str, side: Side) -> InstrumentSpec {
        InstrumentSpec {
            symbol: sym.into(),
            side,
        }
    }

    /// Behavior Test 1: Single + len 1 -> Ok.
    #[test]
    fn validate_arity_accepts_correct_single() {
        let scan = StubSingle;
        let instruments = vec![spec("EURUSD", Side::Bid)];
        assert!(validate_arity(&scan, &instruments).is_ok());
    }

    /// Behavior Test 2: Single + len 2 -> Err with `WrongInstrumentArity` +
    /// `expected_arity == 1`, `supplied_arity == 2`.
    #[test]
    fn validate_arity_rejects_two_instruments_for_single() {
        let scan = StubSingle;
        let instruments = vec![spec("EURUSD", Side::Bid), spec("GBPUSD", Side::Bid)];
        let err = validate_arity(&scan, &instruments).expect_err("must reject");
        assert_eq!(err.code, "wrong_instrument_arity");
        assert_eq!(
            err.context.get("expected_arity"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            err.context.get("supplied_arity"),
            Some(&serde_json::json!(2))
        );
        assert_eq!(
            err.context.get("scan_id"),
            Some(&serde_json::Value::String("stub.single@1".to_string()))
        );
    }

    /// Behavior Test 3: Pair + len 2 -> Ok.
    #[test]
    fn validate_arity_accepts_pair() {
        let scan = StubPair;
        let instruments = vec![spec("EURUSD", Side::Bid), spec("GBPUSD", Side::Bid)];
        assert!(validate_arity(&scan, &instruments).is_ok());
    }

    /// Behavior Test 4: Pair + len 1 -> Err with `WrongInstrumentArity`.
    #[test]
    fn validate_arity_rejects_one_instrument_for_pair() {
        let scan = StubPair;
        let instruments = vec![spec("EURUSD", Side::Bid)];
        let err = validate_arity(&scan, &instruments).expect_err("must reject");
        assert_eq!(err.code, "wrong_instrument_arity");
        assert_eq!(
            err.context.get("expected_arity"),
            Some(&serde_json::json!(2))
        );
        assert_eq!(
            err.context.get("supplied_arity"),
            Some(&serde_json::json!(1))
        );
    }

    // -----------------------------------------------------------------------
    // validate_hygiene_support — Plan 05-03 Task 3 (Tests 1-3)
    // -----------------------------------------------------------------------

    /// A stub scan with explicit hygiene opt-ins for testing.
    struct StubHygiene {
        b: bool,
        ns: bool, // supports CircularShift
        np: bool, // supports PhaseScramble
    }
    impl crate::scan::Scan for StubHygiene {
        fn id(&self) -> &'static str {
            "stub.hygiene"
        }
        fn version(&self) -> u32 {
            1
        }
        fn arity(&self) -> ScanArity {
            ScanArity::Single
        }
        fn param_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn finding_fields(&self) -> ScanFindingShape {
            ScanFindingShape {
                effect_extra_keys: &[],
                raw_series_keys: &[],
            }
        }
        fn run(
            &self,
            _ctx: &ScanCtx<'_>,
            _req: &ScanRequest,
            _sink: &mut dyn crate::findings::FindingSink,
        ) -> Result<(), ScanError> {
            Ok(())
        }
        fn supports_bootstrap(&self) -> bool {
            self.b
        }
        fn supports_null_method(&self, m: NullMethod) -> bool {
            match m {
                NullMethod::CircularShift => self.ns,
                NullMethod::PhaseScramble => self.np,
            }
        }
    }

    /// Plan 05-03 Task 3 Test 1 — both methods None → Ok.
    #[test]
    fn validate_hygiene_support_no_methods_requested_is_ok() {
        let scan = StubHygiene {
            b: false,
            ns: false,
            np: false,
        };
        assert!(validate_hygiene_support(&scan, None, None).is_ok());
    }

    /// Plan 05-03 Task 3 Test 2 — bootstrap requested on `supports_bootstrap=false` → Err.
    #[test]
    fn validate_hygiene_support_rejects_unsupported_bootstrap() {
        let scan = StubHygiene {
            b: false,
            ns: true,
            np: true,
        };
        let err = validate_hygiene_support(&scan, Some(BootstrapMethod::Stationary), None)
            .expect_err("must reject");
        assert_eq!(err.code, "hygiene_not_supported");
        assert_eq!(
            err.context.get("requested_method"),
            Some(&serde_json::Value::String("bootstrap".to_string()))
        );
        assert_eq!(
            err.context.get("method"),
            Some(&serde_json::Value::String("stationary".to_string()))
        );
    }

    /// Plan 05-03 Task 3 Test 3 — null requested on `supports_null_method=false` → Err.
    #[test]
    fn validate_hygiene_support_rejects_unsupported_null() {
        let scan = StubHygiene {
            b: true,
            ns: false,
            np: false,
        };
        let err = validate_hygiene_support(&scan, None, Some(NullMethod::CircularShift))
            .expect_err("must reject");
        assert_eq!(err.code, "hygiene_not_supported");
        assert_eq!(
            err.context.get("requested_method"),
            Some(&serde_json::Value::String("null".to_string()))
        );
        assert_eq!(
            err.context.get("method"),
            Some(&serde_json::Value::String("circular_shift".to_string()))
        );
    }

    /// Plan 05-03 Task 3 Test 4 — bootstrap supported but null unsupported.
    /// The bootstrap check passes; the null check fails second.
    #[test]
    fn validate_hygiene_support_rejects_partial_opt_in() {
        let scan = StubHygiene {
            b: true,
            ns: true,
            np: false, // PhaseScramble unsupported
        };
        let err = validate_hygiene_support(
            &scan,
            Some(BootstrapMethod::Stationary),
            Some(NullMethod::PhaseScramble),
        )
        .expect_err("must reject");
        assert_eq!(err.code, "hygiene_not_supported");
        assert_eq!(
            err.context.get("method"),
            Some(&serde_json::Value::String("phase_scramble".to_string()))
        );
    }

    /// Plan 05-03 Task 3 Test 5 — both supported → Ok.
    #[test]
    fn validate_hygiene_support_accepts_full_opt_in() {
        let scan = StubHygiene {
            b: true,
            ns: true,
            np: true,
        };
        assert!(
            validate_hygiene_support(
                &scan,
                Some(BootstrapMethod::Block),
                Some(NullMethod::CircularShift)
            )
            .is_ok()
        );
    }

    // -----------------------------------------------------------------------
    // parse_params_kv
    // -----------------------------------------------------------------------

    #[test]
    fn parse_params_kv_parses_integer() {
        let v = parse_params_kv(&["lags=20".to_string()]).expect("ok");
        assert_eq!(v, serde_json::json!({"lags": 20}));
    }

    #[test]
    fn parse_params_kv_falls_back_to_string() {
        let v = parse_params_kv(&["lags=abc".to_string()]).expect("ok");
        assert_eq!(v, serde_json::json!({"lags": "abc"}));
    }

    #[test]
    fn parse_params_kv_parses_float() {
        // Use 2.5 instead of 3.14 to avoid clippy::approx_constant (PI lint).
        let v = parse_params_kv(&["floaty=2.5".to_string()]).expect("ok");
        assert_eq!(v, serde_json::json!({"floaty": 2.5}));
    }

    #[test]
    fn parse_params_kv_parses_bool() {
        let v = parse_params_kv(&["boo=true".to_string()]).expect("ok");
        assert_eq!(v, serde_json::json!({"boo": true}));
    }

    #[test]
    fn parse_params_kv_parses_multiple() {
        let v = parse_params_kv(&["k1=1".to_string(), "k2=2".to_string()]).expect("ok");
        assert_eq!(v, serde_json::json!({"k1": 1, "k2": 2}));
    }

    #[test]
    fn parse_params_kv_rejects_malformed() {
        let err = parse_params_kv(&["malformed-no-equals".to_string()]).expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
        assert_eq!(
            err.context.get("param"),
            Some(&serde_json::Value::String("malformed-no-equals".into()))
        );
    }

    #[test]
    fn parse_params_kv_rejects_duplicate_key() {
        let err =
            parse_params_kv(&["k=v1".to_string(), "k=v2".to_string()]).expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
        assert_eq!(
            err.context.get("key"),
            Some(&serde_json::Value::String("k".into()))
        );
    }

    // -----------------------------------------------------------------------
    // parse_iso_utc_window
    // -----------------------------------------------------------------------

    #[test]
    fn parse_iso_utc_window_date_only() {
        let r = parse_iso_utc_window("2024-01-01:2024-12-31").expect("ok");
        assert_eq!(r.start, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        assert_eq!(r.end, Utc.with_ymd_and_hms(2024, 12, 31, 0, 0, 0).unwrap());
    }

    #[test]
    fn parse_iso_utc_window_full_datetime() {
        let r = parse_iso_utc_window("2024-01-01T00:00:00Z:2024-12-31T00:00:00Z").expect("ok");
        assert_eq!(r.start, Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        assert_eq!(r.end, Utc.with_ymd_and_hms(2024, 12, 31, 0, 0, 0).unwrap());
    }

    #[test]
    fn parse_iso_utc_window_rejects_non_z_offset() {
        let err = parse_iso_utc_window("2024-01-01T00:00:00+02:00:2024-12-31T00:00:00+02:00")
            .expect_err("must reject A3 violation");
        assert_eq!(err.code, "invalid_parameter");
    }

    #[test]
    fn parse_iso_utc_window_rejects_invalid_rhs() {
        let err = parse_iso_utc_window("2024-01-01T00:00:00Z:invalid").expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
    }

    #[test]
    fn parse_iso_utc_window_rejects_garbage() {
        let err = parse_iso_utc_window("not-a-window").expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
    }

    #[test]
    fn parse_iso_utc_window_rejects_empty_window() {
        let err = parse_iso_utc_window("2024-12-31:2024-01-01").expect_err("must reject");
        assert_eq!(err.code, "invalid_parameter");
        assert!(
            err.message.contains("empty window"),
            "error message must mention empty window; got {:?}",
            err.message
        );
    }

    // -----------------------------------------------------------------------
    // Date fence — M33-R1
    // -----------------------------------------------------------------------

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    /// A half-open end may sit on the fence (it uses data strictly before
    /// it); one second past the fence reaches it.
    #[test]
    fn fence_exclusive_bound_edges() {
        let f = utc(2030, 1, 1, 0, 0);
        assert!(!FenceBound::Exclusive(utc(2029, 12, 31, 23, 59)).reaches(f));
        assert!(!FenceBound::Exclusive(f).reaches(f));
        assert!(FenceBound::Exclusive(f + chrono::Duration::seconds(1)).reaches(f));
    }

    /// An included end on the fence uses the fence instant: it reaches it.
    #[test]
    fn fence_inclusive_bound_edges() {
        let f = utc(2030, 1, 1, 0, 0);
        assert!(!FenceBound::Inclusive(f - chrono::Duration::seconds(1)).reaches(f));
        assert!(FenceBound::Inclusive(f).reaches(f));
    }

    #[test]
    fn check_fence_without_fence_is_ok() {
        assert!(check_fence(None, "window", FenceBound::Inclusive(utc(2100, 1, 1, 0, 0))).is_ok());
    }

    #[test]
    fn check_fence_error_carries_code_and_context() {
        let f = utc(2030, 1, 1, 0, 0);
        let err = check_fence(
            Some(f),
            "window",
            FenceBound::Exclusive(utc(2030, 1, 2, 0, 0)),
        )
        .expect_err("must refuse");
        assert_eq!(err.code, "date_fence");
        assert_eq!(
            err.context.get("not_after"),
            Some(&serde_json::json!("2030-01-01T00:00:00Z"))
        );
        assert_eq!(
            err.context.get("reached_by"),
            Some(&serde_json::json!("window"))
        );
        assert_eq!(
            err.context.get("bound"),
            Some(&serde_json::json!("2030-01-02T00:00:00Z"))
        );
        assert_eq!(
            err.context.get("bound_kind"),
            Some(&serde_json::json!("exclusive"))
        );
    }

    /// `scan --window` is half-open: `S:F` is accepted, `S:F+1d` is refused.
    #[test]
    fn validate_scan_fence_half_open_window() {
        let scan = StubSingle;
        let f = Some(utc(2030, 1, 1, 0, 0));
        let at = parse_iso_utc_window("2029-12-01:2030-01-01").unwrap();
        let past = parse_iso_utc_window("2029-12-01:2030-01-02").unwrap();
        let params = serde_json::json!({});
        assert!(validate_scan_fence(&scan, at, Timeframe::Tf1h, &params, f).is_ok());
        let err = validate_scan_fence(&scan, past, Timeframe::Tf1h, &params, f)
            .expect_err("window past the fence");
        assert_eq!(err.code, "date_fence");
        assert!(validate_scan_fence(&scan, past, Timeframe::Tf1h, &params, None).is_ok());
    }

    /// Sweep windows are closed: a date-only end includes the whole end day,
    /// a datetime end includes that instant.
    #[test]
    fn closed_window_fence_bound_reads_the_end_as_included() {
        let f = utc(2030, 1, 1, 0, 0);
        let b = closed_window_fence_bound("2029-12-01:2029-12-31").unwrap();
        assert_eq!(b, FenceBound::Exclusive(f));
        assert!(!b.reaches(f), "2029-12-31 included ends at the fence");
        let b = closed_window_fence_bound("2029-12-01:2030-01-01").unwrap();
        assert!(
            b.reaches(f),
            "2030-01-01 included is on and after the fence"
        );
        let b = closed_window_fence_bound("2029-12-01T00:00:00Z:2030-01-01T00:00:00Z").unwrap();
        assert_eq!(b, FenceBound::Inclusive(f));
        assert!(b.reaches(f));
        let b = closed_window_fence_bound("2029-12-01T00:00:00Z:2029-12-31T23:59:59Z").unwrap();
        assert!(!b.reaches(f));
        assert_eq!(
            closed_window_fence_bound("garbage").unwrap_err().code,
            "invalid_parameter"
        );
    }

    /// The event-window scan names its post windows; a span that ends past
    /// the fence is refused even when the request window is clear of it.
    #[test]
    fn validate_scan_fence_event_window_spans() {
        let reg = bootstrap();
        let scan = resolve_scan("seas.event.pre_post_window@1", &reg).expect("registered");
        let f = utc(2030, 1, 1, 0, 0);
        let window = parse_iso_utc_window("2029-11-01:2029-12-01").unwrap();
        let ms = |t: DateTime<Utc>| t.timestamp_millis();

        // Event at 2029-12-31 21:30, 1h bars, 2 post bars: bar opens 21:00,
        // post window ends 23:00 — before the fence.
        let ok = serde_json::json!({
            "event_timestamps": [ms(utc(2029, 12, 31, 21, 30))],
            "post_window_bars": 2
        });
        assert!(validate_scan_fence(scan, window, Timeframe::Tf1h, &ok, Some(f)).is_ok());

        // Same event, 4 post bars: ends 2030-01-01 01:00 — refused.
        let bad = serde_json::json!({
            "event_timestamps": [ms(utc(2029, 12, 31, 21, 30))],
            "post_window_bars": 4
        });
        let err = validate_scan_fence(scan, window, Timeframe::Tf1h, &bad, Some(f))
            .expect_err("event window past the fence");
        assert_eq!(err.code, "date_fence");
        assert_eq!(
            err.context.get("reached_by"),
            Some(&serde_json::json!("event_window"))
        );
        assert_eq!(
            err.context.get("bound"),
            Some(&serde_json::json!("2030-01-01T01:00:00Z"))
        );

        // Default post_window_bars (5) on 1d bars: an event 3 days before the
        // fence reaches it.
        let dflt = serde_json::json!({ "event_timestamps": [ms(utc(2029, 12, 29, 0, 0))] });
        assert!(validate_scan_fence(scan, window, Timeframe::Tf1d, &dflt, Some(f)).is_err());
    }

    /// A scan with no param spans (the default) is checked on its window only.
    #[test]
    fn default_scan_has_no_param_spans() {
        let scan = StubSingle;
        assert!(
            scan.param_spans(
                &serde_json::json!({"event_timestamps": [0]}),
                Timeframe::Tf1h
            )
            .is_empty()
        );
    }

    // -----------------------------------------------------------------------
    // classify_param_error
    // -----------------------------------------------------------------------

    #[test]
    fn classify_param_error_returns_invalid_parameter() {
        let bad = serde_json::from_str::<serde_json::Value>("{not-json}").unwrap_err();
        assert_eq!(classify_param_error(&bad), PreflightCode::InvalidParameter);
    }
}

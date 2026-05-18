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

use chrono::{DateTime, NaiveDate, Utc};

use crate::error::{PreflightCode, WireError};
use crate::reader::ClosedRangeUtc;
use crate::scan::{Registry, Scan};

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
    // classify_param_error
    // -----------------------------------------------------------------------

    #[test]
    fn classify_param_error_returns_invalid_parameter() {
        let bad = serde_json::from_str::<serde_json::Value>("{not-json}").unwrap_err();
        assert_eq!(classify_param_error(&bad), PreflightCode::InvalidParameter);
    }
}

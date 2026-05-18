//! Preflight: `--params` parser + error mapping + registry lookup.
//!
//! Pattern analog: `miner-cli/src/main.rs::classify_figment_error` (lines 107-130)
//! — typed-figment-error → typed `PreflightCode` mapper. Plan 03-01 ships
//! signature-only stubs; Plan 02 fills them.
//!
//! ## Boundary contract (D-06)
//!
//! Pre-flight rejections write a single `WireError` JSON line to stderr and
//! exit 1. Stdout stays empty (T-01-03 stdout discipline). Every helper here
//! returns a typed `PreflightCode` (NEVER a stringly-typed code) so the
//! `WireError::preflight` boundary constructor (in `error/codes.rs`) is the
//! single point of code-to-string conversion.
//!
//! Wave 0 scaffold: signature only. Plan 03-02 fills the bodies.

#![allow(dead_code, unused_variables)]

use crate::error::{PreflightCode, WireError};

/// Map a `serde_json::Error` from `--params KEY=VAL` parsing into a typed
/// [`PreflightCode`].
///
/// Pattern analog: `main.rs:107-130` `classify_figment_error` — match on the
/// proximate error variant, fall through to a default.
///
/// Wave 0 scaffold: signature only. Plan 03-02 fills the body. Will always
/// return [`PreflightCode::InvalidParameter`] in v1 (every JSON-parse failure
/// at the params boundary is a user-input error, not a config or transient).
#[must_use]
pub fn classify_param_error(err: &serde_json::Error) -> PreflightCode {
    unimplemented!(
        "Plan 03-02 wires classify_param_error; will return PreflightCode::InvalidParameter \
         per error/codes.rs:39-53 vocabulary"
    )
}

/// Parse a list of `--params KEY=VAL` items into a single
/// `serde_json::Value::Object`.
///
/// Each `KEY=VAL` is interpreted as `{ "KEY": <VAL parsed as JSON> }` (so
/// `lags=20` parses to `{"lags": 20}` and `name="foo"` parses to
/// `{"name": "foo"}`). The resulting object is the input to the scan's
/// `param_schema()` validator and ultimately to [`super::param_hash::param_hash`]
/// (D3-13).
///
/// # Errors
///
/// Returns a [`WireError`] with code [`PreflightCode::InvalidParameter`] when:
/// - An item lacks an `=` separator.
/// - The right-hand-side is not valid JSON.
/// - A duplicate key is supplied (Plan 03-02 decides — strict reject in v1).
///
/// Wave 0 scaffold: signature only. Plan 03-02 fills the body.
pub fn parse_params_kv(items: &[String]) -> Result<serde_json::Value, WireError> {
    unimplemented!(
        "Plan 03-02 wires parse_params_kv; will iterate items, split on first '=', \
         parse RHS via serde_json::from_str, fold into a serde_json::Map (BTreeMap-backed)"
    )
}

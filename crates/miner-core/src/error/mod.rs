//! Error model — D-18 + RESEARCH §Pitfall 3.
//!
//! Two-layer split:
//! - `MinerError` (this file) — internal `thiserror`-derived enum. NOT `Serialize` —
//!   the `io::Error` variant doesn't compose with the serde derive (RESEARCH §Risk 3).
//! - `WireError` (in [`codes`]) — wire-form for embedding in `kind: scan_error`
//!   envelopes. Carries an OPEN-string `code` for additive extensibility.
//!
//! Plan 03 ships the type definitions and the locked Phase 1 vocabulary
//! ([`PreflightCode`], [`ScanErrorCode`]). Plan 04 wires `stderr_emit` to the
//! pre-flight rejection path.

pub mod codes;
pub mod stderr_emit;

pub use codes::{PreflightCode, ScanErrorCode, WireError};

/// Internal error enum for `miner-core`. Variants are added as Phase 1+ surfaces them;
/// the wire-form for embedding in findings is [`WireError`].
///
/// **Does NOT derive `serde::Serialize`** — the `Io(#[from] std::io::Error)` variant
/// is incompatible with serde-derive (RESEARCH §Pitfall 3). Cross the wire boundary
/// via the `From<MinerError> for WireError` impl below.
#[derive(Debug, thiserror::Error)]
pub enum MinerError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(#[from] figment::Error),

    #[error("scan error: {0}")]
    Scan(String),

    /// Typed preflight rejection carrying the structured `WireError` produced
    /// by `engine::preflight::resolve_scan` (and future preflight helpers).
    /// Eliminates the fragile stringly-typed `MinerError::Scan(format!(...))`
    /// path plus the CLI-side substring-match dispatch on the format-string
    /// prefix (Plan 03-07 CR-03 fix).
    ///
    /// The thiserror attribute uses the LITERAL positional form (with an
    /// explicit format-args expression naming the inner field) rather than
    /// the `{0}` short-form, because `WireError` does NOT implement
    /// `std::fmt::Display`. The format-args expression reaches into the
    /// public `message: String` field of the embedded `WireError` directly.
    /// See the `miner_error_preflight_carries_typed_wireerror` test below
    /// for the Display-impl regression gate.
    #[error("preflight error: {}", _0.message)]
    Preflight(WireError),

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<MinerError> for WireError {
    /// Generic conversion at the engine boundary. The default discriminator is
    /// [`ScanErrorCode::InternalPanicCaught`] — callers that have a more specific
    /// code in hand should construct the `WireError` directly via
    /// [`WireError::scan`] / [`WireError::preflight`].
    ///
    /// The `MinerError::Preflight(WireError)` variant short-circuits the
    /// default mapping and returns the typed `WireError` verbatim, preserving
    /// the original `PreflightCode` and `context` map (Plan 03-07 CR-03 fix
    /// — typed passthrough). All other variants fall through to the
    /// `InternalPanicCaught` default mapping.
    fn from(err: MinerError) -> Self {
        match err {
            MinerError::Preflight(w) => w,
            other => WireError {
                code: ScanErrorCode::InternalPanicCaught.as_str().to_string(),
                message: other.to_string(),
                context: std::collections::BTreeMap::new(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 4 — `miner_error_does_not_require_serialize`: confirms `MinerError`
    /// compiles WITHOUT `Serialize` and that the `Io(#[from] std::io::Error)`
    /// `From`-bridge works. The compile of this function IS the test.
    #[test]
    fn miner_error_does_not_require_serialize() {
        // Build a MinerError via the From<io::Error> bridge.
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "x");
        let mine: MinerError = io_err.into();
        // Cross the wire boundary — proves the From<MinerError> for WireError works.
        let wire: WireError = mine.into();
        assert_eq!(wire.code, "internal_panic_caught");
        assert!(wire.message.contains('x'));
        assert!(wire.context.is_empty());
    }

    /// Plan 03-07 Task 1 — `miner_error_preflight_carries_typed_wireerror`.
    ///
    /// Pins three contracts in one test (Warning 5 + Info 6):
    /// 1. The `MinerError::Preflight(WireError)` variant exists and accepts a
    ///    typed `WireError` built via `WireError::preflight`.
    /// 2. The thiserror attribute on `MinerError::Preflight` produces a
    ///    Display impl whose output starts with `"preflight error:"` AND
    ///    contains the underlying `WireError.message`. This pin prevents a
    ///    future refactor from silently switching to the `{0}` short-form,
    ///    which would not compile because `WireError` does NOT implement
    ///    `Display`.
    /// 3. The `From<MinerError> for WireError` impl returns the embedded
    ///    `WireError` VERBATIM — preserving the typed `PreflightCode` on the
    ///    `code` field — when the input is `MinerError::Preflight(_)`. This
    ///    pin is what the CLI dispatch in `handle_scan_subcommand` relies on:
    ///    the `code: "unknown_scan"` string on stderr is produced by the
    ///    engine's `engine::preflight::resolve_scan`, not reconstructed in
    ///    the CLI from a substring match.
    #[test]
    fn miner_error_preflight_carries_typed_wireerror() {
        let wire_err = WireError::preflight(PreflightCode::UnknownScan, "no such scan: foo@1");
        let mine = MinerError::Preflight(wire_err.clone());

        // Pin 2 — Display impl produced by the literal thiserror attribute.
        let display = format!("{mine}");
        assert!(
            display.starts_with("preflight error:"),
            "Display impl must start with 'preflight error:' (Warning 5 — \
             positional form pinned because WireError lacks Display); got: {display:?}"
        );
        assert!(
            display.contains("no such scan: foo@1"),
            "Display impl must echo the embedded WireError.message via the \
             positional format-args expression; got: {display:?}"
        );

        // Pin 3 — From<MinerError> for WireError passthrough.
        let wire: WireError = mine.into();
        assert_eq!(
            wire.code, "unknown_scan",
            "From<MinerError> for WireError must preserve the typed \
             PreflightCode::UnknownScan on the wire-form `code` field; got: {wire:?}"
        );
        assert!(
            wire.message.contains("foo@1"),
            "WireError.message must echo the original input; got: {wire:?}"
        );
    }
}

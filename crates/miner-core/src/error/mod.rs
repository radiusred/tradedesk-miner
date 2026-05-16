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

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<MinerError> for WireError {
    /// Generic conversion at the engine boundary. The default discriminator is
    /// [`ScanErrorCode::InternalPanicCaught`] — callers that have a more specific
    /// code in hand should construct the `WireError` directly via
    /// [`WireError::scan`] / [`WireError::preflight`].
    fn from(err: MinerError) -> Self {
        WireError {
            code: ScanErrorCode::InternalPanicCaught.as_str().to_string(),
            message: err.to_string(),
            context: std::collections::BTreeMap::new(),
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
}

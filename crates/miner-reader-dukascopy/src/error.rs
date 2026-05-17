//! Task 4 placeholder — Plan 02-01 Task 4 replaces this with the full
//! `DukascopyError` thiserror enum + `From<DukascopyError> for WireError`.

/// Placeholder error type. Task 4 expands this with `Io`, `Csv`, `WalkDir`,
/// `TimestampParse`, `MissingField`, `CorruptSourceFile`, `PathLayout`, `HexDecode`
/// variants and the `From for WireError` engine-boundary impl.
#[derive(Debug, thiserror::Error)]
pub enum DukascopyError {
    #[error("stub")]
    Stub,
}

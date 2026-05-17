//! Pluggable data-source abstraction (D2-12 / D2-13).
//!
//! Phase 2 ships the `Reader` trait here; concrete impls live in sibling crates
//! (e.g., `miner-reader-dukascopy`). The aggregator, gap detector, and cache all
//! consume `&dyn Reader` so a future equity / crypto reader plugs in without
//! changing `miner-core`.
//!
//! ## Trait shape (D2-12)
//!
//! - `Send + Sync` because Phase 3+ readers may be shared across rayon workers.
//! - Associated `type Error` (mirrors `FindingSink` from Phase 1) so each impl
//!   can carry its own thiserror enum without the trait owning the variants.
//! - `read_1m_bars` returns `Box<dyn Iterator + Send + 'a>` because the inner
//!   `Result`-iterator chain is not nameable in stable Rust (no `impl Iterator`
//!   in trait-method-return position without `feature(return_impl_trait_in_trait)`).
//! - `fingerprint_day` returns `Option<Blake3Hex>` — `None` when the day file is
//!   absent, `Some` when present. Hash is over the FULL `.csv.zst` bytes (D2-05).
//! - `enumerate_days` is the cache-invalidator helper that walks days in range
//!   without parsing CSV (RESEARCH Refinement #1).
//!
//! ## A1 invariant (D2-13)
//!
//! `RawBar.tick_volume: f64` — never named `volume`, never an integer count. The CSV
//! `volume` column from Dukascopy is renamed at the reader boundary so downstream
//! consumers cannot confuse it with contract volume. Aggregation across bars is
//! by SUM (matches `tradedesk-dukascopy/export.py:312 vol.resample().sum()`).
//!
//! ## Object-safety (D2-12)
//!
//! The compile-time test below pins the trait shape — any change that breaks
//! `dyn Reader<Error = E>` coercion fails the workspace build.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::calendar::Calendar;

/// Bid or ask side. `#[serde(rename_all = "lowercase")]` so wire form matches the
/// Dukascopy filename suffix (`<DD>_bid.csv.zst` / `<DD>_ask.csv.zst`).
// `Ord` / `PartialOrd` derive added in Plan 02-02 so `Side` can serve as a key
// component in `BTreeMap<(String, Side, NaiveDate), _>` (the integration
// MockReader). The discriminant order is `Bid < Ask`; nothing in the codebase
// relies on the absolute ordering, only on its existence and stability.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Bid,
    Ask,
}

impl Side {
    /// Filename-component string form (`"bid"` / `"ask"`). Used by the Dukascopy
    /// path layout; centralised here so wire form and filesystem form cannot drift.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bid => "bid",
            Self::Ask => "ask",
        }
    }
}

/// Half-open UTC range `[start, end)`, matching Phase 1 `TimeRange` semantics.
///
/// Bars whose `ts_open_utc` falls in this range are yielded; the end timestamp is
/// exclusive (a bar at `ts_open_utc == end` is NOT yielded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedRangeUtc {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// One 1-minute OHLCV bar produced by a [`Reader`].
///
/// `ts_open_utc` is the start of the 60-second window; `ts_close_utc =
/// ts_open_utc + 60s` (the reader populates this explicitly so downstream code
/// doesn't recompute it). OHLC fields are `f64`; `tick_volume` is `f64` per D2-13.
///
/// ## `tick_volume` (A1 invariant)
///
/// Per-bar SUM of per-tick `bid_vol` / `ask_vol` floats produced by
/// `tradedesk-dukascopy/export.py:312` (`vol.resample(rule).sum()`) — NOT contract
/// volume, NOT an integer tick count. Aggregates across bars by SUM. The field
/// name is `tick_volume` everywhere; the CSV column is `volume` only at the
/// Dukascopy reader boundary and is renamed in `RawBar` (D2-13).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawBar {
    pub ts_open_utc: DateTime<Utc>,
    pub ts_close_utc: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Per-bar SUM of per-tick float volumes. Never exposed as `volume` or any
    /// integer count.
    pub tick_volume: f64,
}

/// blake3 fingerprint as a 64-byte ASCII-hex array.
///
/// Fixed-size (no `String` allocation per fingerprint) per RESEARCH Refinement #2.
/// blake3's `to_hex()` produces exactly 64 lowercase hex characters in the alphabet
/// `[0-9a-f]`, so the contents are guaranteed valid UTF-8 and ASCII; `as_str` uses
/// the checked `from_utf8` form so no `unsafe` is required.
///
/// The `Serialize` / `Deserialize` / `JsonSchema` impls are MANUAL (not derived)
/// because `serde`'s default impls cap array length at 32; the wire form is a
/// 64-char lowercase-hex string with the regex `^[0-9a-f]{64}$`. Mirrors the
/// `RunId` manual-impl pattern at `findings/run_id.rs:48-65`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Blake3Hex([u8; 64]);

impl Blake3Hex {
    /// Construct from a fixed-size hex-byte array. Caller is responsible for
    /// ensuring the bytes are ASCII hex (they are when produced by `blake3::Hash::to_hex`).
    #[must_use]
    pub fn from_hex_bytes(bytes: &[u8; 64]) -> Self {
        Self(*bytes)
    }

    /// Borrow as `&str`. blake3 hex is always valid UTF-8 (ASCII `[0-9a-f]`), so the
    /// checked `from_utf8` returns `Ok` and we surface it via `expect` — any panic
    /// here would indicate `Blake3Hex` was constructed from non-hex bytes, which the
    /// type's contract forbids.
    ///
    /// # Panics
    /// Panics if the wrapped bytes are not valid UTF-8 — impossible when constructed
    /// via [`Blake3Hex::from_hex_bytes`] with output from `blake3::Hash::to_hex`, which
    /// always emits ASCII hex `[0-9a-f]`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("Blake3Hex must contain ASCII hex bytes")
    }

    /// Borrow the raw 64-byte hex-ASCII array. Used by the cache sidecar writer
    /// (Plan 02-05) when serialising into Arrow IPC schema metadata.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl Serialize for Blake3Hex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Blake3Hex {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let s = <&str as Deserialize>::deserialize(deserializer)?;
        let bytes: &[u8; 64] = s.as_bytes().try_into().map_err(|_| {
            D::Error::custom(format!("blake3 hex must be 64 chars (got {})", s.len()))
        })?;
        for &b in bytes {
            if !b.is_ascii_hexdigit() || (b.is_ascii_alphabetic() && b.is_ascii_uppercase()) {
                return Err(D::Error::custom(
                    "blake3 hex must contain only lowercase ASCII hex chars [0-9a-f]",
                ));
            }
        }
        Ok(Self(*bytes))
    }
}

impl JsonSchema for Blake3Hex {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Blake3Hex".into()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        "miner_core::reader::Blake3Hex".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        serde_json::json!({
            "type": "string",
            "pattern": "^[0-9a-f]{64}$",
            "description": "64-character lowercase blake3 hex fingerprint of the source bytes",
        })
        .try_into()
        .expect("valid schema fragment")
    }
}

/// Iterator-of-`Result<RawBar, E>` returned by [`Reader::read_1m_bars`]. Aliased so
/// the trait signature stays readable and so `clippy::type_complexity` doesn't fire
/// on every implementation.
pub type RawBarIter<'a, E> = Box<dyn Iterator<Item = Result<RawBar, E>> + Send + 'a>;

/// Pluggable data-source trait. The aggregator, gap detector, and cache all
/// consume `&dyn Reader` so a future equity/crypto reader plugs in without
/// changing `miner-core`.
///
/// `Send + Sync` because Phase 3+ workers may share `&Reader` across rayon threads.
pub trait Reader: Send + Sync {
    /// Implementation-specific error type. Mirrors `FindingSink::Error` from
    /// Phase 1 — each reader carries its own thiserror enum.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Stable identifier for the data source (e.g. `"dukascopy"`). Used as a cache
    /// path component and in `Finding::source.source_id`.
    ///
    /// Returns `&'static str` because every reader's source-id is a compile-time
    /// constant — there is no dynamic-dispatch on this value.
    fn source_id(&self) -> &'static str;

    /// Per-source trading calendar. Phase 2-02 ships the FX-major default; a future
    /// equity-reader returns its exchange's calendar. Returned by value (with a
    /// `.clone()` internally) so the trait is dyn-compatible without lifetime
    /// gymnastics — `Calendar` is `Clone` for that reason (D2-08).
    fn trading_calendar(&self) -> Calendar;

    /// Stream 1-minute bars for `(symbol, side)` whose `ts_open_utc` falls in
    /// `range`. Bars are yielded in ascending UTC order. Errors mid-stream surface
    /// as `Err(Self::Error)` items — callers MUST handle them (e.g., abort the
    /// scan or splice the rest of the range).
    ///
    /// # Errors
    /// Returns `Err(Self::Error)` at construction time if the reader cannot start
    /// the stream (e.g., source root unreadable). Per-bar parse failures surface
    /// inside the iterator as `Err(Self::Error)` items, not as the outer `Result`.
    fn read_1m_bars<'a>(
        &'a self,
        symbol: &str,
        side: Side,
        range: ClosedRangeUtc,
    ) -> Result<RawBarIter<'a, Self::Error>, Self::Error>;

    /// blake3 hex fingerprint over the FULL `.csv.zst` bytes of one day. `None`
    /// when the day file is absent (D2-05). Used by the cache invalidator to
    /// detect upstream file edits without re-parsing the CSV.
    ///
    /// # Errors
    /// Returns `Err(Self::Error)` if the day file exists but cannot be read
    /// (permissions, IO error).
    fn fingerprint_day(
        &self,
        symbol: &str,
        side: Side,
        date: NaiveDate,
    ) -> Result<Option<Blake3Hex>, Self::Error>;

    /// Enumerate day files present for `(symbol, side)` whose UTC date falls in
    /// `range`. Returned `Vec` is ascending-sorted (RESEARCH Refinement #1).
    /// Cache-invalidator helper — does NOT parse CSV.
    ///
    /// # Errors
    /// Returns `Err(Self::Error)` if the source root cannot be walked.
    fn enumerate_days(
        &self,
        symbol: &str,
        side: Side,
        range: ClosedRangeUtc,
    ) -> Result<Vec<NaiveDate>, Self::Error>;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time regression gate (mirrors `FindingSink::trait_object_safe` at
    /// `sink.rs:399-409`). If `Reader` becomes non-dyn-compatible the workspace
    /// stops building — that's the test.
    #[test]
    fn reader_trait_object_safe() {
        fn _accept(_r: &dyn crate::reader::Reader<Error = std::io::Error>) {}
    }

    #[test]
    fn side_as_str_roundtrip() {
        assert_eq!(Side::Bid.as_str(), "bid");
        assert_eq!(Side::Ask.as_str(), "ask");
    }

    #[test]
    fn blake3_hex_as_str_matches_bytes() {
        // Construct from a known valid hex byte array (all 'a').
        let bytes = [b'a'; 64];
        let hex = Blake3Hex::from_hex_bytes(&bytes);
        assert_eq!(hex.as_str(), "a".repeat(64));
    }
}

//! tradedesk-miner Dukascopy reader.
//!
//! Phase 2 implementation of the [`miner_core::Reader`] trait for the
//! `tradedesk-dukascopy` cache layout:
//! `<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<bid|ask>.csv.zst`.
//!
//! Plan 02-01 Task 3 lands `path_layout` + boundary tests. Task 4 lands the full
//! `DukascopyReader` impl + `DukascopyError` enum + the `SyntheticCache` test fixture.

pub mod error;
pub mod path_layout;
pub mod reader;

// FROZEN public surface — extended in a backwards-compatible way only.
pub use error::DukascopyError;
pub use path_layout::{DukascopyMonth, ParsedDayPath, PathParseError, day_csv_zst, parse_day_path};
pub use reader::DukascopyReader;

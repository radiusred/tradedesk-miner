//! Task 4 placeholder — Plan 02-01 Task 4 replaces this with the full
//! `DukascopyReader` struct + `impl miner_core::Reader for DukascopyReader`.

/// Placeholder reader struct. Task 4 expands this with the `BufReader<File>` →
/// `zstd::Decoder` → `csv::Reader` pipeline + the four trait methods.
pub struct DukascopyReader;

impl DukascopyReader {
    /// Placeholder constructor — accepts the cache root for API stability but
    /// stores nothing until Task 4 wires the real implementation.
    #[must_use]
    pub fn new(_cache_root: impl Into<std::path::PathBuf>) -> Self {
        Self
    }
}

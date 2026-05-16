//! `FindingSink` trait — D-19 / Plan 03 ships the interface; Plan 04 adds `StdoutSink`.
//!
//! Per D-19, all envelope serialisation in `miner-core` flows through
//! `FindingSink::write_envelope`. The single sink writer pattern is what enforces
//! byte-identical output across CLI / MCP / HTTP wrappers (ARCHITECTURE.md
//! anti-pattern #6).

use crate::error::MinerError;
use crate::findings::Finding;

/// Sink for `Finding` envelopes.
///
/// Implementations:
/// - `StdoutSink` (Plan 04) — writes JSONL to `io::stdout`. The ONLY type in the
///   workspace that calls `Write` against `io::stdout()`.
/// - Test-only sinks for unit testing (see `VecSink` in this module behind `#[cfg(test)]`).
///
/// `Send` bound: scans (Phase 3+) may emit findings from `rayon` workers, which
/// requires the sink to be `Send`.
pub trait FindingSink: Send {
    /// Write a single `Finding` envelope. Implementations must produce one JSON
    /// object per call followed by a `\n` terminator and flush so a panic loses at
    /// most the in-flight envelope (PITFALLS #4).
    ///
    /// # Errors
    /// Returns [`MinerError::Io`] if the underlying writer fails, or
    /// [`MinerError::Serialize`] if `serde_json` cannot serialise the envelope.
    fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError>;

    /// Explicit flush. Called at the close of a run (post `RunEnd`).
    ///
    /// # Errors
    /// Returns [`MinerError::Io`] if the underlying writer fails.
    fn flush(&mut self) -> Result<(), MinerError>;
}

// ---------------------------------------------------------------------------
// Test-only in-memory sink
// ---------------------------------------------------------------------------

/// Test-only sink that captures envelopes into an in-memory `Vec<u8>` exactly as
/// `StdoutSink` will write them in Plan 04 (one JSON object per call followed by
/// `\n`).
///
/// Plan 04 will likely promote this pattern (or a generic `WriterSink<W: Write>`)
/// into a production helper for the `--output=file` path. For Plan 03 it exists
/// solely as a `FindingSink` impl that Tasks 2's object-safety test can exercise
/// and that downstream tasks can use in unit tests without hitting `io::stdout()`.
#[cfg(test)]
pub struct VecSink(pub Vec<u8>);

#[cfg(test)]
impl VecSink {
    pub fn new() -> Self {
        Self(Vec::new())
    }
}

#[cfg(test)]
impl FindingSink for VecSink {
    fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError> {
        let bytes = serde_json::to_vec(finding).map_err(MinerError::Serialize)?;
        self.0.extend_from_slice(&bytes);
        self.0.push(b'\n');
        Ok(())
    }
    fn flush(&mut self) -> Result<(), MinerError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 6 — `trait_object_safe`: `FindingSink` is object-safe, i.e., a
    /// `Box<dyn FindingSink>` compiles and dispatches correctly. This pins the
    /// trait signature so Plan 04's `StdoutSink` (and any future sink) can be
    /// stored behind a trait object inside the wrapper binaries.
    #[test]
    fn trait_object_safe() {
        let mut sink: Box<dyn FindingSink> = Box::new(VecSink::new());
        sink.flush().expect("flush ok");
        // Confirm the Send bound is honoured by trying to move the boxed sink into
        // a thread (rayon workers will need this in Phase 3+).
        let handle = std::thread::spawn(move || {
            sink.flush().expect("flush ok in thread");
        });
        handle.join().expect("thread joined");
    }
}

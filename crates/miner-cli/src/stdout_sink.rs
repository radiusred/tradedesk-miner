//! Local stub `StdoutSink` for Plan 05 — workaround while Plan 04 (which lands
//! the canonical `miner_core::findings::sink::StdoutSink` and the structured
//! `error::stderr_emit::emit_to_stderr`) runs in parallel on the same wave.
//!
//! When Plan 04 merges, this module SHOULD be deleted and `main.rs` switched
//! to `use miner_core::findings::sink::StdoutSink` directly. The local impl is
//! semantically identical to the contract described in
//! `miner-core/src/findings/sink.rs` doc comments: one JSON object per call
//! followed by `\n` against `io::stdout()`, with an explicit flush.

use std::io::{self, BufWriter, Write};

use miner_core::error::MinerError;
use miner_core::findings::{Finding, FindingSink};

/// Wraps `io::stdout()` in a `BufWriter` so the write-then-newline-then-flush
/// pattern is efficient. `Send` because `Stdout` is `Send`.
pub struct StdoutSink {
    out: BufWriter<io::Stdout>,
}

impl StdoutSink {
    /// Construct a new sink over `io::stdout()`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            out: BufWriter::new(io::stdout()),
        }
    }
}

impl Default for StdoutSink {
    fn default() -> Self {
        Self::new()
    }
}

impl FindingSink for StdoutSink {
    fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError> {
        // One JSON object per call (no pretty-printing), followed by '\n'.
        serde_json::to_writer(&mut self.out, finding).map_err(MinerError::Serialize)?;
        self.out.write_all(b"\n").map_err(MinerError::Io)?;
        // Per PITFALLS #4, flush after every envelope so a panic loses at most
        // the in-flight record. This matches the contract documented on the
        // FindingSink trait in miner-core.
        self.out.flush().map_err(MinerError::Io)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), MinerError> {
        self.out.flush().map_err(MinerError::Io)
    }
}

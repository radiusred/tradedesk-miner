//! `FindingSink` trait + `StdoutSink` impl — D-19 / Plan 04.
//!
//! Per D-19, all envelope serialisation in `miner-core` flows through
//! `FindingSink::write_envelope`. The single sink writer pattern is what enforces
//! byte-identical output across CLI / MCP / HTTP wrappers (ARCHITECTURE.md
//! anti-pattern #6).
//!
//! `StdoutSink` is the ONLY type in the workspace that opens `io::stdout()`.
//! Together with the workspace `clippy.toml` (which bans `println!` / `eprintln!`
//! / `print!` / `eprint!` / `dbg!` globally) and `stderr_emit` (the sanctioned
//! stderr writer for pre-flight errors), this is the three-layer defence
//! mitigating threat T-01-03 (stdout pollution).
//!
//! Note: `StdoutSink` writes via `serde_json::to_writer` + `Write::write_all` +
//! `Write::flush` — it never invokes a banned macro. Per RESEARCH §"Stdout/Stderr
//! Enforcement Mechanics" point 2, NO `#[allow(clippy::disallowed_macros)]`
//! attribute is needed (and adding one would mask future regressions).

use std::io::{BufWriter, Stdout, Write};

use crate::error::MinerError;
use crate::findings::Finding;

/// Sink for `Finding` envelopes.
///
/// Implementations:
/// - [`StdoutSink`] — writes JSONL to `io::stdout`. The ONLY type in the
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
// StdoutSink — the SINGLE sanctioned writer to io::stdout (D-19, T-01-03).
// ---------------------------------------------------------------------------

/// The single sanctioned writer to `io::stdout()` in the workspace.
///
/// Wraps `std::io::Stdout` in a `BufWriter` for throughput, then writes one JSON
/// object per `write_envelope` call followed by `\n` and flushes (PITFALLS #4 —
/// per-envelope flush so a panic loses at most the in-flight finding).
///
/// Per D-19 + RESEARCH §"Stdout/Stderr Enforcement Mechanics", this is the only
/// type in the workspace that constructs `std::io::stdout()`. The workspace
/// `clippy.toml` (Plan 04) bans the convenience macros (`println!`, `print!`,
/// `eprintln!`, `eprint!`, `dbg!`) so contributors cannot accidentally bypass
/// this sink.
///
/// The implementation never invokes a banned macro — it uses
/// `serde_json::to_writer`, `Write::write_all`, and `Write::flush` directly.
/// No `#[allow]` attribute is applied or needed here.
pub struct StdoutSink {
    writer: BufWriter<Stdout>,
}

impl StdoutSink {
    /// Construct a `StdoutSink` wrapping a buffered handle to `io::stdout()`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            writer: BufWriter::new(std::io::stdout()),
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
        serde_json::to_writer(&mut self.writer, finding).map_err(MinerError::Serialize)?;
        self.writer.write_all(b"\n").map_err(MinerError::Io)?;
        // Per-envelope flush — closes PITFALLS #4. A panic mid-sweep loses at most
        // the in-flight finding; the consumer never sees a torn JSON value.
        self.writer.flush().map_err(MinerError::Io)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), MinerError> {
        self.writer.flush().map_err(MinerError::Io)
    }
}

// ---------------------------------------------------------------------------
// Test-only in-memory sink — the byte-level mirror of StdoutSink.
// ---------------------------------------------------------------------------

/// Test-only sink that captures envelopes into an in-memory `Vec<u8>` exactly as
/// `StdoutSink` writes them (one JSON object per call followed by `\n`).
///
/// `VecSink` is the byte-level mirror of `StdoutSink` used by unit tests so the
/// JSONL framing can be asserted without capturing the process's actual stdout.
#[cfg(test)]
pub struct VecSink(pub Vec<u8>);

#[cfg(test)]
impl VecSink {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }
}

#[cfg(test)]
impl Default for VecSink {
    fn default() -> Self {
        Self::new()
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
#[allow(
    clippy::naive_bytecount,
    reason = "filter().count() over small in-memory test buffers is fine; pulling in the `bytecount` crate just for tests would add dep surface for negligible gain"
)]
mod tests {
    use super::*;
    use crate::findings::{RunEnd, RunId, RunStart, RunSummary};
    use chrono::{TimeZone, Utc};
    use std::sync::{Arc, Mutex};

    fn sample_run_start() -> Finding {
        Finding::RunStart(RunStart {
            run_id: RunId::new(),
            started_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            miner_version: "0.1.0".into(),
            code_revision: "abc123".into(),
            request: serde_json::json!({"scan_id": "x@1"}),
        })
    }

    fn sample_run_end() -> Finding {
        Finding::RunEnd(RunEnd {
            run_id: RunId::new(),
            ended_at_utc: Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap(),
            wall_clock_ms: 60_000,
            summary: RunSummary::default(),
        })
    }

    // ---------------------------------------------------------------------------
    // Shared writer-backed test scaffolding.
    //
    // `WriterSink<W>` is a generic `FindingSink` impl wrapping any `Write` — it
    // mirrors `StdoutSink` exactly (BufWriter + per-envelope flush) but lets the
    // test inject a custom inner writer (`Vec<u8>` for byte assertions, or a
    // `FlushCounter` for the flush-call regression gate). The point is that
    // `StdoutSink`'s observable behaviour IS exactly `WriterSink<Stdout>`, so a
    // test against `WriterSink<W>` is equivalent in shape to a test against
    // `StdoutSink` without needing to capture process stdout.
    // ---------------------------------------------------------------------------

    struct WriterSink<W: Write + Send> {
        writer: BufWriter<W>,
    }

    impl<W: Write + Send> WriterSink<W> {
        fn new(w: W) -> Self {
            Self {
                writer: BufWriter::new(w),
            }
        }
    }

    impl<W: Write + Send> FindingSink for WriterSink<W> {
        fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError> {
            serde_json::to_writer(&mut self.writer, finding)
                .map_err(MinerError::Serialize)?;
            self.writer.write_all(b"\n").map_err(MinerError::Io)?;
            self.writer.flush().map_err(MinerError::Io)?;
            Ok(())
        }
        fn flush(&mut self) -> Result<(), MinerError> {
            self.writer.flush().map_err(MinerError::Io)
        }
    }

    /// `Write` impl that records how many times `flush` was called against it.
    /// Wrapped in `Arc<Mutex>` so the test can read the count after the sink is
    /// dropped (`BufWriter` only forwards `flush()` calls to the inner writer
    /// when the buffer is non-empty AND the call is explicit, which is the
    /// per-envelope semantics we need to verify).
    struct FlushCounter {
        inner: Vec<u8>,
        flushes: Arc<Mutex<usize>>,
    }

    impl Write for FlushCounter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.inner.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            *self.flushes.lock().unwrap() += 1;
            self.inner.flush()
        }
    }

    // ---------------------------------------------------------------------------
    // Plan 04 Task 1 tests — JSONL framing + per-envelope flush.
    // ---------------------------------------------------------------------------

    /// Test 1 — writes a `Finding::RunStart` to a writer-backed sink (mirroring
    /// `StdoutSink`); the bytes contain exactly one `\n`, the prefix parses as JSON,
    /// and the parsed JSON has `"kind": "run_start"`.
    #[test]
    fn stdoutsink_writes_one_jsonl_line_per_envelope() {
        let buf: Vec<u8> = Vec::new();
        let mut sink = WriterSink::new(buf);
        sink.write_envelope(&sample_run_start())
            .expect("write_envelope ok");
        sink.flush().expect("flush ok");

        // Drop the sink to flush the BufWriter's last bytes; reconstruct the inner
        // Vec<u8> via into_inner.
        let inner = sink.writer.into_inner().expect("BufWriter into_inner");
        let newlines = inner.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(newlines, 1, "expected exactly one newline; got {newlines}");

        // The bytes before the trailing `\n` must parse as JSON.
        let payload = inner.strip_suffix(b"\n").expect("trailing newline");
        let parsed: serde_json::Value = serde_json::from_slice(payload).expect("parse JSON");
        assert_eq!(
            parsed["kind"], "run_start",
            "kind discriminator mismatch: {parsed}"
        );
    }

    /// Test 2 — writes a `RunStart` + `RunEnd`; bytes contain exactly two `\n`
    /// and split-on-newline yields two valid JSON objects.
    #[test]
    fn stdoutsink_writes_multiple_envelopes_separated_by_newline() {
        let buf: Vec<u8> = Vec::new();
        let mut sink = WriterSink::new(buf);
        sink.write_envelope(&sample_run_start())
            .expect("write_envelope ok (run_start)");
        sink.write_envelope(&sample_run_end())
            .expect("write_envelope ok (run_end)");

        let inner = sink.writer.into_inner().expect("BufWriter into_inner");
        let newlines = inner.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(newlines, 2, "expected exactly two newlines; got {newlines}");

        let mut lines: Vec<&[u8]> = inner.split(|&b| b == b'\n').collect();
        // `split` on a trailing `\n` produces a final empty slice; drop it.
        assert_eq!(lines.last().expect("at least one chunk").len(), 0);
        lines.pop();
        assert_eq!(lines.len(), 2, "expected two JSON lines");

        let first: serde_json::Value =
            serde_json::from_slice(lines[0]).expect("parse first line");
        let second: serde_json::Value =
            serde_json::from_slice(lines[1]).expect("parse second line");
        assert_eq!(first["kind"], "run_start");
        assert_eq!(second["kind"], "run_end");
    }

    /// Test 3 — `write_envelope` calls `flush()` internally on the underlying
    /// writer (PITFALLS #4). We exercise three envelopes through a
    /// `BufWriter<FlushCounter>` and assert the count is exactly 3 (one
    /// per-envelope flush, no extras). The `BufWriter::flush` forwards to the
    /// inner writer only when there are buffered bytes to flush — which is the
    /// case after each envelope is written.
    #[test]
    fn stdoutsink_flushes_per_envelope() {
        let flushes = Arc::new(Mutex::new(0usize));
        let counter = FlushCounter {
            inner: Vec::new(),
            flushes: Arc::clone(&flushes),
        };
        let mut sink = WriterSink::new(counter);
        sink.write_envelope(&sample_run_start())
            .expect("write_envelope ok 1");
        sink.write_envelope(&sample_run_end())
            .expect("write_envelope ok 2");
        sink.write_envelope(&sample_run_start())
            .expect("write_envelope ok 3");

        assert_eq!(
            *flushes.lock().unwrap(),
            3,
            "expected one flush per envelope (got {} flushes)",
            *flushes.lock().unwrap()
        );
    }

    /// Test 6 — `trait_object_safe`: `FindingSink` is object-safe, i.e., a
    /// `Box<dyn FindingSink>` compiles and dispatches correctly. This pins the
    /// trait signature so `StdoutSink` (and any future sink) can be stored behind
    /// a trait object inside the wrapper binaries.
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

    /// Additional smoke test — `StdoutSink::new()` and `Default::default()`
    /// compile and produce usable values. We do NOT call `write_envelope` here
    /// because that would write to the test runner's stdout; the byte-level
    /// behaviour is covered by Tests 1, 2, 3 via the equivalent `WriterSink<W>`.
    #[test]
    fn stdoutsink_constructs_via_new_and_default() {
        let _sink1 = StdoutSink::new();
        let _sink2: StdoutSink = StdoutSink::default();
    }
}

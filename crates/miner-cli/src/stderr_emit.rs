//! Local stub `emit_to_stderr` for Plan 05 — workaround while Plan 04 (which
//! lands the canonical `miner_core::error::stderr_emit::emit_to_stderr`) runs
//! in parallel on the same wave.
//!
//! When Plan 04 merges, this module SHOULD be deleted and `main.rs` switched to
//! `use miner_core::error::stderr_emit::emit_to_stderr` directly. The contract
//! enforced here matches D-06: one JSON line on stderr (NOT stdout), `\n`
//! terminated, flushed before exit.

use std::io::{self, Write};

use miner_core::error::WireError;

/// Emit a single [`WireError`] as one JSON line to `io::stderr()`, terminated
/// with `\n` and flushed. Returns `io::Error` on writer failure.
///
/// D-06: pre-flight rejections write a structured-error JSON line to stderr,
/// leave stdout empty, and exit with code 1. This helper is the writer; the
/// caller is responsible for `std::process::exit(1)`.
///
/// # Errors
///
/// Returns the underlying `io::Error` if stderr writes fail. The caller should
/// generally ignore the failure (we're already about to exit with code 1).
pub fn emit_to_stderr(err: &WireError) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    serde_json::to_writer(&mut stderr, err)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    stderr.write_all(b"\n")?;
    stderr.flush()
}

//! Pre-flight error emitter — writes structured-error JSON to stderr (D-06).
//!
//! D-06 requires that pre-flight rejection (unknown scan, invalid parameter,
//! missing required config, etc.) emit a single structured-error JSON line to
//! stderr while leaving stdout empty. This module is the ONLY writer of such
//! records; logging via `tracing` is a SEPARATE concern (tracing emits
//! human-readable log lines; this emits one-shot structured-error JSON for the
//! pre-flight rejection path — RESEARCH §"Config Precedence Mechanics" closing
//! paragraph).
//!
//! `stderr_emit` is the second of the two "sanctioned writer" modules. Together
//! with [`crate::findings::sink::StdoutSink`], these are the only places in
//! `miner-core` that touch `io::stdout()` or `io::stderr()` via `io::Write`.
//! Everything else uses `tracing::*!` macros (which route to stderr via the
//! tracing-subscriber initialised in each binary's `main()`).
//!
//! Per RESEARCH §"Stdout/Stderr Enforcement Mechanics" point 2 and the D-15
//! surgical reinterpretation in the plan `must_haves`: this module uses
//! `io::Write` directly (never `eprintln!`), so no
//! `#[allow(clippy::disallowed_macros)]` attribute is applied — adding one
//! would mask future regressions if a contributor slipped in a banned macro.

use std::io::{self, Write};

use crate::error::WireError;

/// Write one [`WireError`] as a single JSON line to the provided writer.
///
/// The caller chooses the writer — production code uses [`emit_to_stderr`]
/// (which wraps `io::stderr()`); tests use an in-memory `Vec<u8>`.
///
/// Output shape: `<json-encoded WireError>\n`, with the writer flushed
/// afterwards (mirrors the per-envelope flush discipline of
/// [`crate::findings::sink::StdoutSink`]).
///
/// # Errors
/// Returns [`io::Error`] if `serde_json` fails to serialise the `WireError` or
/// if the underlying writer's `write_all` / `flush` calls fail. The
/// serialisation error is wrapped via [`io::Error::other`].
pub fn write_preflight_error<W: Write>(out: &mut W, err: &WireError) -> io::Result<()> {
    serde_json::to_writer(&mut *out, err).map_err(io::Error::other)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// Production convenience: write to `std::io::stderr()`.
///
/// The CLI / MCP / HTTP wrappers call this exactly once during pre-flight
/// rejection (D-06). The structured-error JSON line lands on stderr; stdout
/// stays empty — protecting the MCP transport and the agent's JSONL parser.
///
/// # Errors
/// Returns [`io::Error`] if writing to stderr fails (extremely rare; closed
/// pipe is the typical case).
pub fn emit_to_stderr(err: &WireError) -> io::Result<()> {
    write_preflight_error(&mut io::stderr(), err)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::naive_bytecount,
    reason = "filter().count() over a small in-memory buffer is fine in tests; pulling in the `bytecount` crate just for this would add dep surface for negligible test-only gain"
)]
mod tests {
    use super::*;
    use crate::error::PreflightCode;
    use serde_json::Value;
    use std::collections::BTreeMap;

    /// Test 1 — calls `write_preflight_error` against an in-memory `Vec<u8>`;
    /// asserts the buffer contains exactly one `\n` and the prefix parses as
    /// JSON.
    #[test]
    fn write_preflight_error_emits_jsonline_to_writer() {
        let err = WireError::preflight(PreflightCode::InvalidParameter, "bad param");
        let mut buf: Vec<u8> = Vec::new();
        write_preflight_error(&mut buf, &err).expect("write_preflight_error ok");

        let newlines = buf.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(
            newlines,
            1,
            "expected exactly one newline; got {newlines} (buf={:?})",
            String::from_utf8_lossy(&buf)
        );

        let payload = buf.strip_suffix(b"\n").expect("trailing newline");
        let parsed: Value = serde_json::from_slice(payload).expect("parse JSON");
        assert!(
            parsed.is_object(),
            "expected top-level JSON object; got {parsed}"
        );
        assert_eq!(parsed["message"], "bad param");
    }

    /// Test 2 — confirms the emitted JSON contains the `snake_case` wire form
    /// for [`PreflightCode::InvalidParameter`] (per RESEARCH §"`error_code`
    /// Vocabulary").
    #[test]
    fn error_code_uses_snake_case() {
        let err = WireError::preflight(PreflightCode::InvalidParameter, "bad");
        let mut buf: Vec<u8> = Vec::new();
        write_preflight_error(&mut buf, &err).expect("write ok");

        let payload = buf.strip_suffix(b"\n").expect("trailing newline");
        let parsed: Value = serde_json::from_slice(payload).expect("parse JSON");
        assert_eq!(
            parsed["code"], "invalid_parameter",
            "expected snake_case 'invalid_parameter'; got {parsed}"
        );
    }

    /// Test 3 — adds three context entries with keys "z", "a", "m"; the
    /// serialised JSON lists them in alphabetical order (`BTreeMap` key
    /// ordering — OUT-03 groundwork).
    #[test]
    fn context_preserves_btreemap_ordering() {
        let err = WireError::preflight(PreflightCode::InvalidConfig, "missing")
            .with_context("z", Value::String("z-value".into()))
            .with_context("a", Value::String("a-value".into()))
            .with_context("m", Value::String("m-value".into()));

        let mut buf: Vec<u8> = Vec::new();
        write_preflight_error(&mut buf, &err).expect("write ok");

        let payload = buf.strip_suffix(b"\n").expect("trailing newline");
        let s = std::str::from_utf8(payload).expect("valid utf8");
        // The serialised JSON for `context` MUST list keys in alphabetical
        // order. Find the substring "context":{...} and check the key order.
        let ctx_start = s
            .find("\"context\":{")
            .expect("context key present in output");
        let after_ctx = &s[ctx_start..];
        let a_pos = after_ctx.find("\"a\"").expect("key 'a' present");
        let m_pos = after_ctx.find("\"m\"").expect("key 'm' present");
        let z_pos = after_ctx.find("\"z\"").expect("key 'z' present");
        assert!(
            a_pos < m_pos && m_pos < z_pos,
            "expected alphabetical key order in context: a < m < z; got positions a={a_pos} m={m_pos} z={z_pos} in {s}"
        );

        // Sanity round-trip: BTreeMap deserialise preserves the keys.
        let parsed: WireError = serde_json::from_slice(payload).expect("deserialise WireError");
        let keys: Vec<&String> = parsed.context.keys().collect();
        assert_eq!(keys, vec!["a", "m", "z"]);
        // BTreeMap is the runtime type — sanity check via the type system.
        let _: &BTreeMap<String, Value> = &parsed.context;
    }

    /// Additional smoke test — the `emit_to_stderr` convenience compiles and
    /// can be called. We don't assert on actual stderr contents (the test
    /// runner captures it) — Tests 1-3 cover the byte-level shape via the
    /// equivalent `write_preflight_error<W>`.
    #[test]
    fn emit_to_stderr_compiles_and_runs() {
        let err = WireError::preflight(PreflightCode::InternalError, "smoke");
        // Best-effort: stderr is always writable in a normal test environment.
        let _ = emit_to_stderr(&err);
    }
}

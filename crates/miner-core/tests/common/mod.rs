//! Shared helpers for `miner-core` integration tests (Plan 03-04 Warning 6 + Plan 03-06).
//!
//! Exposes:
//! - [`counting_sink::CountingSink`]: per-envelope counter wrapper used by
//!   future cancellation-style integration tests.
//! - [`BufferSink`]: a `FindingSink` implementation backed by `Vec<u8>` (the
//!   integration-test mirror of miner-core's lib-cfg-gated `VecSink`). Plan
//!   03-06 uses this to capture engine output in-process without spawning the
//!   binary.

#![allow(dead_code)] // each integration test consumes a different subset of helpers.

pub mod counting_sink;
pub mod synthetic_cache;

use miner_core::{Finding, FindingSink, MinerError};

/// Test-only sink that captures envelopes into an in-memory `Vec<u8>` exactly
/// as `StdoutSink` writes them (one JSON object per call followed by `\n`).
///
/// Mirrors the cfg-gated `miner_core::findings::sink::VecSink` byte-for-byte;
/// duplicated here because the lib's `VecSink` is `#[cfg(test)]`-gated and
/// therefore unreachable from integration tests under `tests/`. Integration
/// tests sharing this module via `mod common;` get the same framing semantics
/// as the lib unit tests.
#[derive(Default)]
pub struct BufferSink(pub Vec<u8>);

impl BufferSink {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Borrow the captured bytes as a UTF-8 string slice for the JSONL parse
    /// step. Panics if the bytes are not valid UTF-8 (JSON is ASCII-superset,
    /// so this never fires for well-formed envelopes).
    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("BufferSink bytes are valid utf-8 — JSON envelopes")
    }
}

impl FindingSink for BufferSink {
    fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError> {
        let bytes = serde_json::to_vec(finding).map_err(MinerError::Serialize)?;
        self.0.extend_from_slice(&bytes);
        self.0.push(b'\n');
        Ok(())
    }

    fn write_raw_json(&mut self, v: &serde_json::Value) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(v).map_err(std::io::Error::other)?;
        self.0.extend_from_slice(&bytes);
        self.0.push(b'\n');
        Ok(())
    }

    fn flush(&mut self) -> Result<(), MinerError> {
        Ok(())
    }
}

/// Recursively mask the volatile envelope fields (`run_id`, `started_at_utc`,
/// `produced_at_utc`, `ended_at_utc`, `wall_clock_ms`).
///
/// Mirrors `crates/miner-cli/tests/cli_streams.rs::mask_volatile_fields` per
/// 03-PATTERNS line 825-850; duplicated here because Cargo compiles each
/// integration-test directory as a separate crate so the two test trees
/// cannot share a `mod` block. Keep the field list in sync with the cli
/// crate's helper.
pub fn mask_volatile_fields(v: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for key in [
            "run_id",
            "started_at_utc",
            "produced_at_utc",
            "ended_at_utc",
        ] {
            if map.contains_key(key) {
                map.insert(
                    key.to_string(),
                    serde_json::Value::String(format!("<masked_{key}>")),
                );
            }
        }
        if map.contains_key("wall_clock_ms") {
            map.insert("wall_clock_ms".to_string(), serde_json::Value::from(0i64));
        }
        for (_, child) in map.iter_mut() {
            mask_volatile_fields(child);
        }
    } else if let serde_json::Value::Array(arr) = v {
        for child in arr.iter_mut() {
            mask_volatile_fields(child);
        }
    }
}

/// Parse a JSONL byte buffer into a vector of envelope `serde_json::Value`s
/// with the volatile fields masked. Convenience wrapper for the determinism
/// + golden tests.
#[must_use]
pub fn parse_and_mask_jsonl(buf: &[u8]) -> Vec<serde_json::Value> {
    let text = std::str::from_utf8(buf).expect("JSONL bytes are valid utf-8");
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut v: serde_json::Value =
                serde_json::from_str(line).expect("each JSONL line is valid JSON");
            mask_volatile_fields(&mut v);
            v
        })
        .collect()
}

/// Parse a JSONL byte buffer into typed `Finding` envelopes. The variant
/// dispatch verifies the envelope-tag invariant; the test asserts shape over
/// the resulting `Vec<Finding>`.
#[must_use]
pub fn parse_findings(buf: &[u8]) -> Vec<Finding> {
    let text = std::str::from_utf8(buf).expect("JSONL bytes are valid utf-8");
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|line| serde_json::from_str::<Finding>(line).expect("Finding parses"))
        .collect()
}

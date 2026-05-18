//! Shared helpers for `miner-core` integration tests (Plan 03-04 Warning 6).
//!
//! Currently exposes the [`counting_sink::CountingSink`] wrapper that tracks
//! per-envelope counters and supports flipping a cancel flag after the first
//! `Finding::Result` — used by the engine cancellation integration tests in
//! Plan 03-06 and beyond.
//!
//! Plan 03-04 declares this directory in its `files_modified` list; the
//! engine's lib-level `cancellation_tests` sub-module re-implements an
//! equivalent flip-on-result wrapper inline because the integration-test
//! `tests/common/*.rs` module is not in scope of the unit-test compilation
//! unit. This file exists so future integration tests can `use common::*;`.

pub mod counting_sink;

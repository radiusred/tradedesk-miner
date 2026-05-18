//! `CountingSink<S>` — `FindingSink` wrapper for integration tests.
//!
//! Plan 03-04 Warning 6 — this helper lives at
//! `crates/miner-core/tests/common/counting_sink.rs` (declared in the plan's
//! `files_modified` list). It increments per-envelope counters and optionally
//! invokes a callback when the first `Finding::Result` is observed (used to
//! flip the cancel token in the engine cancellation tests, mimicking the
//! `cancel_before_subrange` SC-5b yield site).
//!
//! Production scan output uses [`miner_core::FindingSink`] directly; this
//! wrapper is reserved for tests that need to count envelope kinds.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use miner_core::{Finding, FindingSink, MinerError};

/// A `FindingSink` decorator that counts envelopes by variant and supports an
/// `on_first_result` callback for race-deterministic cancellation tests.
///
/// `Arc<AtomicUsize>` counters are shared so the test can inspect them after
/// the sink is dropped (or while it is still alive — atomics are lock-free).
pub struct CountingSink<S: FindingSink> {
    pub inner: S,
    pub run_start_count: Arc<AtomicUsize>,
    pub result_count: Arc<AtomicUsize>,
    pub scan_error_count: Arc<AtomicUsize>,
    pub gap_aborted_count: Arc<AtomicUsize>,
    pub run_end_count: Arc<AtomicUsize>,
    pub dry_run_count: Arc<AtomicUsize>,
    pub on_first_result: Option<Box<dyn FnMut() + Send>>,
}

impl<S: FindingSink> CountingSink<S> {
    /// Wrap `inner` with fresh zero counters and no `on_first_result` hook.
    #[must_use]
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            run_start_count: Arc::new(AtomicUsize::new(0)),
            result_count: Arc::new(AtomicUsize::new(0)),
            scan_error_count: Arc::new(AtomicUsize::new(0)),
            gap_aborted_count: Arc::new(AtomicUsize::new(0)),
            run_end_count: Arc::new(AtomicUsize::new(0)),
            dry_run_count: Arc::new(AtomicUsize::new(0)),
            on_first_result: None,
        }
    }

    /// Install an `on_first_result` callback. The callback runs BEFORE the
    /// first `Finding::Result` envelope is forwarded to `inner` — perfect for
    /// flipping a cancel token between sub-ranges.
    #[must_use]
    pub fn with_on_first_result(mut self, cb: Box<dyn FnMut() + Send>) -> Self {
        self.on_first_result = Some(cb);
        self
    }
}

impl<S: FindingSink> FindingSink for CountingSink<S> {
    fn write_envelope(&mut self, finding: &Finding) -> Result<(), MinerError> {
        match finding {
            Finding::RunStart(_) => {
                self.run_start_count.fetch_add(1, Ordering::SeqCst);
            }
            Finding::Result(_) => {
                let prev = self.result_count.fetch_add(1, Ordering::SeqCst);
                if prev == 0 {
                    if let Some(cb) = self.on_first_result.as_mut() {
                        cb();
                    }
                }
            }
            Finding::ScanError(_) => {
                self.scan_error_count.fetch_add(1, Ordering::SeqCst);
            }
            Finding::GapAborted(_) => {
                self.gap_aborted_count.fetch_add(1, Ordering::SeqCst);
            }
            Finding::RunEnd(_) => {
                self.run_end_count.fetch_add(1, Ordering::SeqCst);
            }
            Finding::DryRun(_) => {
                self.dry_run_count.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.inner.write_envelope(finding)
    }

    fn write_raw_json(&mut self, v: &serde_json::Value) -> std::io::Result<()> {
        self.inner.write_raw_json(v)
    }

    fn flush(&mut self) -> Result<(), MinerError> {
        self.inner.flush()
    }
}

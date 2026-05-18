//! Phase 3 integration test — `Finding::DryRun` shape (D3-21).
//!
//! Pattern analog: `tests/cache_smoke.rs::cache_hit_skips_reader` (single-scenario
//! integration test using only FROZEN public surface).
//!
//! ## D3-21 contract
//!
//! `--dry-run` emits ONE `Finding::DryRun` envelope on stdout, wrapped by the
//! usual `RunStart` / `RunEnd` framing, then exits 0. `Finding::Result` MUST
//! NOT be emitted. `RunSummary.results_emitted` MUST be 0 (RESEARCH §Pitfall 3).
//!
//! Wave 0 scaffold: signature-only `#[test] #[ignore]` stub. Plan 03-06 fills
//! the body once the `Finding::DryRun` variant + `DryRunFinding` payload land
//! in `findings/mod.rs`.

#![allow(dead_code, unused_imports)]

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
fn dry_run_emits_dry_run_finding_only() {
    // Plan 03-06 fills:
    // 1. Construct ScanRequest with dry_run = true.
    // 2. Run through engine::run_one with VecSink.
    // 3. Parse the captured bytes — assert kinds: RunStart, DryRun, RunEnd.
    // 4. Assert NO Result finding.
    // 5. Assert RunEnd.summary.results_emitted == 0 (Pitfall 3).
    unimplemented!(
        "Plan 03-06 implements dry_run_emits_dry_run_finding_only per D3-21 / OP-05"
    )
}

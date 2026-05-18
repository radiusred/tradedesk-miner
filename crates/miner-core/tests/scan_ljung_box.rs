//! Phase 3 integration test — Ljung-Box scan golden snapshot.
//!
//! Pattern analog: `tests/gap_manifest_snapshot.rs` (full file) — construct a
//! typed value, run through the production code path, `insta::assert_json_snapshot!`.
//!
//! ## D3-05 contract
//!
//! Feed a deterministic AR(1) synthetic series (256 samples, fixed seed)
//! through the full facade (`engine::run_one` against a `VecSink` test-only
//! sink, see `findings/sink.rs:188-216`); parse the resulting JSONL; mask
//! volatile fields (`run_id`, `started_at_utc`, `produced_at_utc`,
//! `ended_at_utc`, `wall_clock_ms`); insta-snapshot the masked envelope shape.
//! Floats inside `RawArray.data` are byte-equal because the kernel summation
//! order is deterministic (03-RESEARCH §"Biased ACF" note).
//!
//! Wave 0 scaffold: signature-only `#[test] #[ignore]` stub. Plan 03-06 fills
//! the body (synthetic AR(1) builder + VecSink wiring + insta snapshot).

#![allow(dead_code, unused_imports)]

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
fn ljung_box_matches_statsmodels_golden() {
    // Plan 03-06 fills:
    // 1. Build a deterministic AR(1) BarFrame (256 samples, fixed seed) per D3-05.
    // 2. Construct ScanCtx + ScanRequest for stats.autocorr.ljung_box@1.
    // 3. Run the scan through a VecSink (sink.rs:188-216 test-only sink).
    // 4. Parse the captured bytes as JSONL.
    // 5. Mask volatile fields via mask_volatile_fields helper (cli_streams.rs:323-344).
    // 6. insta::assert_json_snapshot!(masked) against snapshots/scan_ljung_box__ljung_box_matches_statsmodels_golden.snap.
    unimplemented!(
        "Plan 03-06 implements ljung_box_matches_statsmodels_golden per 03-VALIDATION.md row OUT-04/SC-1"
    )
}

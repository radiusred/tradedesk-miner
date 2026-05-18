//! Phase 3 integration test — twice-run masked byte-equality (OUT-03 closure).
//!
//! Pattern analog: `miner-cli/tests/cli_streams.rs::emit_fixture_byte_identical_when_volatile_fields_masked`
//! (Test 7, lines 452-478) + `tests/full_determinism.rs` (synthetic-cache + in-process VecSink pattern).
//!
//! ## D3-23 contract
//!
//! Same `(scan_id@version, params, instrument, side, timeframe, window,
//! gap_policy, source bars)` → byte-identical JSONL output modulo the four
//! volatile fields (`run_id` + three timestamps + `wall_clock_ms`).
//!
//! Difference from `cli_streams.rs` Test 7: this test runs `engine::run_one`
//! IN-PROCESS (no `assert_cmd`) against a `VecSink`. Cheaper and the same
//! byte assertion.
//!
//! Wave 0 scaffold: signature-only `#[test] #[ignore]` stub. Plan 03-06 fills
//! the body.

#![allow(dead_code, unused_imports)]

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
#[serial_test::serial]
fn twice_run_byte_identical_when_volatile_fields_masked() {
    // Plan 03-06 fills:
    // 1. Build SyntheticCache via crates/miner-cli/tests/fixtures/mod.rs.
    // 2. Call engine::run_one twice against the same cache + sink twice.
    // 3. Mask volatile fields via mask_volatile_fields_in_jsonl helper.
    // 4. assert_eq!(masked1, masked2, "OUT-03 closure for scan facade");
    unimplemented!(
        "Plan 03-06 implements twice_run_byte_identical_when_volatile_fields_masked per OUT-03 / SC-7"
    )
}

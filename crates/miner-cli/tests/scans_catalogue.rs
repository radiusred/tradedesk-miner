//! Phase 3 integration test — `miner scans` introspection (OP-07).
//!
//! Pattern analog: `tests/cli_streams.rs::emit_fixture_writes_two_jsonl_lines_to_stdout`
//! (Test 1, lines 94-120) — assert_cmd subprocess + parse stdout JSONL + assert per-line kind.
//!
//! ## D3-20 contract
//!
//! `miner scans` emits ONE JSONL line per registered scan in deterministic
//! registration order (= BTreeMap iteration order). Each line shape:
//!
//! ```json
//! {"scan_id":"stats.autocorr.ljung_box","version":1,
//!  "params":{...JSON Schema...},
//!  "finding_fields":{"effect_extra_keys":["lags","q_stats","p_values","acf"],
//!                    "raw_series_keys":["returns","timestamps_ms"]}}
//! ```
//!
//! Phase 3 ships one scan (Ljung-Box) so the test asserts `lines.len() == 1`.
//! Phase 4 will extend this assertion as more scans land.
//!
//! Wave 0 scaffold: signature-only `#[test] #[ignore]` stub. Plan 03-06 fills
//! the body.

#![allow(dead_code, unused_imports)]

#[test]
#[ignore = "Wave 5 implements (Plan 03-06) — Wave 0 scaffold per VALIDATION harness"]
#[serial_test::serial]
fn scans_emits_one_line_per_registered_scan() {
    unimplemented!(
        "Plan 03-06 implements per VALIDATION OP-07/SC-6 — spawn `miner scans`, parse stdout JSONL, \
         assert lines.len() == 1, lines[0].scan_id == \"stats.autocorr.ljung_box\", \
         lines[0].version == 1, lines[0].finding_fields.effect_extra_keys is array"
    )
}

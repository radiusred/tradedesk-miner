//! Spike test — Risk 2 / Assumption A1 verification (Plan 01-02).
//!
//! Closes the only [ASSUMED] line in 01-RESEARCH §"Architecture Patterns" Pattern 2:
//! the `serde_json::json!{...}.try_into().expect("...")` conversion inside
//! `Base64Bytes::json_schema`. If this test passes, Plan 03 can implement the
//! production `Base64Bytes` exactly as written. If it fails, SUMMARY.md must
//! document the working alternative (likely field-level `#[schemars(schema_with = ...)]`)
//! and Plan 03 must be updated before Wave 3 starts.
//!
//! This test (and the `spike_base64` module it exercises) will be DELETED by Plan 03.

use miner_core::spike_base64::SpikeFinding;

#[test]
fn spike_emits_content_encoding() {
    let schema = schemars::schema_for!(SpikeFinding);
    let s = serde_json::to_string_pretty(&schema).expect("schema serialises");

    assert!(
        s.contains("\"contentEncoding\": \"base64\""),
        "schema fragment missing contentEncoding; got:\n{s}"
    );
    assert!(
        s.contains("\"contentMediaType\": \"application/octet-stream\""),
        "schema fragment missing contentMediaType; got:\n{s}"
    );
    assert!(
        s.contains("\"shape\""),
        "schema missing shape property; got:\n{s}"
    );
    assert!(
        s.contains("\"data\""),
        "schema missing data property; got:\n{s}"
    );
}

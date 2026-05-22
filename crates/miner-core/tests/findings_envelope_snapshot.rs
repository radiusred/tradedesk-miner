// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 RadiusRed

//! Locked findings-envelope snapshot test per D7-06 + Plan 07-09.
//!
//! Hand-rolled byte-equal (NOT `insta` — see 07-RESEARCH §Pitfall 8) so CI
//! runs without an `insta review` ceremony on first push. Compares masked
//! stdout against the checked-in golden at
//! `tests/goldens/envelope_snapshot.jsonl`.
//!
//! Pin coverage:
//! - FOUND-02 (stdout=findings discipline) — the envelope shape is the same
//!   shape every consumer parses.
//! - FOUND-03 (locked envelope schema) — byte-equality against the golden
//!   trips on any structural drift (renamed field, reordered map, etc.).
//! - OUT-03 (deterministic output ordering for golden-file diffing) — the
//!   second test asserts byte-identical re-run after masking.
//!
//! The invocation under test replicates the `miner emit-fixture` subcommand
//! IN PROCESS (Plan 07-09 option b: construct `RunStart` + `RunEnd` with a
//! shared `RunId` and write them through a `BufferSink`). The CLI binary
//! cannot be spawned from `miner-core` integration tests without adding
//! `assert_cmd` to dev-deps — that addition is out of scope for this plan
//! (Plan 07-06 owns the `Cargo.toml` surface in a parallel worktree).
//! Replicating `emit_fixture` in-process exercises the same `FindingSink`
//! envelope-write path so the byte-equality assertion still covers the
//! shared serialisation discipline.
//!
//! The non-`#[ignore]`d tests are active under `cargo test --workspace`
//! per ROADMAP Phase 7 success criterion #1.

mod common;

use std::collections::HashSet;

use chrono::Utc;

use common::BufferSink;
use miner_core::findings::{Finding, FindingSink, RunEnd, RunId, RunStart, RunSummary};

/// Pinned golden bytes — `include_str!` so a missing or empty file is a
/// hard compile-time error, not a silent runtime mismatch.
const GOLDEN_JSONL: &str = include_str!("goldens/envelope_snapshot.jsonl");

/// Replicate `miner emit-fixture` in-process: emit one `RunStart` envelope
/// followed by one `RunEnd` envelope through a `BufferSink`, sharing the
/// same `RunId` across both (relies on `RunId: Copy`).
///
/// All non-volatile fields are pinned to fixed values so the masked output
/// is byte-stable across runs. `RunId` + timestamps + `wall_clock_ms` are
/// the only volatile fields and are masked out before comparison.
fn run_envelope_invocation_capture_stdout() -> Vec<u8> {
    let run_id = RunId::new();
    let started = Utc::now();

    let start = Finding::RunStart(RunStart {
        run_id,
        started_at_utc: started,
        // Fixed-literal to keep the masked golden stable across `cargo` /
        // workspace version bumps — the real `emit_fixture` uses
        // `env!("CARGO_PKG_VERSION")`, but tracking the live version
        // would make the golden churn on every release bump. The byte-
        // equal contract is over envelope SHAPE, not version string.
        miner_version: "0.1.0".to_string(),
        code_revision: "test-revision-fixed".to_string(),
        request: serde_json::json!({ "command": "emit-fixture" }),
    });

    let mut sink = BufferSink::new();
    sink.write_envelope(&start)
        .expect("RunStart write_envelope ok");

    // Same time used for ended — wall_clock_ms will be 0 here, but the
    // mask helper resets it to 0 anyway, so the byte-equal contract is
    // robust to either real clock drift or fixed timestamps.
    let ended = started;
    let end = Finding::RunEnd(RunEnd {
        run_id,
        ended_at_utc: ended,
        wall_clock_ms: ended.signed_duration_since(started).num_milliseconds(),
        summary: RunSummary::default(),
    });
    sink.write_envelope(&end).expect("RunEnd write_envelope ok");
    sink.flush().expect("flush ok");

    sink.0
}

/// Parse a JSONL byte buffer, mask the 5 volatile envelope fields per
/// `common::mask_volatile_fields` (Pattern D), and re-serialise compactly
/// (one masked JSON object per output `String`). Preserves order — OUT-03
/// guarantees envelope emission ordering is stable.
fn mask_envelope_jsonl(raw: &[u8]) -> Vec<String> {
    let text = std::str::from_utf8(raw).expect("envelope JSONL bytes are valid utf-8");
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut v: serde_json::Value =
                serde_json::from_str(line).expect("envelope JSONL line is valid JSON");
            common::mask_volatile_fields(&mut v);
            serde_json::to_string(&v).expect("masked envelope re-serialises")
        })
        .collect()
}

/// Behaviour Test 1 — locked golden gate.
///
/// Run the canonical envelope-emitting invocation, mask the volatile
/// fields, and assert byte-equality against the checked-in golden.
/// Drift in any of the seven locked envelope fields, the `Finding` enum
/// variant payloads, the `BTreeMap` ordering of `RunSummary.per_scan`,
/// or the `serde_json` compact-form output trips this test.
#[test]
fn envelope_snapshot_matches_golden() {
    let raw = run_envelope_invocation_capture_stdout();
    let masked_lines = mask_envelope_jsonl(&raw);
    let actual = format!("{}\n", masked_lines.join("\n"));
    assert_eq!(
        actual, GOLDEN_JSONL,
        "envelope snapshot drift — regenerate via\n  \
         cargo test -p miner-core --test findings_envelope_snapshot -- --ignored regenerate_envelope_snapshot_golden\n\
         and commit the updated golden ONLY if the schema-evolution rationale\n\
         is documented in the same PR (D7-06)."
    );
}

/// Behaviour Test 2 — OUT-03 byte-identical-re-run contract.
///
/// Run the same invocation twice in the same test process; mask both
/// outputs; assert mask(run1) == mask(run2). Proves OUT-03 (deterministic
/// ordering for golden-file diffing) end-to-end across the envelope's
/// serialised form. If a `HashMap` ever sneaks into a payload field, this
/// test fires non-deterministically.
#[test]
fn envelope_snapshot_byte_identical_across_runs() {
    let raw1 = run_envelope_invocation_capture_stdout();
    let raw2 = run_envelope_invocation_capture_stdout();
    let masked1 = mask_envelope_jsonl(&raw1);
    let masked2 = mask_envelope_jsonl(&raw2);
    assert_eq!(
        masked1, masked2,
        "OUT-03 closure: masked envelopes from two in-process emit-fixture\n\
         invocations differ.\nRun 1:\n{}\nRun 2:\n{}",
        masked1.join("\n"),
        masked2.join("\n"),
    );
}

/// Behaviour Test 3 — variant-coverage assertion.
///
/// The golden must contain at least the expected envelope variants
/// (`run_start` + `run_end` — the two envelopes the emit-fixture invocation
/// emits). Implemented by reading the in-test invocation's masked output
/// and walking each line's `"kind"` discriminant. Adding a new variant to
/// the emit-fixture path (e.g., a `dry_run` framing record) would be a
/// deliberate change; this test forces the golden author to make the
/// expected-set update explicit.
#[test]
fn envelope_snapshot_covers_all_emitted_variants() {
    let raw = run_envelope_invocation_capture_stdout();
    let text = std::str::from_utf8(&raw).expect("utf-8");
    let kinds: HashSet<String> = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
            v.get("kind")
                .and_then(|k| k.as_str())
                .expect("every envelope carries a 'kind' discriminant")
                .to_string()
        })
        .collect();

    // The emit-fixture invocation emits exactly these two framing
    // envelopes per D-09 / D-11; a future plan that adds another variant
    // to the path must update this set in the same PR.
    let expected: HashSet<String> =
        ["run_start", "run_end"].iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        kinds, expected,
        "envelope-snapshot variant coverage drift: expected {expected:?}, got {kinds:?}.\n\
         If a new variant was intentionally added to the emit-fixture path,\n\
         update the `expected` set here AND regenerate the golden."
    );
}

/// Regen helper — operator-triggered ONLY. Writes the current masked
/// envelope bytes to `crates/miner-core/tests/goldens/envelope_snapshot.jsonl`.
/// Run via:
///
///     cargo test -p miner-core --test findings_envelope_snapshot \
///         -- --ignored regenerate_envelope_snapshot_golden
///
/// Re-running the golden via this helper is a documented schema-evolution
/// step per D7-06. The committed change MUST include a rationale entry in
/// the same PR — otherwise the byte-equal gate is a no-op (any schema
/// drift can be silently masked by re-running the regen helper).
#[test]
#[ignore = "regen-only: writes the golden — operator-triggered via --ignored"]
fn regenerate_envelope_snapshot_golden() {
    let raw = run_envelope_invocation_capture_stdout();
    let masked_lines = mask_envelope_jsonl(&raw);
    let golden_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens/envelope_snapshot.jsonl");
    let body = format!("{}\n", masked_lines.join("\n"));
    std::fs::write(&golden_path, body).expect("write golden");
    eprintln!("[regenerate] wrote {}", golden_path.display());
}

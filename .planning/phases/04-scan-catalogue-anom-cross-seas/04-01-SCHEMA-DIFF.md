# Plan 04-01 Schema Regen Spike — Schema Diff and D4-01 / D4-03 Decision Memo

**Date:** 2026-05-19
**Spike:** Plan 04-01 Task 1 (D4-01 / D4-03 schema-additive verification per 04-RESEARCH §1.1, §Section 7, Pitfall 7, Pitfall 8)

## Overview

Phase 4 introduces two facade-shape changes that must be classified against the
project's schema-sync CI gate (`git diff --exit-code schemas/`) BEFORE callsite
code is changed:

- **D4-01** — `ScanRequest.instrument: String + side: Side` generalises to
  `ScanRequest.instruments: Vec<InstrumentSpec>`.
- **D4-03** — `DataSlice` gains a leg-labelled `sources: Vec<Source>` field
  (replacing the singleton `source: Source` field currently living on
  `ResultFinding` and `GapAbortedFinding`).

This memo records the regenerated-schema diff for the SCRATCH spike and locks
the D4-01 / D4-03 implementation path for the downstream Plan 04-01 tasks.

## Findings v1 Schema Diff

Spike steps performed:

1. Added three workspace dependencies (ndarray 0.16, ndarray-stats 0.6,
   nalgebra 0.33) to root `Cargo.toml` `[workspace.dependencies]` and mirrored
   into `crates/miner-core/Cargo.toml`. `cargo build -p miner-core` succeeded
   with no resolver errors.
2. Applied SCRATCH type-change: added `pub sources: Vec<Source>` field to
   `DataSlice` (with `#[serde(default)]`). Patched the four in-crate
   `DataSlice` struct-literal construction sites (`engine/mod.rs` lines 268,
   343, 584; `scan/ljung_box/mod.rs` line 200; `findings/mod.rs:382`
   `sample_data_slice()` test helper) with `sources: Vec::new(),` so the
   workspace compiles for the spike. No changes were made to `ScanRequest` —
   `ScanRequest` does NOT derive `JsonSchema` (it is engine-internal; the wire
   form lives in `RunStart.request: serde_json::Value`), so D4-01 cannot
   produce a `schemas/findings-v1.schema.json` diff. The InstrumentSpec struct
   was likewise NOT pre-introduced for this spike since the schema impact of
   adding a `JsonSchema`-deriving struct under `ScanRequest` is zero.
3. Ran `cargo run -p xtask -- gen-schema /tmp/schemas-scratch`. Both schema
   files regenerated successfully.
4. Compared against the committed `schemas/findings-v1.schema.json`:

```diff
--- schemas/findings-v1.schema.json	2026-05-19 23:27:23.649661248 +0100
+++ /tmp/schemas-scratch/findings-v1.schema.json	2026-05-19 23:34:03.711013649 +0100
@@ -29,6 +29,14 @@
         },
         "range": {
           "$ref": "#/$defs/TimeRange"
+        },
+        "sources": {
+          "default": [],
+          "description": "Phase 4 SCRATCH SPIKE (D4-03): leg-labelled source vector. Length =\nscan arity (1 for ANOM/SEAS, 2 for CROSS). Self-describing per-finding.\nReplaces the previous `source: Source` field on `ResultFinding` and\n`GapAbortedFinding`.",
+          "items": {
+            "$ref": "#/$defs/Source"
+          },
+          "type": "array"
         }
       },
       "required": [
```

Diff line count: 17 lines (one hunk).

## Catalogue v1 Schema Diff

```diff
(no diff — schemas/scans-catalogue-v1.schema.json byte-identical between
committed artifact and /tmp/schemas-scratch/scans-catalogue-v1.schema.json)
```

Diff line count: 0 lines.

Rationale: the `scans-catalogue-v1.schema.json` schema is derived from the
`ScansCatalogueEntry` xtask-local shim, which has no `DataSlice`,
`ScanRequest`, or `Source` references. Plan 04-01 does not yet add an `arity`
field to that shim — Plan 04-05 / Plan 06 wires the `miner scans` catalogue
output to surface `Scan::arity()`. This memo's catalogue-schema baseline is
zero-diff and that's the expected steady-state for Plan 04-01.

## Classification

Per 04-RESEARCH §Section 7 "What additive means":

- **Truly additive** (no schema_version bump, no diff): adding an optional
  field with `#[serde(default)]` whose absence in old data is valid. The new
  field appears in `properties` map and `required` list stays unchanged.

The D4-03 spike output PASSES the "truly additive" classification:

- Added `sources` field is bare `#[serde(default)]` — defaults to empty
  `Vec<Source>` when absent.
- The `required` array on `DataSlice` is unchanged (`sources` does not appear
  in `required` because of the `#[serde(default)]` attribute).
- No removed fields, no renamed fields, no type changes.

This contradicts the conservative prediction in 04-RESEARCH §1.1 paragraph
"Verdict (HIGH confidence ...)" which assumed `Vec<T>` field addition to a
JsonSchema-deriving struct WOULD necessitate option (a) "accept the schema
regen as a deliberate Phase-4 facade-shape update". The actual outcome is the
strongest possible classification — additive AND with no `required` change.

**Conclusion for D4-03:** Additive. No `schema_version` bump. The committed
`schemas/findings-v1.schema.json` regenerates with the diff above and the
schema-sync gate (`git diff --exit-code schemas/`) PASSES on the FIRST
regen-and-commit cycle (the diff is the regen output, committed as part of
Plan 04-01 Task 3).

**Conclusion for D4-01:** Zero schema impact (ScanRequest is not
`JsonSchema`-derived). The wire form for `instruments` lives in
`RunStart.request: serde_json::Value` which is opaque to schemars. The wire
form for `Finding::Result.data_slice.sources` IS the leg-provenance contract
Phase 6 wrappers will mirror.

## Decision

**D4-03 — chosen path: D4-03 (full Vec generalisation on DataSlice).**

Rationale: The schema diff is purely additive (no `required` change, no field
removal, default `[]`). The D4-03-ALT fallback (`source` + `peer_sources` Vec
sibling) is NOT required and is rejected as the implementation path. The
single Vec is cleaner — no privileged-primary leg, no plural/singular ambiguity
— and is the path 04-RESEARCH §1.1 recommended as primary.

## Decision: D4-01 instruments Vec

**D4-01 — chosen path: full Vec generalisation on ScanRequest
(`instruments: Vec<InstrumentSpec>` replaces `instrument: String + side: Side`).**

Rationale: ScanRequest is NOT `JsonSchema`-derived; the JSON Schema artifact
is unaffected. The wire-form surface for D4-01 is internal to the engine
(`ScanRequest`) plus the `RunStart.request` echo (opaque `serde_json::Value`).
The CLI value-parser change is purely the `ScanArgs::instruments: Vec<InstrumentSpec>`
field plus a value-parser that splits `SYMBOL:side`. No schemars surface
gates this decision; the D4-01-ALT fallback (additive `peer_instruments` Vec
sibling) is NOT required and is rejected as the implementation path.

## Side Note: Plan baseline vs current code (deviation)

The PLAN.md `<interfaces>` block stated:

> From crates/miner-core/src/findings/mod.rs (Phase 3):
> - pub struct DataSlice { pub source: Source, pub time_range: TimeRange,
>   pub gap_manifest: Option<GapManifest>, ... }

The actual current shape of `DataSlice` (Phase 3 head, commit dd7c709) is:

```rust
pub struct DataSlice {
    pub range: TimeRange,
    pub gap_manifest_ref: Option<String>,
    #[serde(default)] pub gap_manifest: Option<GapManifest>,
}
```

`source: Source` lives directly on `ResultFinding` and `GapAbortedFinding` (NOT
on `DataSlice`). The D4-03 implementation in Task 2 will therefore ADD
`sources: Vec<Source>` to `DataSlice` and REMOVE `source: Source` from
`ResultFinding` and `GapAbortedFinding` (Rule 3 deviation — corrects plan's
incorrect baseline; the spirit of D4-03 is preserved: per-finding leg
provenance lives on `DataSlice`).

## FOUND-04 Sync-Only Invariant

`cargo tree -p miner-core --edges normal 2>/dev/null | grep -cE 'tokio|async-std|smol'`
returns **0** post-add. The 11 matches surfaced by the unqualified
`cargo tree -p miner-core | grep -cE 'tokio|async-std|smol'` are pre-existing
dev-dep transitives from `miner-reader-dukascopy` (declared as a dev-dep on
`miner-core` for the `full_determinism.rs` integration test per
`crates/miner-core/Cargo.toml:74-78`); they are NOT introduced by Phase 4's
three new workspace deps and they are NOT in the lib graph. The FOUND-04
contract — "miner-core is sync + rayon only" — is preserved on the lib
(`--edges normal`) graph as it was pre-Phase 4.

## Idempotency

Running `cargo run -p xtask -- gen-schema` a second time produces zero diff
against the first regen output (verified post-revert by re-running on the
restored baseline: `git diff --exit-code schemas/` exits 0). The
schemars 1.x + BTreeMap-backed serde_json pipeline is deterministic by
construction (workspace `Cargo.toml` determinism note + `xtask/main.rs:98-110`
the determinism pipeline comment block).

## Spike Cleanup

All scratch type changes have been reverted:

- `crates/miner-core/src/findings/mod.rs` — restored from `/tmp/findings-mod-rs.bak`.
- `crates/miner-core/src/scan/mod.rs` — restored from `/tmp/scan-mod-rs.bak`.
- `crates/miner-core/src/engine/mod.rs` — reverted via `git checkout --`.
- `crates/miner-core/src/scan/ljung_box/mod.rs` — reverted via `git checkout --`.

Persistent changes from Task 1 (committed in this task's commit):

- `Cargo.toml` workspace dependency additions (ndarray, ndarray-stats, nalgebra).
- `crates/miner-core/Cargo.toml` `[dependencies]` mirror.
- This memo (`.planning/phases/04-scan-catalogue-anom-cross-seas/04-01-SCHEMA-DIFF.md`).

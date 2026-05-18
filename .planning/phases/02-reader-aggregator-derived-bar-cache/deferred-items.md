# Phase 02 — Deferred Items

Pre-existing issues discovered during Plan 02-05 execution. NOT in scope for this plan.

## rustfmt drift in Plan 02-03 integration tests

`cargo fmt --all --check` fails on three integration test files that were committed before this plan started (base commit `a4c8366`):

- `crates/miner-core/tests/aggregator_edge_cases.rs`
- `crates/miner-core/tests/dst_fall_back.rs`
- `crates/miner-core/tests/dst_spring_forward.rs`

Each has multiple `assert!(idx + 1 < frame.len(), "..." )` lines that exceed the wrap width. These should be reformatted in a follow-up plan / quick-fix. They are not Plan 02-05's concern.

Plan 02-05 verifies its own files (`cache.rs`, `cache/fingerprints.rs`, `cache_smoke.rs`, `arrow_schema_snapshot.rs`, `lib.rs`) are `fmt`-clean.

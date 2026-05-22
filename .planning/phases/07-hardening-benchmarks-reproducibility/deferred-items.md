# Phase 7 — Deferred Items

Out-of-scope discoveries logged during plan execution. Each entry names the
discoverer (plan that found it) and the proposed owner (plan or follow-up).

## From Plan 07-06 (2026-05-22)

### `cargo clippy --workspace --all-targets -- -D warnings` fails on `crates/miner-bench/src/bin/gen-fixtures.rs`

**Discovered:** Plan 07-06 Task 2 verification of the workspace-wide clippy gate.

**Symptom:** `cargo clippy --workspace --all-targets -- -D warnings` exits non-zero
with 4 errors in `crates/miner-bench/src/bin/gen-fixtures.rs` (the `format_collect`
lint on the SHA256 hex-encoding loops at lines 195-196). The errors are
pre-existing — they were introduced by Plan 07-02 when `gen-fixtures.rs`
landed; the Phase 7 wave-0 acceptance gates ran clippy WITHOUT `-D warnings`
strict, so the errors did not block the Plan 07-02 commit.

**Why deferred:** Plan 07-06's scope is `crates/miner-core/benches/`. The
`gen-fixtures.rs` errors are out of scope per the GSD scope-boundary rule
("Only auto-fix issues DIRECTLY caused by the current task's changes").
Plan 07-06's stricter `cargo clippy -p miner-core --benches -- -D warnings`
gate passes — that is the gate the plan acceptance criteria explicitly
require.

**Proposed owner:** A follow-up cleanup plan in Phase 7 (or Phase 8) that
runs the full `--all-targets -D warnings` gate and fixes the remaining
pre-existing lints crate-wide. The mechanical fixes are:
- `crates/miner-bench/src/bin/gen-fixtures.rs:195` — replace
  `.map(|b| format!("{b:02x}")).collect::<String>()` with a `write!`-into-
  `String` loop (clippy's `format_collect` lint).

**Concrete patch suggestion (un-applied, for the follow-up):**
```rust
use std::fmt::Write as _;
let digest_hex = digest.iter().fold(String::with_capacity(64), |mut acc, b| {
    write!(acc, "{b:02x}").expect("writing to String never fails");
    acc
});
```

**Risk if left:** None for Plan 07-06 acceptance. The workspace-wide
clippy gate in CI may already accept these warnings (depending on the
existing CI clippy invocation); if a future plan tightens CI to
`-D warnings` workspace-wide, this becomes a blocker for that plan.

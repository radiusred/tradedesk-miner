# Phase 07 — Deferred Items

Out-of-scope discoveries logged by executors during plan execution. These
items were observed but do NOT belong to the current plan's scope; they are
tracked here so the phase verifier (or a follow-on plan) can address them.

## From Plan 07-03 (cargo audit + cargo deny CI gates)

### 1. Pre-existing `[[bench]]` declarations in `crates/miner-core/Cargo.toml` reference files that do not yet exist

- **Discovered while:** Attempting to run `cargo deny check` locally against
  the workspace to verify the new `deny.toml`.
- **Evidence:** `cargo-deny 0.18.3` failed with:
  ```
  can't find `bench_aggregate_1m_to_15m` bench at
  `benches/bench_aggregate_1m_to_15m.rs` or
  `benches/bench_aggregate_1m_to_15m/main.rs`.
  ```
  `crates/miner-core/benches/` does not exist; multiple `[[bench]]` entries
  in `crates/miner-core/Cargo.toml` (lines ~143+) declare bench targets
  whose source files have not been authored yet.
- **Scope:** Phase 07 bench-harness work — owned by Plan 07-06 / 07-07 (the
  criterion microbenches and `miner-bench` recipe runner). Not introduced by
  this plan and not in this plan's `<files>` list.
- **Impact on 07-03:** None for CI — `EmbarkStudios/cargo-deny-action@v2` in
  CI installs cargo-deny 0.19.6+ which handles workspaces with declared
  benches whose source files are missing. Local verification was skipped per
  the plan's explicit acceptance-criteria fallback ("trust the GH Action that
  runs in Task 2 ... OR document in the SUMMARY that local verification was
  skipped and CI is the gate").
- **Resolution:** Plan 07-06 created the missing `benches/*.rs` files; this
  item is now resolved (kept here for historical traceability).

### 2. `cargo-deny 0.18.3` cannot parse RUSTSEC entries that use CVSS 4.0

- **Discovered while:** Same local verification attempt as item 1.
- **Evidence:**
  ```
  failed to load advisory database: parse error: error parsing
  .../advisory-db-.../crates/wasmtime/RUSTSEC-2026-0022.md:
  unsupported CVSS version: 4.0
  ```
- **Scope:** This is a tooling-version-mismatch issue: cargo-deny 0.19.6
  (which the plan targets) handles CVSS 4.0 entries; cargo-deny 0.18.3 (the
  highest version compatible with the workspace's pinned rustc 1.85) does
  not. The CI runner upgrades rustc automatically and uses cargo-deny 0.19.6
  via `EmbarkStudios/cargo-deny-action@v2`, so this only affects local runs
  on a 1.85-pinned host.
- **Impact on 07-03:** None — CI gate is the canonical check.
- **Recommendation:** No action required. Document in CONTRIBUTING.md /
  README at v1.x time if local cargo-deny becomes a contributor expectation
  (would require bumping `rust-toolchain.toml` to 1.88+).

## From Plan 07-06 (criterion microbenches)

### 3. `cargo clippy --workspace --all-targets -- -D warnings` fails on `crates/miner-bench/src/bin/gen-fixtures.rs`

- **Discovered while:** Plan 07-06 Task 2 verification of the workspace-wide
  clippy gate.
- **Symptom:** `cargo clippy --workspace --all-targets -- -D warnings` exits
  non-zero with 4 errors in `crates/miner-bench/src/bin/gen-fixtures.rs` (the
  `format_collect` lint on the SHA256 hex-encoding loops at lines 195-196).
  The errors are pre-existing — they were introduced by Plan 07-02 when
  `gen-fixtures.rs` landed; the Phase 7 wave-0 acceptance gates ran clippy
  WITHOUT `-D warnings` strict, so the errors did not block the Plan 07-02
  commit.
- **Why deferred:** Plan 07-06's scope is `crates/miner-core/benches/`. The
  `gen-fixtures.rs` errors are out of scope per the GSD scope-boundary rule
  ("Only auto-fix issues DIRECTLY caused by the current task's changes").
  Plan 07-06's stricter `cargo clippy -p miner-core --benches -- -D warnings`
  gate passes — that is the gate the plan acceptance criteria explicitly
  require.
- **Proposed owner:** A follow-up cleanup plan in Phase 7 (or Phase 8) that
  runs the full `--all-targets -D warnings` gate and fixes the remaining
  pre-existing lints crate-wide. The mechanical fixes are:
  - `crates/miner-bench/src/bin/gen-fixtures.rs:195` — replace
    `.map(|b| format!("{b:02x}")).collect::<String>()` with a `write!`-into-
    `String` loop (clippy's `format_collect` lint).
- **Concrete patch suggestion (un-applied, for the follow-up):**
  ```rust
  use std::fmt::Write as _;
  let digest_hex = digest.iter().fold(String::with_capacity(64), |mut acc, b| {
      write!(acc, "{b:02x}").expect("writing to String never fails");
      acc
  });
  ```
- **Risk if left:** None for Plan 07-06 acceptance. If a future plan tightens
  CI to `-D warnings` workspace-wide, this becomes a blocker for that plan.

## From Plan 07-08 (miner-bench recipe runner + dhat profiling)

### 5. Pre-existing clippy errors under `--all-features` confirmed still present

- **Discovered while:** Plan 07-08 Task 1 verification — running
  `cargo clippy -p miner-bench --all-targets --all-features -- -D warnings`
  per the acceptance criteria.
- **Status:** This is the SAME breakage logged in item 3 above
  (`crates/miner-bench/src/bin/gen-fixtures.rs` — 4 errors: 2× `doc_markdown`
  on line 8, 1× `cast_precision_loss` on line 98, 1× `format_collect` on
  lines 193-196). Plan 07-08 did NOT introduce any of these; the new
  `crates/miner-bench/src/main.rs` (recipe runner) is clean under both
  `cargo clippy -p miner-bench --bin miner-bench -- -D warnings` and
  `cargo clippy -p miner-bench --bin miner-bench --features dhat -- -D warnings`.
- **Workspace-wide breakage also confirmed:** `cargo clippy --workspace
  --all-targets -- -D warnings` (the CI gate from `.github/workflows/ci.yml`
  line 43) also fails on main HEAD with:
  - `crates/miner-core/tests/noise_replay_regression.rs:330` —
    `clippy::len_zero` (`families.len() >= 1`)
  - `crates/miner-core/tests/findings_envelope_snapshot.rs` —
    `clippy::disallowed_macros` (`std::eprintln!` use from Plan 07-09)
  - Plus the gen-fixtures.rs errors above.
- **Scope:** None caused by 07-08. Per the GSD scope-boundary rule, 07-08
  does not auto-fix these. The 07-08 acceptance criterion `cargo clippy
  -p miner-bench --all-targets --all-features -- -D warnings` cannot pass
  while item 3 above is outstanding; the SUMMARY documents this exception
  and points at the 07-08 binary-scoped clippy invocations
  (`cargo clippy -p miner-bench --bin miner-bench [--features dhat] --
  -D warnings`) which DO pass cleanly.
- **Proposed owner:** A follow-up `chore(07)` plan that batches all of items
  3, 4, and 5 into a single clippy-cleanup PR, restoring the workspace-wide
  `-D warnings` gate.

## From Plan 07-09 (locked findings-envelope snapshot test)

### 4. Pre-existing `cargo clippy -p miner-core --lib -- -D warnings` failures in hygiene modules

- **Discovered while:** Running `cargo clippy -p miner-core --test
  findings_envelope_snapshot -- -D warnings` per Plan 07-09 acceptance
  criteria (the new test file itself emits zero clippy warnings).
- **Evidence:** 6 errors on main HEAD (pre-existing — not introduced by this
  plan), all in:
  - `crates/miner-core/src/engine/hygiene_dispatch.rs:576` (3× `doc_markdown`
    — "item in documentation is missing backticks")
  - `crates/miner-core/src/engine/hygiene_dispatch.rs:626` (`items_after_statements`)
  - `crates/miner-core/src/engine/hygiene_dispatch.rs:654` (`manual_let_else`)
  - `crates/miner-core/src/scan/hygiene/null.rs:372` (`explicit_iter_loop`)
- **Scope:** Pre-existing in lib code from Phase 5 hygiene work; unrelated to
  Plan 07-09's `tests/findings_envelope_snapshot.rs` + `tests/goldens/
  envelope_snapshot.jsonl` deliverables.
- **Impact on 07-09:** None for the snapshot test itself — the test file
  passes clippy with zero warnings. The pre-existing lib errors block the
  acceptance criterion's `cargo clippy -p miner-core --test
  findings_envelope_snapshot -- -D warnings` invocation only because clippy
  compiles the lib as a dependency of the integration test.
- **Note:** Plan 07-06 (merged after 07-09) applied Rule 3 clippy fixes to
  several of these lints in `hygiene_dispatch.rs` and `null.rs`. A focused
  follow-up should re-run the gate to confirm what remains.
- **Recommendation:** A follow-on hygiene-module cleanup PR should address
  whatever remains. Out of scope for the byte-determinism gate this plan ships.

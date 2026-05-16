---
phase: 01-foundations-contracts
plan: 04
subsystem: foundations
tags: [rust, stdout-discipline, stderr-discipline, clippy, disallowed-macros, finding-sink, stdout-sink, stderr-emit, ci-gate, threat-mitigation-t-01-03, found-02, out-01]

# Dependency graph
requires: [plan-01-01, plan-01-02, plan-01-03]
provides:
  - "`StdoutSink` — the SINGLE sanctioned writer to `io::stdout()` in the workspace (D-19)"
  - "`StdoutSink::write_envelope` writes `serde_json::to_writer(&BufWriter<Stdout>) + write_all(b\"\\n\") + flush` per envelope, mirroring D-19 + PITFALLS #4 (per-envelope flush so a panic loses at most the in-flight finding)"
  - "`StdoutSink: FindingSink` impl + `Default` + `#[must_use] fn new()` — Plan 05's `emit-fixture` path is now wireable"
  - "`error::stderr_emit::write_preflight_error<W: Write>(out, &WireError) -> io::Result<()>` — the generic primitive (tests inject Vec<u8>)"
  - "`error::stderr_emit::emit_to_stderr(&WireError) -> io::Result<()>` — production helper wrapping `io::stderr()`"
  - "Workspace `clippy.toml` at the repo root banning `std::println`, `std::print`, `std::eprintln`, `std::eprint`, `std::dbg`. From this plan forward every PR is mechanically rejected if it slips one in outside the two sanctioned exemptions."
  - "Sanctioned exemptions are explicit and audited: `crates/miner-core/build.rs` (cargo build-script protocol uses `println!` by design) via crate-level `#![allow(clippy::disallowed_macros)]`; `xtask/src/main.rs` (already carried `#![allow]` from Plan 01-01)."
  - "CI-gate-clean workspace: `cargo clippy --workspace --all-targets -- -D warnings` exits 0 (the contract underpinning Plan 07's `.github/workflows/ci.yml`)."
  - "Three-layer T-01-03 defence is fully assembled: (1) StdoutSink is the only stdout writer, (2) clippy.toml bans the convenience macros workspace-wide, (3) stderr_emit is the sanctioned stderr writer for structured pre-flight errors — eprintln! is never the right answer."
affects: [plan-01-05, plan-01-06, plan-01-07, phase-02, phase-03, phase-04, phase-05, phase-06, phase-07]

# Tech tracking
tech-stack:
  added: []  # no new dependencies; uses existing serde_json + std::io::Write
  patterns:
    - "`BufWriter<Stdout>` wrapping inside `StdoutSink` for throughput; per-envelope `flush()` so SIGINT/panic loses at most the in-flight finding (PITFALLS #4)"
    - "Generic writer primitive (`write_preflight_error<W: Write>`) + production wrapper (`emit_to_stderr`) — same shape as StdoutSink, lets tests inject Vec<u8>"
    - "Two-sanctioned-writers pattern: every byte that leaves miner-core via stdout or stderr flows through one of two auditable code paths (StdoutSink for findings; stderr_emit for one-shot structured pre-flight errors). Everything else uses `tracing` -> the subscriber-routed stderr stream."
    - "Workspace `clippy.toml` with `disallowed-macros` as the lint gate. Exemptions are CRATE-LEVEL `#![allow(clippy::disallowed_macros)]` on the two legitimate exception cases (build.rs + xtask); no per-call-site allows anywhere."
    - "WriterSink<W: Write + Send> test scaffolding — mirrors StdoutSink's observable behaviour against any inner writer (Vec<u8> for byte assertions, FlushCounter for the per-envelope-flush regression gate). Lets unit tests verify JSONL framing + flush discipline without capturing process stdout."

key-files:
  created:
    - "clippy.toml"
  modified:
    - "crates/miner-core/src/findings/sink.rs (added StdoutSink struct + Default + FindingSink impl; extended tests module with WriterSink<W> + FlushCounter + Tests 1/2/3 + new() smoke test; tightened VecSink to add Default + #[must_use])"
    - "crates/miner-core/src/error/stderr_emit.rs (replaced Plan 03 placeholder with write_preflight_error + emit_to_stderr; 3 behavioural tests + 1 smoke test)"
    - "crates/miner-core/build.rs (added crate-level #![allow(clippy::disallowed_macros)] for the cargo build-script protocol; rewrote .map().unwrap_or_else() -> .map_or_else() to clear the pre-existing clippy::map_unwrap_or warning carried forward from Plan 01-02)"
    - "crates/miner-core/src/findings/mod.rs (added # Errors section to Raw::new docstring; scoped clippy::no_effect_underscore_binding allow to run_id_is_copy with a `reason` explaining the bindings ARE the test; tightened three doc backticks in Tests 6/7/8)"
    - "crates/miner-core/src/error/codes.rs (one doc-backtick fix on a Test 1 docstring)"
    - "crates/miner-core/src/lib.rs (one doc-backtick fix in the module docstring)"
    - "crates/miner-cli/src/main.rs (added #![allow(clippy::unnecessary_wraps)] for the placeholder main with rationale comment — Plan 05 will use ? on real fallible ops and the lint will self-correct)"

key-decisions:
  - "D-15 surgical reinterpretation HONOURED: neither StdoutSink nor write_preflight_error carries `#[allow(clippy::disallowed_macros)]`. Both use `io::Write` directly (`serde_json::to_writer` + `Write::write_all` + `Write::flush`) — they NEVER invoke a banned macro. Adding the allow attribute would weaken discipline by silently accepting any later `println!`/`eprintln!` slipped in. Confirmed by the post-Task-3 grep gates returning 0 for both modules."
  - "Pre-existing `clippy::map_unwrap_or` warning in build.rs (deferred-item from Plan 01-02 and explicitly flagged in the environment prompt) FIXED rather than allowed. Rewrote `.map(...).unwrap_or_else(...)` → `.map_or_else(...)`. This is the right-side fix because `clippy::map_unwrap_or` is a genuine clarity lint, not noise, and the rewrite is a one-character delta."
  - "build.rs gets the `#![allow]` exemption (not `#[allow]` at call sites) because `println!(\"cargo:rustc-env=...\")` is the cargo build-script PROTOCOL — there's no alternative API. The exemption is audited and scope-limited (only build scripts; the lint still fires on every other source file in the workspace including miner-core proper)."
  - "`miner-cli/src/main.rs` gets `#![allow(clippy::unnecessary_wraps)]` (NOT `clippy::disallowed_macros`!) because the placeholder's `Ok(())` body triggers the lint today, even though the `anyhow::Result<()>` return type is the right binary-edge shape per D-18 and will become genuinely fallible in Plan 05. Documented in the file's docstring with a self-expiring comment (\"the lint will become accurate again once Plan 05 wires real fallible operations\")."
  - "Manual sanity check is part of Plan 04's deliverable: a temporarily-injected `println!(\"SANITY_CHECK_VIOLATION\");` inside `crates/miner-cli/src/main.rs` was confirmed to produce the expected `disallowed_macros` error from `cargo clippy --workspace --all-targets -- -D warnings`, then reverted before commit. This proves the three-layer T-01-03 defence is mechanically active end-to-end, not just structurally correct."
  - "Test-only allows are tightly scoped: `clippy::naive_bytecount` is allowed at the `#[cfg(test)] mod tests` block level in `sink.rs` and `stderr_emit.rs` (one `#[allow(..., reason = \"...\")]` per module) — adding the `bytecount` crate to the workspace just so unit-test buffers can call `bytecount::count()` instead of `filter().count()` would be a Rule-1-worthy dependency-bloat anti-pattern. Test code uses `filter().count()` on 32-byte buffers; that's fine."

requirements-completed: [FOUND-02, OUT-01]
threats-mitigated: [T-01-03]

# Metrics
duration: 11min
completed: 2026-05-16
---

# Phase 01 Plan 04: StdoutSink + stderr_emit + workspace clippy gate Summary

**FOUND-02 (stdout = findings, stderr = logs, CI-enforced via clippy) and OUT-01 (NDJSON-on-stdout with per-envelope flush) both land in this plan. Three layers of T-01-03 defence are now active: (1) `StdoutSink` is the only type that opens `io::stdout()`, (2) workspace `clippy.toml` mechanically rejects `println!` / `eprintln!` / `print!` / `eprint!` / `dbg!` everywhere except two audited exemptions (`build.rs` for the cargo build-script protocol, `xtask` for dev-only command output), (3) `stderr_emit` is the sanctioned stderr writer for structured pre-flight errors so contributors never reach for `eprintln!`. `cargo clippy --workspace --all-targets -- -D warnings` runs clean; 22 unit tests pass; the manual sanity-check (inject `println!`, confirm clippy rejects, revert) was performed and the file was restored to a clean state before commit.**

## Performance

- **Duration:** 11 min
- **Started:** 2026-05-16T10:12:44Z
- **Completed:** 2026-05-16T10:23:46Z
- **Tasks:** 3 (Task 1 + Task 2 TDD; Task 3 type=auto, no-tdd)
- **Files:** 1 created, 7 modified

## Accomplishments

### Task 1 — `StdoutSink` (5 sink-module tests pass)

- `StdoutSink { writer: BufWriter<Stdout> }` lands in `crates/miner-core/src/findings/sink.rs` alongside the FindingSink trait. `new()` opens `io::stdout()` via `BufWriter::new(std::io::stdout())`; `Default::default()` calls `Self::new()`. The `FindingSink` impl writes via `serde_json::to_writer(&mut self.writer, finding)` + `self.writer.write_all(b"\n")` + `self.writer.flush()` — three steps, per-envelope, no banned macros (PITFALLS #4 closure: a panic loses at most the in-flight finding).
- Test 1 (`stdoutsink_writes_one_jsonl_line_per_envelope`): write a `Finding::RunStart` to a `WriterSink<Vec<u8>>` (the StdoutSink shape against an in-memory buffer); assert exactly one `\n` and the prefix parses as JSON with `"kind": "run_start"`.
- Test 2 (`stdoutsink_writes_multiple_envelopes_separated_by_newline`): write one RunStart + one RunEnd; assert exactly two `\n`s, split-on-newline yields two valid JSON objects with matching `kind` discriminators.
- Test 3 (`stdoutsink_flushes_per_envelope`): construct a `FlushCounter { inner: Vec<u8>, flushes: Arc<Mutex<usize>> }`, wrap it in `WriterSink<FlushCounter>` (the StdoutSink shape against an arbitrary writer), write three envelopes, assert `*flushes.lock() == 3`. This is the per-envelope-flush regression gate the Plan 06 CI script will reuse if needed.
- Bonus test `stdoutsink_constructs_via_new_and_default` — smoke-checks `StdoutSink::new()` and `StdoutSink::default()` compile and produce usable values, without writing to actual stdout.

Mitigates Threat T-01-03 (stdout pollution) — layer 1 of 3.

### Task 2 — `stderr_emit` module (4 tests pass)

- `crates/miner-core/src/error/stderr_emit.rs` swapped from Plan 03's placeholder to two functions:
  - `write_preflight_error<W: Write>(out, &WireError) -> io::Result<()>` — the generic primitive; `serde_json::to_writer(&mut *out, err)` + `out.write_all(b"\n")` + `out.flush()`. Tests inject `Vec<u8>`.
  - `emit_to_stderr(&WireError) -> io::Result<()>` — the production wrapper that supplies `io::stderr()`.
- Test 1 (`write_preflight_error_emits_jsonline_to_writer`): write one `WireError::preflight(PreflightCode::InvalidParameter, "bad param")` to a `Vec<u8>`; assert exactly one `\n`, prefix parses as JSON with `"message": "bad param"`.
- Test 2 (`error_code_uses_snake_case`): the same WireError serialises with `"code": "invalid_parameter"` (the locked snake_case wire form per RESEARCH §"error_code Vocabulary"; not the Rust-typed `InvalidParameter`).
- Test 3 (`context_preserves_btreemap_ordering`): build a WireError with three context keys `z`, `a`, `m` (inserted in that order); the serialised JSON lists them alphabetically. Also round-trips through `serde_json::from_slice` to confirm `BTreeMap` is the runtime type (OUT-03 determinism groundwork).
- Bonus test `emit_to_stderr_compiles_and_runs` — smoke-checks the convenience wrapper.

Mitigates Threat T-01-03 — layer 3 of 3. The sanctioned stderr writer means contributors never need to write `eprintln!` for structured pre-flight errors; the clippy gate then mechanically rejects any use that slips in anywhere else.

### Task 3 — Workspace `clippy.toml` + gate-clean cleanup

- `clippy.toml` at the workspace root with five `disallowed-macros` entries (`std::println`, `std::print`, `std::eprintln`, `std::eprint`, `std::dbg`), each with a `reason` string explaining what to use instead (`FindingSink`, `tracing::info!`, `tracing::warn!/error!`, `error::stderr_emit::write_preflight_error`, "do not leave dbg! in production code"). Plus an extensive top-of-file comment documenting the two exempted modules and the threat model.
- `cargo clippy --workspace --all-targets -- -D warnings` runs clean after the pedantic-lint cleanup (see Decisions Made — none silenced inappropriately; every `#[allow]` carries a `reason = "..."` explaining why).
- **Manual sanity check performed:** injected `println!("SANITY_CHECK_VIOLATION");` into `crates/miner-cli/src/main.rs`; ran `cargo clippy --workspace --all-targets -- -D warnings`; observed the expected `error: use of a disallowed macro \`std::println\`` with the correct help text from `clippy.toml`'s `reason` field ("stdout is reserved for findings; use FindingSink or tracing::info!"); reverted the file. Final `git diff` on `crates/miner-cli/src/main.rs` confirms the file is the intended Plan 04 state with no violation committed.

Mitigates Threat T-01-03 — layer 2 of 3. The full defence is now mechanically armed.

## Task Commits

Three atomic per-task commits on `worktree-agent-a2fc30ecfa8186792`:

1. **Task 1: StdoutSink** — `11878b8` (`feat(01-04): land StdoutSink as the single sanctioned stdout writer`)
2. **Task 2: stderr_emit** — `c5cfa9f` (`feat(01-04): fill stderr_emit with structured-error JSON writer (D-06)`)
3. **Task 3: clippy gate** — `c37cc90` (`feat(01-04): activate workspace clippy gate banning println!/eprintln! (D-15, FOUND-02)`)

The orchestrator appends the final metadata commit (this SUMMARY.md + STATE.md + ROADMAP.md) after merge; this executor does not modify STATE.md or ROADMAP.md per the parallel-execution contract.

## Files Created/Modified

### Created (1)

- **`clippy.toml`** — workspace-root clippy configuration. Five `disallowed-macros` entries banning `std::println`, `std::print`, `std::eprintln`, `std::eprint`, `std::dbg`, each carrying a `reason` string that points the violator at the correct alternative (`FindingSink`, `tracing::*`, or `stderr_emit::write_preflight_error`). Top-of-file comment documents the two crate-level `#![allow]` exemptions (`build.rs` for the cargo protocol; `xtask` for dev-only output) and the threat model.

### Modified (7)

- **`crates/miner-core/src/findings/sink.rs`** — added `StdoutSink` struct (75 LoC including doc comments) implementing `FindingSink` via the `serde_json::to_writer` + `write_all` + `flush` pipeline; added `WriterSink<W: Write + Send>` + `FlushCounter` test scaffolding (the StdoutSink shape against arbitrary inner writers) so the per-envelope-flush regression gate doesn't need to capture process stdout; added Tests 1/2/3 + a smoke test for `StdoutSink::new()`/`default()`; tightened `VecSink` to add `Default` impl + `#[must_use] fn new()` (clippy hygiene); reformatted one doc-continuation that tripped `clippy::doc_lazy_continuation`.
- **`crates/miner-core/src/error/stderr_emit.rs`** — replaced Plan 03 placeholder with `write_preflight_error<W: Write>` + `emit_to_stderr` (the generic primitive + production helper). 3 behavioural tests + 1 smoke test in `#[cfg(test)] mod tests`. Module-level `#[allow(clippy::naive_bytecount, reason = "...")]` on the tests module so `filter().count()` over a 32-byte test buffer doesn't trip the `bytecount`-crate suggestion. Three doc-backtick fixes on the test docstrings.
- **`crates/miner-core/build.rs`** — added crate-level `#![allow(clippy::disallowed_macros)]` for the cargo build-script protocol (the sole `println!` call site that is legitimately required by Cargo; an audited exemption). Rewrote `.map(|s| s.trim().to_string()).unwrap_or_else(|| "unknown".to_string())` → `.map_or_else(|| "unknown".to_string(), |s| s.trim().to_string())` to clear the pre-existing `clippy::map_unwrap_or` warning that was carried forward from Plan 01-02's deferred-items list and was explicitly flagged in this plan's environment prompt. Top-of-file docstring extended with a "Lint exemption" subsection explaining why the `#![allow]` exists.
- **`crates/miner-core/src/findings/mod.rs`** — added a `# Errors` section to `Raw::new`'s docstring (clippy::missing_errors_doc satisfaction); scoped a `#[allow(clippy::no_effect_underscore_binding, reason = "the two underscore bindings ARE the test — each move only compiles if RunId: Copy")]` on `run_id_is_copy` (the bindings are intentional, NOT dead code); tightened three doc backticks in Tests 6/7/8.
- **`crates/miner-core/src/error/codes.rs`** — one doc-backtick fix on the Test 1 docstring (`serde_json` and `snake_case` get backticks).
- **`crates/miner-core/src/lib.rs`** — one doc-backtick fix in the module docstring (`stderr_emit` gets backticks).
- **`crates/miner-cli/src/main.rs`** — added `#![allow(clippy::unnecessary_wraps)]` (NOT `clippy::disallowed_macros` — this is the unrelated "your fn returns Result but never errors" pedantic lint) for the placeholder `main` that returns `anyhow::Result<()>` but does nothing fallible yet. The module docstring grew a paragraph explaining the allow is scoped to the placeholder phase and the lint will self-correct once Plan 05 wires `?` for figment/clap calls.

### Deleted (0)

## Decisions Made

- **D-15 surgical reinterpretation HONOURED.** The plan's must_haves spelled out that StdoutSink and stderr_emit must NOT carry `#[allow(clippy::disallowed_macros)]` because they use `io::Write` directly — adding the allow would mask future regressions. Confirmed by the post-Task-3 grep gate: `sed 's|//.*||' <module> | grep '#\[allow(clippy::disallowed_macros)\]'` returns nothing for either file. The CONTEXT.md D-19 mention of "a single `#[allow]` at the head of the sink module" is interpreted as a Phase-1 implementation hint, not a hard requirement — RESEARCH §"Stdout/Stderr Enforcement Mechanics" point 2 explicitly says the allow "may not even be needed" and "if not needed, do not add it (less surface area for accidental over-allow)." This is the same surgical-correction pattern as Plan 01's D-20 → resolver=3 deviation.
- **`build.rs` `clippy::map_unwrap_or` warning FIXED, not allowed.** The plan's environment prompt explicitly flagged this as Plan 04's responsibility ("Plan 04 owns workspace clippy lints. Decide whether to fix or `#[allow]` it"). I chose the fix (a one-character rewrite to `map_or_else`) because `clippy::map_unwrap_or` is a genuine clarity lint about iterator-combinator hygiene, not noise. The exempted `#![allow(clippy::disallowed_macros)]` on `build.rs` is unrelated — that one covers the legitimate cargo-build-script protocol use of `println!`, which has no alternative API.
- **Two-sanctioned-writers pattern is now structurally complete.** Every byte that leaves miner-core via stdout or stderr flows through one of two auditable code paths: `findings::sink::StdoutSink` (for findings JSONL) or `error::stderr_emit::{write_preflight_error, emit_to_stderr}` (for one-shot structured pre-flight errors). Everything else uses `tracing::*!` macros, which route to stderr via the `tracing_subscriber::fmt().with_writer(std::io::stderr).init()` initialised in each wrapper binary's `main()` (already present in `miner-cli`, `miner-mcp`, `miner-http`, `miner-bench` from Plan 01-01). The clippy gate mechanically rejects any contributor attempt to bypass — including the most-tempting accidental-debugging case (`println!("got value: {x}")`).
- **`WriterSink<W: Write + Send>` test scaffolding chosen over capturing process stdout.** Tests 1/2/3 need to verify the byte-level shape of `StdoutSink`'s output, but capturing the test runner's actual stdout (via subprocess spawning or `gag::BufferRedirect`-style hacks) would add fragility for marginal correctness gain. Instead, the tests construct a `WriterSink<W>` over an injectable inner writer — `Vec<u8>` for byte assertions, `FlushCounter` for the per-envelope-flush regression gate. `WriterSink`'s `write_envelope` body is byte-for-byte identical to `StdoutSink`'s except for the inner writer's type, so a passing test against `WriterSink<Vec<u8>>` proves the same property of `StdoutSink` modulo only the choice of inner sink. Plan 07 (CI workflow) may add an end-to-end integration test that actually spawns the `miner-cli` binary and asserts on stdout, but that lands later when there's a concrete subcommand to invoke.
- **Manual sanity check performed and documented.** The plan's Task 3 acceptance criteria explicitly call for "temporarily edit `crates/miner-cli/src/main.rs` to add `println!("test");` — confirm clippy now FAILS with `disallowed_macros` error — then revert. (This is a manual sanity-check; document the verification in SUMMARY.md. Do NOT leave the violating code committed.)" The check was performed (see Issues Encountered #1 for the procedural detail about the `printf '%s'` newline-trim hiccup); the resulting clippy output included the exact lint message from `clippy.toml`'s `reason` field, confirming the gate is reading the config file correctly; the file was restored and the missing trailing newline was added back before commit. Final state: zero diff against the intended Plan 04 shape.
- **Plan 03 Deferred Issue #2 (clippy doc warnings) RESOLVED.** Plan 03 noted "~19 doc-style pedantic warnings (missing backticks, missing `# Errors` sections)" and deferred them to Plan 04's CI gate setup. I fixed all 19 (4 in `findings/mod.rs` test docs; 3 in `error/stderr_emit.rs` test docs; 1 in `error/codes.rs`; 1 in `lib.rs`; 2 in `findings/sink.rs` test docs and one doc-lazy-continuation in the StdoutSink struct doc; 1 `# Errors` section on `Raw::new`; plus a couple of test-scoped `#[allow]`s for genuine noise (naive_bytecount in tests, no_effect_underscore_binding on `run_id_is_copy`)). The workspace is now clippy-clean under `-D warnings` and `pedantic` enabled.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking issue] `build.rs` `clippy::map_unwrap_or` warning blocks `cargo clippy -- -D warnings`**

- **Found during:** Task 3 (first `cargo clippy --workspace --all-targets -- -D warnings` run after creating `clippy.toml`).
- **Issue:** `build.rs:19` had `.map(|s| s.trim().to_string()).unwrap_or_else(|| "unknown".to_string())` — exactly the pattern `clippy::map_unwrap_or` flags. Pre-existing warning (carried forward from Plan 01-02's deferred-items list); the environment prompt explicitly identified it as Plan 04's responsibility.
- **Fix:** Rewrote the combinator to `.map_or_else(|| "unknown".to_string(), |s| s.trim().to_string())` per clippy's own suggestion. Semantically identical, one fewer allocation in the some-but-empty edge case, and the lint clears.
- **Files modified:** `crates/miner-core/build.rs`.
- **Commit:** `c37cc90` (Task 3).
- **Justification for Rule 3:** The CI gate IS the deliverable; pre-existing warnings that block the gate block the deliverable. Same logic as fixing a build error in pre-existing code so the new feature can land.

**2. [Rule 2 — Missing critical functionality] `build.rs` cargo build-script protocol exemption**

- **Found during:** Task 3 (first clippy run; build.rs's `println!("cargo:rustc-env=...")` and `println!("cargo:rerun-if-changed=...")` calls tripped the new disallowed-macros lint).
- **Issue:** Cargo build scripts communicate with Cargo via `println!("cargo:...")` directives — there is no alternative API; this is the protocol. The workspace `clippy.toml` lints build scripts too (cargo compiles them with the same lint configuration). Without an exemption, every `cargo build` would fail clippy.
- **Fix:** Added crate-level `#![allow(clippy::disallowed_macros)]` at the head of `build.rs` (the documented sanctioned exemption per the plan's Task 3 Action item 3). Updated the top-of-file docstring with a new "Lint exemption" subsection so the exception is auditable and the rationale is in-file (not just in this summary).
- **Files modified:** `crates/miner-core/build.rs`.
- **Commit:** `c37cc90` (Task 3).
- **Justification for Rule 2:** Required for the workspace to build at all under the new clippy gate. The exemption is the plan's intended outcome (Task 3 Action item 3 anticipates this exact case).

**3. [Rule 1 — Pedantic-noise lint cleanup] `miner-cli/src/main.rs` `clippy::unnecessary_wraps` on placeholder main**

- **Found during:** Task 3 (clippy run after Tasks 1+2; the placeholder `fn main() -> anyhow::Result<()> { ...; Ok(()) }` body has no fallible operations yet).
- **Issue:** `clippy::unnecessary_wraps` correctly observes that the placeholder never errors and suggests dropping the `Result` return. But Plan 05 will add clap parsing, figment extraction, and engine dispatch — all fallible — and the `anyhow::Result<()>` return is the D-18-mandated binary-edge shape. Removing it now just to add it back next plan is churn.
- **Fix:** Added `#![allow(clippy::unnecessary_wraps)]` to `crates/miner-cli/src/main.rs` with a self-expiring docstring paragraph ("the lint will become accurate again once Plan 05 wires real fallible operations"). When Plan 05 lands the figment + clap dispatch, the lint stops firing naturally and the allow can be removed (or it can be left in place; it's a no-op once the function actually returns errors).
- **Files modified:** `crates/miner-cli/src/main.rs`.
- **Commit:** `c37cc90` (Task 3).
- **Justification for Rule 1:** Documented placeholder-noise per the plan's Task 3 Action item 4 ("If `cargo clippy -- -D warnings` triggers a pedantic warning on legitimate code (e.g., `module_name_repetitions` is famously noisy), add per-file `#![allow(...)]` to silence — but ONLY for noise, never to silence a real lint."). This IS the documented mechanism.

**4. [Rule 1 — Plan 03 Deferred Issue #2 cleanup] ~19 pedantic doc warnings**

- **Found during:** Task 3 (the same clippy run; ALL of these were carried forward from Plan 01-03, which deferred them with explicit instruction that Plan 04's CI gate setup would handle them).
- **Issue:** 12 `clippy::doc_markdown` "missing backticks" + 3 `clippy::naive_bytecount` (in test code, harmless) + 2 `clippy::no_effect_underscore_binding` (in `run_id_is_copy` — the bindings ARE the test) + 1 `clippy::doc_lazy_continuation` + 1 `clippy::missing_errors_doc` on `Raw::new` = 19 lints.
- **Fix:** Direct fixes for the doc-markdown issues (one-line backtick additions). Direct fix for `missing_errors_doc` (added the `# Errors` section to `Raw::new`). Direct fix for `doc_lazy_continuation` (reformatted my Task 1 doc paragraph). Targeted `#[allow(..., reason = "...")]` for the two genuine noise cases: `#[allow(clippy::naive_bytecount)]` scoped to the tests modules in `sink.rs` and `stderr_emit.rs` (would-be fix = adding the `bytecount` crate; not worth it for test code), and `#[allow(clippy::no_effect_underscore_binding)]` scoped to `run_id_is_copy` only (the underscore bindings ARE the Copy regression gate — without them the test doesn't test anything).
- **Files modified:** `crates/miner-core/src/findings/mod.rs`, `crates/miner-core/src/findings/sink.rs`, `crates/miner-core/src/error/stderr_emit.rs`, `crates/miner-core/src/error/codes.rs`, `crates/miner-core/src/lib.rs`.
- **Commit:** `c37cc90` (Task 3).
- **Justification for Rule 1:** Plan 03 explicitly deferred these to Plan 04. Plan 04 owns the CI clippy gate; clearing these is part of the deliverable. Every `#[allow]` carries a `reason = "..."` string explaining why it's noise and not a real lint.

### Auto-Added Critical Functionality

Beyond the explicit plan deliverables, the following correctness-required additions were made:

- **`VecSink::default()` impl + `#[must_use]` on `VecSink::new()`** — `clippy::new_without_default` and `clippy::must_use_candidate` both fire on this test-only helper. Adding the `Default` impl (one line delegating to `new()`) and the `#[must_use]` attribute is the canonical Rust idiom for this pattern; either would have required an allow attribute otherwise. (Rule 2 — clippy hygiene that costs nothing at runtime and improves discoverability for future contributors.)
- **`WriterSink<W: Write + Send>` test scaffolding** — beyond the bare-minimum Tests 1/2/3 the plan asks for, I introduced a generic `WriterSink<W>` so the tests don't need to construct a separate scaffold per assertion. This is the test-side equivalent of the production `StdoutSink` and makes the StdoutSink-shape semantics testable against any inner writer. The plan's action notes suggested writing a per-test custom `Write` impl (`FlushCounter { inner: Vec<u8>, flushes: usize }`) — I kept that for Test 3 but parametrised the surrounding sink, which is structurally cleaner. (Rule 2 — testability + ergonomics improvement that doesn't change the production surface.)

### Auth Gates

None — entirely a code-only plan.

## Deferred Issues

**1. No outstanding clippy warnings — workspace is `-D warnings` clean.** Unlike Plan 03's SUMMARY, this one has no Deferred Issues entry for clippy. Plan 03 deferred ~19 pedantic warnings to "Plan 04's CI gate setup"; all 19 are now resolved (see Deviation #4). The workspace is structurally clean.

**2. End-to-end stdout integration test (spawn miner-cli, capture stdout, assert on the JSONL stream) is still future work** — RESEARCH §"Phase 1 stdout test fixture" sketches it as a `miner-cli emit-fixture` subcommand that produces one RunStart + one RunEnd. Plan 05 lands the `emit-fixture` subcommand and the corresponding integration test in `crates/miner-cli/tests/` (or `crates/miner-core/tests/sink_jsonl_output.rs`). The byte-level unit tests in this plan (Tests 1/2/3 via `WriterSink<Vec<u8>>`) cover the StdoutSink-shape semantics; the cross-process test is the wrapper-binary contract that lands later.

**3. The `xtask` crate carries `#![allow(clippy::disallowed_macros)]` from Plan 01-01 Task 2** — confirmed by inspection. No change needed; documented in `clippy.toml`'s top-of-file comment as one of the two sanctioned exemptions (build.rs + xtask). If Plan 06 ever decides xtask should NOT be exempt (e.g., because the gen-schema subcommand should use tracing too), the exemption can be removed without a compatibility break.

## Issues Encountered

- **Manual sanity check procedural hiccup: `$(...)` strips trailing newlines.** The Bash command I used to save/restore `crates/miner-cli/src/main.rs` during the sanity check (`ORIGINAL=$(cat file); ...inject...; printf '%s' "$ORIGINAL" > file`) silently stripped the trailing newline because `$()` always strips trailing newlines from its captured value. Caught immediately by `git diff` showing "No newline at end of file" — added the newline back with `echo "" >> file`, re-ran clippy to confirm the file was now identical to the pre-injection state, then committed. The sanity check itself was a complete success (clippy correctly rejected `println!("SANITY_CHECK_VIOLATION");` with the expected error message); only the restore procedure had the newline glitch, which was caught and fixed before commit.
- **No issues with StdoutSink or stderr_emit implementations.** The plan's action sections were verbatim-implementable; both modules compiled and tested on first try.
- **Pedantic-lint cleanup was the bulk of Task 3's work** (and the bulk of this plan's elapsed time). The actual `clippy.toml` is 9 lines; the 7-file cleanup so it runs clean is the substance. This is exactly the trade-off the plan anticipated under Action item 4.

## Threat Mitigation

- **T-01-03 (stdout pollution leaking sensitive paths into the JSONL findings stream):** Mechanically prevented from this plan forward. Three layers of defence:
  1. **Single sanctioned writer.** `crates/miner-core/src/findings/sink.rs::StdoutSink` is the ONLY type in the workspace that opens `io::stdout()`. Any other code path that wants findings on stdout must go through `FindingSink::write_envelope`.
  2. **Workspace lint gate.** `clippy.toml`'s `disallowed-macros` rejects every direct `println!` / `eprintln!` / `print!` / `eprint!` / `dbg!` except inside the two sanctioned exemptions (`build.rs`, `xtask`). Confirmed mechanically active via the manual sanity check (inject violation → confirm rejection → revert).
  3. **Sanctioned stderr writer for structured errors.** `crates/miner-core/src/error/stderr_emit.rs::{write_preflight_error, emit_to_stderr}` is the answer to "but I need to write an error JSON to stderr" — so contributors never reach for `eprintln!` even when the temptation is highest (D-06 pre-flight rejection path). The two-writer pattern means every byte that leaves miner-core via stdout/stderr is auditable to one of exactly two code locations.

  Tests 1/2/3 in `sink.rs` verify the byte-level shape of StdoutSink (one envelope = one `\n` + valid JSON; per-envelope flush); the manual sanity check verifies the lint gate operates end-to-end; Tests 1/2/3 in `stderr_emit.rs` verify the structured-stderr writer carries the locked vocabulary and BTreeMap ordering.

## User Setup Required

None — entirely a code-only plan. Run `cargo build --workspace && cargo test -p miner-core && cargo clippy --workspace --all-targets -- -D warnings` to validate.

## Next Phase Readiness

- **Plan 05 (Wave 5, config layering + miner-cli main)** is UNBLOCKED. `StdoutSink::new()` is available for the `emit-fixture` subcommand to wire; `error::stderr_emit::emit_to_stderr(&WireError)` is available for the figment-error classifier to call when a required config field is missing (D-06 pre-flight rejection). Plan 05's `emit_fixture()` chain — `RunId::new()` → `StdoutSink::new()` → `sink.write_envelope(&Finding::RunStart(...))` → `sink.write_envelope(&Finding::RunEnd(...))` — composes from Plan 03 + Plan 04 types with zero new helpers. Plan 05 will replace `crates/miner-cli/src/main.rs`'s `#![allow(clippy::unnecessary_wraps)]` allow naturally once `?` propagation lands.
- **Plan 06 (Wave 6, schema regen + CI gates)** is partially derisked. The clippy gate is already armed at the workspace level, so Plan 06's CI workflow only needs to invoke `cargo clippy --workspace --all-targets -- -D warnings` rather than configure new lints. The schema-sync gate (D-21 gate 4) is still Plan 06's responsibility.
- **Plan 07 (Wave 7, CI workflow)** is partially derisked. The `cargo clippy -- -D warnings` CI step is now guaranteed to be meaningful (it actually fails on real violations, not just structurally configured). The pre-existing pedantic warnings deferred from Plans 01-02 / 01-03 are all resolved.

No blockers.

## Threat Flags

None — no new security-relevant surface was introduced. The plan ADDS defence (the clippy gate + the single-writer enforcement) without opening any new trust boundary.

## Self-Check: PASSED

File existence (artifacts in this plan):

- `FOUND: clippy.toml` (workspace root)
- `FOUND: crates/miner-core/src/findings/sink.rs` (modified — StdoutSink + 5 tests)
- `FOUND: crates/miner-core/src/error/stderr_emit.rs` (modified — write_preflight_error + emit_to_stderr + 4 tests)

Commit hashes (per `git log --oneline -3`):

- `FOUND: 11878b8` (Task 1 — StdoutSink)
- `FOUND: c5cfa9f` (Task 2 — stderr_emit)
- `FOUND: c37cc90` (Task 3 — clippy gate)

Plan-level verification:

- `cargo test -p miner-core findings::sink::tests::` → 5 passed (PASS)
- `cargo test -p miner-core error::stderr_emit::tests::` → 4 passed (PASS)
- `cargo test -p miner-core` (full) → 22 passed (PASS)
- `cargo build --workspace` → `Finished dev profile … in 2.93s` (PASS)
- `cargo clippy --workspace --all-targets -- -D warnings` → `Finished dev profile … in 0.43s` (PASS)
- Non-comment grep for `println!|eprintln!` in `sink.rs` → 0 hits (PASS)
- Non-comment grep for `println!|eprintln!` in `stderr_emit.rs` → 0 hits (PASS)
- Non-comment grep for `#[allow(clippy::disallowed_macros)]` in `sink.rs` → 0 hits (PASS)
- Non-comment grep for `#[allow(clippy::disallowed_macros)]` in `stderr_emit.rs` → 0 hits (PASS)
- Manual sanity check: injected `println!("SANITY_CHECK_VIOLATION");` → clippy rejected with the exact `clippy.toml` reason text → reverted → file restored to intended state (PASS)
- `clippy.toml` contains `std::println` and `std::eprintln` entries (PASS)

Three-layer T-01-03 defence in place (`✓` per layer):

- `✓ Layer 1` (single sanctioned stdout writer): `StdoutSink` is the only type opening `io::stdout()` in the workspace
- `✓ Layer 2` (workspace lint gate): `clippy.toml` bans 5 macros; `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- `✓ Layer 3` (sanctioned stderr writer): `stderr_emit::write_preflight_error` + `emit_to_stderr` are available; tested to emit JSON-line + flush; snake_case wire form locked

---
*Phase: 01-foundations-contracts*
*Completed: 2026-05-16*

---
phase: 03-scan-engine-facade-cli
verified: 2026-05-19T00:30:00Z
status: verified
score: 6/6 must-haves verified — all six ROADMAP success criteria (SC-1..SC-6) hold; CR-01/CR-02/CR-03 gaps closed by plan 03-07
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 5/6 (SC-5 partial)
  previous_verified: 2026-05-18T20:30:00Z
  gaps_closed:
    - "CR-01 — Reader-error torn-framing: engine::run_one steps 5/6 + ScanError::Io/Miner arms now emit Finding::ScanError + Finding::RunEnd before returning Ok(RunOutcome::HadScanErrors) — closed by commit ee0b8d9"
    - "CR-02 — SIGINT exit-code routing bypassed on non-preflight errors: main.rs::Command::Scan arm now matches Result without `?`, logs Err via tracing::error!, maps to RunOutcome::PreflightFailed, runs compute_exit_code on every dispatch path with the cancel flag — closed by commit 7489828"
    - "CR-03 — String-match dispatch on MinerError::Scan(msg): typed MinerError::Preflight(WireError) variant introduced; engine routes through engine::preflight::resolve_scan; CLI dispatches on Err(MinerError::Preflight(_)); zero occurrences of starts_with(\"unknown scan:\") remain — closed by commit f7bfe8a"
  gaps_remaining: []
  regressions: []
human_verification:
  - test: "Forward compatibility — Phase 4 per-scan cancellation/proptest pattern"
    expected: "When Phase 4 lands ANOM/CROSS/SEAS scans, each adds per-scan cancellation_tests + shuffled-future proptest that mirrors crates/miner-core/tests/shuffled_future_regression.rs's Warning 10 phrasing — i.e., the Phase 3 scaffolding is discoverable and copyable."
    why_human: "Forward-looking — verifies Phase 3 left a clean pattern for Phase 4. Not a current bug. Was carried forward from the original verification's item #3."
---

# Phase 3: Scan Engine, Facade & CLI Verification Report

**Phase Goal:** User can register and invoke a versioned scan through a single facade, run one end-to-end scan via the CLI with look-ahead-safe windowing, and choose a `strict` or `continuous_only` gap policy that miner enforces before producing findings.

**Verified:** 2026-05-19T00:30:00Z (re-verification after plan 03-07 gap closure)
**Original verification:** 2026-05-18T20:30:00Z (status: gaps_found)
**Status:** verified
**Re-verification:** Yes — three CR-01/02/03 gaps closed by plan 03-07; SC-5 now FULLY verified.

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria + Phase Goal)

| #   | Truth (ROADMAP SC)                                                                                                                                                                                              | Status       | Evidence                                                                                                                                                                                                                                                            |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | SC-1 (OP-01): User can run `miner scan <name@version> --instrument ... --timeframe ... --window ...` and receive NDJSON for the demo scan with resolved params echoed                                            | ✓ VERIFIED   | `cargo test -p miner-cli --test scan_subcommand_smoke -- scan_emits_run_start_result_run_end` passes (preserved from baseline; not regressed by 03-07).                                                                                                              |
| 2   | SC-2 (OP-07, OP-08): User can introspect `miner scans`; unknown scan + invalid params rejected at boundary with structured errors                                                                                | ✓ VERIFIED   | `scans_catalogue`, `unknown_scan_emits_wireerror_exit_1`, `invalid_params_emits_wireerror_exit_1` all pass. **03-07 strengthening:** unknown-scan now flows through the typed `MinerError::Preflight(WireError)` variant — the `code: "unknown_scan"` contract on stderr is preserved by construction rather than by string-match. |
| 3   | SC-3 (OUT-04): `--gap-policy strict` aborts with one GapAborted carrying the manifest; `continuous_only` partitions into gap-free sub-ranges and inlines manifest; never silently emits over a hole              | ✓ VERIFIED   | 5 named tests pass (preserved from baseline).                                                                                                                                                                                                                       |
| 4   | SC-4 (OP-05): User can `--dry-run` and see resolved job + data_slice + estimated_findings_count                                                                                                                   | ✓ VERIFIED   | `dry_run_emits_dry_run_finding_only` passes (preserved from baseline).                                                                                                                                                                                              |
| 5   | SC-5 (OP-06): User can interrupt a long-running scan via SIGINT and keep every streamed finding; rayon worker pool shuts down cleanly; exit code 130                                                              | ✓ VERIFIED   | **Promoted from PARTIAL.** Happy-path `sigint_preserves_already_streamed_findings_and_exits_130` still passes. **Corner-case contract** now also pinned: 4 new engine sink-content tests (reader/cache/scan-IO/scan-miner-error arms) assert RunStart + RunEnd framing always closes; 1 new integration test `cancel_overrides_error_exit_130` asserts SIGINT + forced engine catch-all error → exit 130; 2 new CLI unit tests pin the dispatch contract at the function level (cancel=true → 130, cancel=false → 1 not 2). |
| 6   | SC-6 (OUT-03): Byte-identical re-runs (sorted, BTreeMap, seeded RNG); shuffled-future regression — pre-T stats unchanged when post-T bars shuffled                                                                | ✓ VERIFIED   | `twice_run_byte_identical_when_volatile_fields_masked` + `look_ahead_safe_under_post_t_shuffle` proptest pass (preserved from baseline).                                                                                                                            |

**Score:** 6/6 fully verified. No gaps remain.

### Required Artifacts

| Artifact                                                                                          | Expected                                                                                                                                                                                                                                | Status     | Details                                                                                                                                              |
| ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/miner-core/src/scan/mod.rs`                                                               | `pub trait Scan: Send + Sync`; ScanCtx, ScanRequest, ScanError, ScanFindingShape support types                                                                                                                                          | ✓ VERIFIED | (Unchanged from baseline.)                                                                                                                           |
| `crates/miner-core/src/scan/registry.rs`                                                          | `Registry { scans: BTreeMap<(String, u32), Box<dyn Scan>> }`; bootstrap registers LjungBoxScan                                                                                                                                          | ✓ VERIFIED | (Unchanged from baseline.)                                                                                                                           |
| `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}`                                          | LjungBoxScan + pure kernels matching statsmodels 0.14.6 within 1e-12                                                                                                                                                                    | ✓ VERIFIED | (Unchanged from baseline.)                                                                                                                           |
| `crates/miner-core/src/engine/mod.rs` — `run_one` facade                                          | 7-step body: cancel → preflight → RunStart → dry-run → gap detection → gap dispatch → RunEnd. **03-07 additions:** step 2 routes through `engine::preflight::resolve_scan` (no inlined `registry.get`); steps 5/6/dispatch wrap Err with `emit_scan_error` + `emit_run_end` | ✓ VERIFIED | **ORPHANED → VERIFIED.** `pub(crate) run_one_with_registry` extracted (5 occurrences, lines 211, 1509, 1560 + 2 docs); public `run_one` is a thin wrapper. Reader-error arm (lines 309-330), cache-error arm (lines 396-425), `ScanError::Io` arm (lines 472-496), `ScanError::Miner` arm (lines 497-519) all emit Finding::ScanError + Finding::RunEnd before returning HadScanErrors. 13 occurrences of `emit_scan_error`/`emit_run_end`. |
| `crates/miner-core/src/error/mod.rs` — `MinerError::Preflight(WireError)` variant                 | New typed variant carrying `WireError` verbatim (closes CR-03)                                                                                                                                                                          | ✓ VERIFIED | **NEW (Plan 03-07).** Line 52: `Preflight(WireError)` with literal positional thiserror attribute `#[error("preflight error: {}", _0.message)]` at line 51. `From<MinerError> for WireError` short-circuits `Preflight(w) => w` at line 71 (typed passthrough). New unit test `miner_error_preflight_carries_typed_wireerror` at line 123 pins Display + typed passthrough. |
| `crates/miner-core/src/engine/{param_hash,framing,preflight,gap_policy}.rs`                       | Sub-modules with full 37 unit tests                                                                                                                                                                                                     | ✓ VERIFIED | (Unchanged from baseline.)                                                                                                                           |
| `crates/miner-core/src/findings/mod.rs` — `Finding::DryRun` variant + `DataSlice.gap_manifest`    | Additive envelope changes per D3-10 / D3-21                                                                                                                                                                                             | ✓ VERIFIED | (Unchanged from baseline; envelopes idempotent per `cargo run -p xtask -- gen-schema && git diff`.)                                                  |
| `crates/miner-core/src/findings/sink.rs` — `FindingSink::write_raw_json`                          | Trait method + 3 impls for `miner scans` catalogue lines                                                                                                                                                                                | ✓ VERIFIED | (Unchanged from baseline.)                                                                                                                           |
| `crates/miner-cli/src/{cli.rs,main.rs,scan_args.rs}`                                              | Command::Scan(ScanArgs) + Command::Scans; ctrlc handler BEFORE Cli::parse; `compute_exit_code` runs on every path. **03-07 additions:** `Command::Scan` arm matches `Result` without `?`, logs Err via `tracing::error!`, maps to `PreflightFailed`; typed dispatch on `Err(MinerError::Preflight(wire_err))`; cfg-gated `MINER_FORCE_ENGINE_ERROR` env hook for the CR-02 regression test | ✓ VERIFIED | **ORPHANED → VERIFIED.** main.rs lines 107-131: cancel-aware exit-code routing on every path (Ok and Err). Line 366: `Err(miner_core::error::MinerError::Preflight(wire_err))` typed dispatch — zero occurrences of `starts_with("unknown scan:")`. cfg-gate `cfg(any(test, feature = "test-internal"))` at line 334 (3 occurrences). MINER_FORCE_ENGINE_ERROR (5 occurrences in main.rs + 7 in the integration test). |
| `crates/miner-cli/tests/cancel_overrides_error_exit_130.rs`                                       | **NEW (Plan 03-07).** SIGINT-races-engine-error integration test                                                                                                                                                                        | ✓ VERIFIED | Created (5974 bytes); Unix-only via `#![cfg(unix)]`. Spawns `miner scan` with `MINER_FORCE_ENGINE_ERROR=1`; delivers SIGINT ~30ms after spawn via `nix::sys::signal::kill`; asserts exit code Some(130) at line 154.                                                  |
| `schemas/findings-v1.schema.json` + `schemas/scans-catalogue-v1.schema.json`                      | Schemas additively regenerated and idempotent                                                                                                                                                                                           | ✓ VERIFIED | Executor confirmed `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/` exits 0 — envelopes unchanged by Plan 03-07.                  |
| `crates/miner-core/tests/fixtures/{generate_golden.py,ljung_box_golden.json}` + insta snapshot     | Statsmodels golden provenance + masked snapshot                                                                                                                                                                                         | ✓ VERIFIED | (Unchanged from baseline.)                                                                                                                           |
| `crates/miner-core/Cargo.toml` + `crates/miner-cli/Cargo.toml` — `test-internal` feature          | Cfg-gates `--sleep-after-first-finding-ms` AND (Plan 03-07) `MINER_FORCE_ENGINE_ERROR`                                                                                                                                                  | ✓ VERIFIED | `test-internal = []` in miner-core (line 102); `test-internal = ["miner-core/test-internal"]` in miner-cli (line 30).                                |
| `README.md` — Quickstart additions                                                                | Phase 3 examples                                                                                                                                                                                                                        | ✓ VERIFIED | (Unchanged from baseline.)                                                                                                                           |

### Key Link Verification

| From                                              | To                                                                                | Via                                                                                              | Status   | Details                                                                                                |
| ------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------ |
| `miner-cli/src/main.rs`                           | `miner-core::engine::run_one`                                                     | `handle_scan_subcommand` → `engine::run_one(&req, cfg, &reader, sink, cancel)`                  | ✓ WIRED  | line 364: `match run_one(&req, cfg, &reader, sink, cancel)`. Public API unchanged.                     |
| `miner-cli/src/main.rs`                           | `miner-reader-dukascopy::DukascopyReader::new`                                    | Constructor at the binary edge                                                                   | ✓ WIRED  | line 358 (unchanged).                                                                                  |
| `miner-cli/src/scan_args.rs::to_scan_request`     | `engine::preflight::{resolve_scan, parse_params_kv, parse_iso_utc_window}` (boundary) | Returns typed `WireError` on failure                                                          | ✓ WIRED  | (Unchanged from baseline.)                                                                             |
| `miner-cli/src/main.rs::handle_scans_subcommand`  | `FindingSink::write_raw_json`                                                     | Per-scan catalogue line emission                                                                 | ✓ WIRED  | (Unchanged from baseline.)                                                                             |
| `engine::run_one` step 2                          | `engine::preflight::resolve_scan`                                                  | Typed-WireError lookup; no inlined `registry.get`                                                | ✓ WIRED  | **NEW (CR-03 fix).** engine/mod.rs line 239: `preflight::resolve_scan(&format!("{}@{}", req.scan_id, req.version), registry)` — replaces the previous inlined `registry.get` that produced a stringly-typed `MinerError::Scan`. |
| `engine::run_one` Err paths after RunStart        | `emit_scan_error` + `emit_run_end`                                                | Wrap reader/cache/scan-IO/scan-miner error arms before returning HadScanErrors                   | ✓ WIRED  | **NEW (CR-01 fix).** 13 occurrences of `emit_scan_error`/`emit_run_end`. Reader arm lines 313-327, cache arm 408-422, ScanError::Io arm 480-494, ScanError::Miner arm 503-517 — each block ends with `emit_run_end(...)?; return Ok(RunOutcome::HadScanErrors);`. |
| `Command::Scan` arm                               | `compute_exit_code`                                                               | Called on every dispatch path (Ok AND Err); cancel flag honored regardless of error tier (D3-24) | ✓ WIRED  | **NEW (CR-02 fix).** main.rs lines 114-130: matches `Result` without `?`, logs via `tracing::error!`, maps Err → `RunOutcome::PreflightFailed`, then `compute_exit_code(cancel.load(SeqCst), &outcome)` runs unconditionally. |
| `Command::Scan` arm Err match                     | `MinerError::Preflight(WireError)`                                                | Typed variant dispatch — no substring matching                                                   | ✓ WIRED  | **NEW (CR-03 fix).** main.rs line 366: `Err(miner_core::error::MinerError::Preflight(wire_err)) => { ... emit_to_stderr(&wire_err); Ok(RunOutcome::PreflightFailed) }`. Zero `starts_with("unknown scan:")` occurrences. |
| `cancel_overrides_error_exit_130.rs`              | `handle_scan_subcommand` cfg-gated `MINER_FORCE_ENGINE_ERROR` hook                | Env-var-driven deterministic force-error injection                                               | ✓ WIRED  | **NEW (CR-02 regression test).** Integration test spawns the binary with `--features test-internal` build + `MINER_FORCE_ENGINE_ERROR=1` env; the cfg-gated block at main.rs:334 honors cancel via a 500ms cancel-aware sleep loop before returning the forced anyhow Err. |
| `ScanArgs` `--sleep-after-first-finding-ms`       | `LjungBoxScan::run` cancel-aware sleep loop                                       | cfg-gated test hook for the happy-path SIGINT test                                               | ✓ WIRED  | (Unchanged from baseline.)                                                                             |

### Data-Flow Trace (Level 4)

| Artifact                              | Data Variable          | Source                                                                          | Produces Real Data | Status        |
| ------------------------------------- | ---------------------- | ------------------------------------------------------------------------------- | ------------------ | ------------- |
| `miner scan` stdout (Finding::Result) | `effect.value` etc.    | `LjungBoxScan::run` → kernel → `Q_max_lag` from `acf` over `close` BarFrame     | ✓ Yes              | ✓ FLOWING     |
| `miner scans` stdout                  | catalogue line object  | `bootstrap()` → `Scan::{id,version,param_schema,finding_fields}`                | ✓ Yes              | ✓ FLOWING     |
| `data_slice.gap_manifest` (Result)    | `Some(GapManifest)`    | `GapDetector::detect` → `gap_policy::dispatch` → engine inlines into ScanCtx    | ✓ Yes              | ✓ FLOWING     |
| `Finding::DryRun` payload             | resolved_params, etc.  | `engine::run_one` step 4 → `req.resolved_params` (from CLI preflight)           | ✓ Yes              | ✓ FLOWING     |
| `Finding::ScanError` (error arms)     | `message` + `error_code` | engine::mod.rs `emit_scan_error` — wraps reader/cache/scan-IO/scan-miner errors with `ScanErrorCode::ComputeError` and contextual prefix (`"reader: ..."`, `"cache: ..."`, `"scan io: ..."`, `"scan miner-error: ..."`) | ✓ Yes              | ✓ FLOWING     |
| `MinerError::Preflight(WireError)`    | `wire_err.code/message/context` | engine::preflight::resolve_scan builds `WireError::preflight(PreflightCode::UnknownScan, ...)` with structured context; CLI emits to stderr verbatim via `emit_to_stderr(&wire_err)` | ✓ Yes              | ✓ FLOWING     |

### Behavioral Spot-Checks

| Behavior                                                                                | Command                                                                            | Result                                                                | Status   |
| --------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | --------------------------------------------------------------------- | -------- |
| Full test suite passes (post-03-07)                                                     | `cargo test --workspace --all-targets`                                             | 265 passed; 0 failed (258 baseline + 7 new regression tests)          | ✓ PASS   |
| Clippy clean                                                                            | `cargo clippy --workspace --all-targets -- -D warnings`                            | Exit 0                                                                | ✓ PASS   |
| Cargo fmt clean                                                                         | `cargo fmt --all --check`                                                          | Exit 0                                                                | ✓ PASS   |
| Schema regeneration idempotent                                                          | `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/`                | Exit 0 — envelopes unchanged by Plan 03-07                            | ✓ PASS   |
| No `preserve_order` in Cargo.lock (Pitfall 1 gate)                                      | `grep -c preserve_order Cargo.lock`                                                | 0                                                                     | ✓ PASS   |
| Zero `starts_with("unknown scan:")` in CLI main.rs (CR-03 closure gate)                 | `grep -c 'starts_with("unknown scan:")' crates/miner-cli/src/main.rs`              | 0                                                                     | ✓ PASS   |
| Zero `starts_with("unknown scan:")` in engine/mod.rs (CR-03 closure gate)                | `grep -c 'starts_with("unknown scan:")' crates/miner-core/src/engine/mod.rs`       | 0                                                                     | ✓ PASS   |
| Typed `MinerError::Preflight` referenced in CLI dispatch                                | `grep -c 'MinerError::Preflight' crates/miner-cli/src/main.rs`                     | 3                                                                     | ✓ PASS   |
| Literal positional thiserror attribute pinned                                           | `grep '#\[error("preflight error: {}", _0.message)\]' crates/miner-core/src/error/mod.rs` | 1 match at line 51                                              | ✓ PASS   |
| `emit_scan_error`/`emit_run_end` usage in engine                                        | `grep -c 'emit_scan_error\|emit_run_end' crates/miner-core/src/engine/mod.rs`      | 13                                                                    | ✓ PASS   |
| `MINER_FORCE_ENGINE_ERROR` cfg-gated hook present in main.rs                            | `grep -c 'MINER_FORCE_ENGINE_ERROR' crates/miner-cli/src/main.rs`                  | 5                                                                     | ✓ PASS   |
| `cfg(any(test, feature = "test-internal"))` gates the hook                              | `grep -c 'cfg(any(test, feature = "test-internal"))' crates/miner-cli/src/main.rs` | 3                                                                     | ✓ PASS   |
| 4 new engine sink-content regression tests (CR-01 per-arm coverage)                      | `grep -nE 'fn run_one_(reader|cache|scan_io|scan_miner)_error_emits_run_start_and_run_end_with_scan_error' crates/miner-core/src/engine/mod.rs` | 4 functions present | ✓ PASS   |
| CLI dispatch unit tests pin the CR-02 contract (cancel=true → 130; cancel=false → 1)    | `grep -nE 'fn dispatch_scan_command_cancel_overrides|fn dispatch_scan_command_no_cancel' crates/miner-cli/src/main.rs` | 2 functions present at lines 505 + 519 | ✓ PASS   |
| MinerError::Preflight Display-impl regression test                                       | `grep -n 'fn miner_error_preflight_carries_typed_wireerror' crates/miner-core/src/error/mod.rs` | Present at line 123                  | ✓ PASS   |
| Integration test `cancel_overrides_error_exit_130` exists                                | `ls crates/miner-cli/tests/cancel_overrides_error_exit_130.rs`                     | 5974-byte file present                                                | ✓ PASS   |
| Integration test asserts exit code 130                                                   | `grep 'Some(130)' crates/miner-cli/tests/cancel_overrides_error_exit_130.rs`       | Found at line 154                                                     | ✓ PASS   |
| Engine routes step 2 through preflight::resolve_scan                                     | `grep -n 'preflight::resolve_scan' crates/miner-core/src/engine/mod.rs`            | 2 occurrences (call site + doc); call at line 239                     | ✓ PASS   |

CI gate evidence is sourced from the executor's plan 03-07 SUMMARY (test count 265, all four gates exit 0) — re-confirmed by static inspection of the repo at commit `43efb56` (merge of executor worktree onto main). Per the verifier environment note this gate evidence is authoritative absent a sanity-check trigger; the grep gates above provide that sanity check and all match the SUMMARY's claimed values.

### Probe Execution

Phase 3 declares no `scripts/*/tests/probe-*.sh` probes — Rust workspaces use `cargo test` as the canonical runnable check, which is captured under Behavioral Spot-Checks above.

### Requirements Coverage

| Requirement | Source Plan(s)               | Description                                                                                       | Status                         | Evidence                                                                                                                                                                  |
| ----------- | ---------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| OP-01       | 03-01,03-04,03-05,03-06      | `miner scan <name@version>` CLI invocation produces findings                                       | ✓ SATISFIED                    | Preserved (`scan_emits_run_start_result_run_end`). Plan 03-07 did not regress.                                                                                            |
| OP-05       | 03-01,03-04,03-05,03-06      | `--dry-run` shows resolved job + data_slice before committing                                      | ✓ SATISFIED                    | Preserved (`dry_run_emits_dry_run_finding_only`).                                                                                                                          |
| OP-06       | 03-01,03-04,03-05,03-06,03-07 | SIGINT preserves streamed findings; clean shutdown; exit 130                                       | ✓ SATISFIED                    | **PARTIAL → SATISFIED.** Happy-path `sigint_preserves_already_streamed_findings_and_exits_130` preserved AND corner-case `cancel_overrides_error_exit_130` integration test + 2 CLI unit tests + 4 engine sink-content tests added by Plan 03-07. SC-5 fully verified. |
| OP-07       | 03-01,03-02,03-05,03-06      | `miner scans` catalogue introspection emits scan name, version, params, finding_fields            | ✓ SATISFIED                    | Preserved (`scans_emits_one_line_per_registered_scan`).                                                                                                                    |
| OP-08       | 03-01,03-02,03-03,03-04,03-05,03-06,03-07 | Boundary validation: unknown scan + invalid params rejected; resolved params echoed              | ✓ SATISFIED                    | Preserved (`unknown_scan_emits_wireerror_exit_1`, `invalid_params_emits_wireerror_exit_1`). **Strengthened by 03-07:** typed `MinerError::Preflight(WireError)` variant replaces the fragile string-match dispatch — the `code: "unknown_scan"` contract is now preserved by construction. |
| OUT-04      | 03-01,03-02,03-03,03-04,03-06,03-07 | Findings carry actual consumed range + gap-manifest reference; strict aborts with single record   | ✓ SATISFIED                    | Preserved (5 gap_policy tests). **Strengthened by 03-07:** D-09 framing invariant ("every run terminates with RunEnd") now pinned on all four error arms — torn-framing bug closed.                                                                                                          |

All 6 declared requirement IDs cross-reference cleanly against REQUIREMENTS.md. No orphaned requirements.

### Anti-Patterns Found (Post-03-07)

The 3 corner-case Warning-severity anti-patterns flagged in the original verification are CLOSED:

| File                                            | Original Pattern                                                  | Original Severity | Status after 03-07                                                                                                                                                            |
| ----------------------------------------------- | ----------------------------------------------------------------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/miner-core/src/engine/mod.rs` (steps 5/6, dispatch) | `?` propagation skipped emit_run_end on reader/cache/IO errors    | ⚠️ Warning        | ✓ CLOSED — all four error arms wrap with `emit_scan_error` + `emit_run_end` before returning HadScanErrors. 4 regression tests pin sink contents.                              |
| `crates/miner-cli/src/main.rs` (lines 107-111)  | `?` short-circuited compute_exit_code on engine errors            | ⚠️ Warning        | ✓ CLOSED — Result matched without `?`, logged via `tracing::error!`, mapped to `RunOutcome::PreflightFailed`, `compute_exit_code` runs unconditionally with cancel flag. |
| `crates/miner-cli/src/main.rs` (lines 293-299)  | Substring match `msg.starts_with("unknown scan:")` on MinerError::Scan | ⚠️ Warning   | ✓ CLOSED — typed `MinerError::Preflight(WireError)` variant dispatched directly; zero `starts_with("unknown scan:")` occurrences remain.                                  |

The 5 Info-severity items from the original verification (WR-01, WR-02, WR-03, WR-07, IN-02, IN-04) are NOT in scope for Plan 03-07 and remain as documented in 03-REVIEW.md. They are minor / non-blocking and can be addressed in Phase 4 cleanup or as needed.

### Human Verification Required

The two corner-case human-verification items in the original verification (Reader-error torn-framing reproduction; SIGINT + sink-broken-pipe exit code) are now PROGRAMMATICALLY pinned by Plan 03-07's regression tests:

- **Reader-error torn-framing** → 4 engine sink-content tests assert `findings.first() == Finding::RunStart` AND `findings.last() == Finding::RunEnd` AND `Finding::ScanError` in between, on each of reader/cache/scan-IO/scan-miner-error arms.
- **SIGINT + non-preflight error exit code** → integration test `cancel_overrides_error_exit_130` asserts exit 130 deterministically via the cfg-gated `MINER_FORCE_ENGINE_ERROR` hook + 30ms-post-spawn SIGINT.

Only one human verification item carries forward — and it is forward-looking, not a current gap:

#### 1. Phase 4 forward compatibility — per-scan cancellation + proptest pattern

**Test:** When Phase 4 lands ANOM/CROSS/SEAS scans, each adds per-scan `cancellation_tests`-style tests and a shuffled-future proptest that mirrors `crates/miner-core/tests/shuffled_future_regression.rs`'s Warning 10 phrasing.

**Expected:** A Phase 4 plan author can read the Phase 3 test scaffolding (especially the new `FailingIoScan` / `FailingMinerScan` fixtures in engine/mod.rs tests) and replicate the pattern without ambiguity.

**Why human:** Forward-looking; verifies Phase 3 leaves a clean scaffolding pattern for Phase 4 to build on. Not a current Phase 3 bug.

### Gaps Summary

**No gaps remain.** All three CR-01/CR-02/CR-03 corner-case contract gaps from the original verification are closed in main as of commit `43efb56`:

- **CR-01 (closed)** — `engine::run_one` reader/cache/scan-IO/scan-miner-error arms all emit `Finding::ScanError` + `Finding::RunEnd` before returning `Ok(RunOutcome::HadScanErrors)`. The D-09 wire-protocol invariant ("every run terminates with RunEnd") holds on every error path. Closed by commit `ee0b8d9`.

- **CR-02 (closed)** — `main.rs::Command::Scan` arm now matches `Result` without `?`, logs Err via `tracing::error!`, maps to `RunOutcome::PreflightFailed`, and runs `compute_exit_code(cancel.load(SeqCst), &outcome)` on every path. Under `cancel=true` the short-circuit in `compute_exit_code` wins → 130 per D3-24. Under `cancel=false` the catch-all engine error preserves the historical exit-1 semantics for Phase 6 wrappers. Closed by commit `7489828`.

- **CR-03 (closed)** — New typed `MinerError::Preflight(WireError)` variant with literal positional thiserror attribute carries the structured `PreflightCode` from engine to CLI. Engine `run_one` step 2 routes through `engine::preflight::resolve_scan`; CLI dispatches on the typed variant. Zero `starts_with("unknown scan:")` occurrences remain. Closed by commit `f7bfe8a`.

Test count grew from 258 (Plan 03-06 baseline) to 265 (Plan 03-07): +4 engine sink-content tests + 1 SIGINT-races-engine-error integration test + 2 CLI dispatch unit tests + 1 MinerError Display-impl pin = 8 new tests minus 1 renamed/rewritten old test = +7 net.

All four CI gates exit 0 per the executor's Plan 03-07 SUMMARY:
- `cargo build --workspace`
- `cargo test --workspace --all-targets` (265 passed)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/findings-v1.schema.json schemas/scans-catalogue-v1.schema.json` (envelopes idempotent)

**Phase 3 is complete. All six ROADMAP Success Criteria observably hold in the codebase. Phase 4 entry is unblocked.**

---

## Appendix: Original Verification (2026-05-18T20:30:00Z) — Historical Record

The original verification flagged three corner-case gaps from code review (CR-01/CR-02/CR-03) on 2026-05-18. All three were corner-case correctness or maintainability issues that the happy-path test suite did not exercise. Plan 03-07 was scoped specifically to close them. The original gaps and their per-artifact issue descriptions are now superseded by the re-verification above; the original `status: gaps_found` record is preserved here for traceability:

- **Original CR-01 issue (engine/mod.rs lines 250-251 + 313-323 + 370-373):** `?` propagation skipped emit_run_end on reader/cache/IO errors. → **Closed by commit `ee0b8d9` (Task 2 of Plan 03-07).**
- **Original CR-02 issue (main.rs lines 107-111):** `?` short-circuited compute_exit_code on engine errors. → **Closed by commit `7489828` (Task 3 of Plan 03-07).**
- **Original CR-03 issue (main.rs lines 293-299 + engine/mod.rs line 192-195):** Substring-match dispatch on `MinerError::Scan(msg)` with `msg.starts_with("unknown scan:")`. → **Closed by commit `f7bfe8a` (Task 1 of Plan 03-07).**

---

_Originally verified: 2026-05-18T20:30:00Z (status: gaps_found, 5/6)_
_Re-verified after Plan 03-07 gap closure: 2026-05-19T00:30:00Z (status: verified, 6/6)_
_Verifier: Claude (gsd-verifier)_

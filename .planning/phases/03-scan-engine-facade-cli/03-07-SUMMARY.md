---
phase: 03-scan-engine-facade-cli
plan: 07
subsystem: api
tags: [rust, thiserror, anyhow, tracing, ctrlc, nix, sigint, exit-codes, jsonl, framing-invariant]

# Dependency graph
requires:
  - phase: 03-scan-engine-facade-cli
    provides: "Plan 03-06 SIGINT integration test, SyntheticCache fixture, MinerError baseline, engine::preflight::resolve_scan, ScanCtx cfg-gated test hook, emit_scan_error/emit_run_end helpers"
provides:
  - "Typed MinerError::Preflight(WireError) variant — closes CR-03 fragile string-match dispatch"
  - "Engine error paths wrap with emit_scan_error + emit_run_end before returning HadScanErrors — closes CR-01 orphaned-RunStart bug"
  - "main.rs Command::Scan arm honors cancel flag on Err arm via PreflightFailed mapping — closes CR-02"
  - "cfg-gated MINER_FORCE_ENGINE_ERROR env hook with cancel-aware sleep loop — deterministic CR-02 regression test"
  - "Internal run_one_with_registry seam for fixture injection — enables FailingIoScan / FailingMinerScan engine tests"
  - "4 engine sink-content regression tests + 1 SIGINT-races-engine-error integration test + 2 CLI dispatch unit tests + 1 MinerError Display-impl pin"
affects: [04-scans, 06-mcp-http]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Typed-variant dispatch over format-string parsing at trust boundaries — engine emits typed PreflightCode, CLI matches the variant"
    - "Cancel-aware-sleep pattern in cfg-gated test hooks — gives SIGINT a window to win without subprocess flakiness"
    - "Public-thin-wrapper + pub(crate) internal seam for registry injection — preserves API while enabling fixture tests"

key-files:
  created:
    - "crates/miner-cli/tests/cancel_overrides_error_exit_130.rs — SIGINT-races-engine-error integration regression"
  modified:
    - "crates/miner-core/src/error/mod.rs — added MinerError::Preflight(WireError) + typed-passthrough From impl + Display-impl regression test"
    - "crates/miner-core/src/engine/mod.rs — pub(crate) run_one_with_registry extracted; step 5/6/dispatch error arms wrap with emit_scan_error + emit_run_end; 4 new sink-content tests"
    - "crates/miner-cli/src/main.rs — Command::Scan arm matches Result, logs Err, maps to PreflightFailed; cfg-gated MINER_FORCE_ENGINE_ERROR hook with cancel-aware sleep; 2 new CLI unit tests + helper"

key-decisions:
  - "Catch-all Err mapping is PreflightFailed (NOT HadScanErrors) to preserve exit-1 semantics for Phase 6 wrappers per D3-24"
  - "MinerError::Preflight uses literal positional thiserror form `#[error(\"preflight error: {}\", _0.message)]` because WireError lacks Display"
  - "cfg-gated MINER_FORCE_ENGINE_ERROR hook sleeps cancel-aware (poll every 5ms for up to 500ms) so SIGINT races deterministically"
  - "run_one stays public; new pub(crate) run_one_with_registry is the registry-injection seam — keeps the public API unchanged"
  - "All wrapped error paths reuse ScanErrorCode::ComputeError; per-arm context lives in the `message` field (reader/cache/scan io/scan miner-error prefixes)"

patterns-established:
  - "Pattern: typed-variant dispatch at engine/CLI boundary — never match on format strings"
  - "Pattern: every Err path after RunStart MUST emit ScanError + RunEnd before returning HadScanErrors (D-09 framing invariant)"
  - "Pattern: catch-all CLI Err under cancel=true → 130 via PreflightFailed mapping + compute_exit_code short-circuit (D3-24)"
  - "Pattern: cfg-gated test hooks under `cfg(any(test, feature = \"test-internal\"))` mirror the --sleep-after-first-finding-ms convention"

requirements-completed: [OP-06, OP-08, OUT-04]

# Metrics
duration: ~75min
completed: 2026-05-18
---

# Phase 3 Plan 07: Gap closure (CR-01, CR-02, CR-03) Summary

**Typed MinerError::Preflight variant + reader/cache/scan-IO/scan-miner-error wrapping + cancel-honoring exit-code routing — closes all three Phase 3 code-review gaps and restores the D-09 framing invariant + D3-24 cancel-overrides-everything contract.**

## Performance

- **Duration:** ~75 min
- **Started:** 2026-05-18T22:08:00Z
- **Completed:** 2026-05-18T23:23:20Z
- **Tasks:** 3
- **Files modified:** 3 (1 created, 2 modified, 1 modified)

## Accomplishments

- **CR-01 closed:** `engine::run_one` no longer leaves an orphaned `RunStart` envelope on stdout. Reader, cache, scan-IO, and scan-miner-error error paths all emit `Finding::ScanError` + `Finding::RunEnd` before returning `Ok(RunOutcome::HadScanErrors)`. Four regression tests pin sink contents per arm.
- **CR-02 closed:** SIGINT mid-run now yields exit 130 regardless of which non-preflight error tier the engine reached. The `Command::Scan` arm matches `Result` (no `?` short-circuit), logs the Err via `tracing::error!`, maps to `RunOutcome::PreflightFailed` (preserves exit-1 semantics for Phase 6 wrappers — Warning 4 fix), then calls `compute_exit_code`. Under cancel=true the short-circuit at `compute_exit_code:315` wins → 130.
- **CR-03 closed:** Eliminated the fragile `MinerError::Scan(_)` + `starts_with("unknown scan:")` CLI substring-match dispatch. Engine `run_one` step 2 routes through `engine::preflight::resolve_scan` (typed `WireError(PreflightCode::UnknownScan)`); CLI dispatches on the new typed `MinerError::Preflight(WireError)` variant. The OP-08 SC-2 contract (`code: "unknown_scan"` on stderr, exit 1) is preserved by construction.
- **Determinism unlocked:** New cfg-gated `MINER_FORCE_ENGINE_ERROR` env hook inside `handle_scan_subcommand` makes the CR-02 catch-all-Err code path reachable from the regression test without depending on production reader/cache error paths. The hook does a cancel-aware sleep (poll every 5ms for up to 500ms) so SIGINT wins the race deterministically.
- **Test count:** 258 baseline → 265 tests (4 engine sink-content + 1 SIGINT-races-engine-error integration + 2 CLI dispatch unit tests; the existing `run_one_reader_error_wraps_via_miner_error_scan` was renamed and rewritten as the new sink-content test, so the net is +7 minus 0 = +7).
- **All four CI gates green:** `cargo test --workspace --all-targets` (265 passed), `cargo clippy --workspace --all-targets -- -D warnings` (0), `cargo fmt --all --check` (0), and `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/...` (0 — envelopes unchanged).

## Task Commits

Each task was committed atomically:

1. **Task 1: Add typed MinerError::Preflight + route engine via resolve_scan (CR-03)** — `f7bfe8a` (feat)
2. **Task 2: Wrap engine error paths to preserve RunStart/RunEnd framing (CR-01)** — `ee0b8d9` (fix)
3. **Task 3: Honor cancel flag on Err arm + MINER_FORCE_ENGINE_ERROR hook (CR-02)** — `7489828` (fix)

## Files Created/Modified

- **created** `crates/miner-cli/tests/cancel_overrides_error_exit_130.rs` — SIGINT-races-engine-error integration test (Unix-only, `#![cfg(unix)]`); rebuilds with `--features test-internal`, spawns the binary with `MINER_FORCE_ENGINE_ERROR=1`, delivers SIGINT ~30ms after spawn, asserts exit 130.
- **modified** `crates/miner-core/src/error/mod.rs` — added `MinerError::Preflight(WireError)` variant with literal positional thiserror attribute; `From<MinerError> for WireError` passes the typed `WireError` through verbatim; new `miner_error_preflight_carries_typed_wireerror` unit test pinning both Display impl (Warning 5) and typed passthrough (Info 6).
- **modified** `crates/miner-core/src/engine/mod.rs` — extracted `pub(crate) run_one_with_registry` (registry-injection seam); `run_one` is now a thin wrapper; step 2 routes through `engine::preflight::resolve_scan`; steps 5, 6 (cache load), and the `ScanError::Io` / `ScanError::Miner` arms all emit `Finding::ScanError` + `Finding::RunEnd` before returning `Ok(HadScanErrors)`; `scan_id_at_version` hoisted above step 5 so the new error blocks can build the envelope; 4 new regression tests (`run_one_reader_error_emits_run_start_and_run_end_with_scan_error`, `run_one_cache_error_emits_run_start_and_run_end_with_scan_error`, `run_one_scan_io_error_emits_run_start_and_run_end_with_scan_error`, `run_one_scan_miner_error_emits_run_start_and_run_end_with_scan_error`) + `FailingIoScan` / `FailingMinerScan` fixtures using explicit `ScanFindingShape` struct literals (Warning 2).
- **modified** `crates/miner-cli/src/main.rs` — `Command::Scan` arm now matches `Result` without `?`, logs Err via `tracing::error!`, maps to `RunOutcome::PreflightFailed` (Warning 4 — preserves exit-1 semantics); typed dispatch on `Err(MinerError::Preflight(wire_err))` instead of substring match; new cfg-gated `MINER_FORCE_ENGINE_ERROR` env hook in `handle_scan_subcommand` with a cancel-aware sleep loop; new `dispatch_scan_command_for_test` helper + 2 unit tests (`dispatch_scan_command_cancel_overrides_anyhow_err_returns_130`, `dispatch_scan_command_no_cancel_anyhow_err_returns_1_not_2`).

## Decisions Made

- **PreflightFailed mapping (NOT HadScanErrors) for the catch-all Err arm.** Per Warning 4: the historical "exit 1 on engine non-preflight error" semantics are preserved per D3-24 so Phase 6 MCP/HTTP wrappers see no behaviour change for catch-all engine errors. Under cancel=true, `compute_exit_code`'s short-circuit still wins → 130.
- **Literal positional thiserror form for `MinerError::Preflight`.** `#[error("preflight error: {}", _0.message)]` — the `{0}` short-form would not compile because `WireError` does NOT implement `Display`. The unit test pins this so a future refactor cannot silently break the Display impl.
- **Cancel-aware sleep loop inside the `MINER_FORCE_ENGINE_ERROR` hook.** Without a window for SIGINT to land, the Err+map+`compute_exit_code` path is microsecond-fast and the integration test would race-fail (observed exit 1 on first run). The 500ms cancel-poll loop mirrors the existing `--sleep-after-first-finding-ms` cancel-aware pattern and lets the test's 30ms-post-spawn SIGINT win deterministically. This is the only deviation from the plan's prose — see "Deviations from Plan" below.
- **Public `run_one` stays unchanged; new `pub(crate) run_one_with_registry` is the test seam.** Binary callers (CLI / future MCP / HTTP wrappers) continue to call `run_one`; only the engine tests reach into `run_one_with_registry` to inject `FailingIoScan` / `FailingMinerScan` fixtures.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking] Added cancel-aware sleep loop inside the MINER_FORCE_ENGINE_ERROR hook**

- **Found during:** Task 3 (cancel_overrides_error_exit_130 integration test first run)
- **Issue:** The plan's prose at Task 3 Step 2 step 6 assumed the child "should be in compute_exit_code" by the time SIGINT lands (30ms after spawn). In practice the catch-all Err path (handle_scan_subcommand returns Err → main() logs + maps → compute_exit_code runs → process::exit) completes in microseconds once the env-var check fires, so the child exited with code 1 before SIGINT arrived. The test FAILED with `got Some(1); right Some(130)`.
- **Fix:** Extended the cfg-gated `MINER_FORCE_ENGINE_ERROR` block in `handle_scan_subcommand` with a cancel-aware sleep loop (poll the cancel flag every 5ms for up to 500ms) BEFORE returning `Err(anyhow::Error)`. The loop mirrors the existing `--sleep-after-first-finding-ms` cancel-aware sleep pattern. This gives SIGINT a 500ms window to flip the cancel flag in the child; the loop yields early when cancel becomes true (still returning Err — the test's contract is still "SIGINT races a forced engine error").
- **Files modified:** `crates/miner-cli/src/main.rs` (the cfg-gated block inside `handle_scan_subcommand`).
- **Verification:** `cargo test -p miner-cli --test cancel_overrides_error_exit_130` passes (exit code Some(130)). The 500ms upper bound is well within cargo's per-test timeout AND well beyond the test's 30ms pre-SIGINT delay, so the race is deterministic in CI.
- **Committed in:** `7489828` (Task 3 commit).

---

**Total deviations:** 1 auto-fixed (Rule 3 — Blocking; deterministic-test enablement). The deviation was a required test-mechanism fix; the production semantic contract is unchanged.

**Impact on plan:** Minor — the cfg-gated test hook is still ABSENT from release builds, and the cancel-aware sleep is only reachable via `MINER_FORCE_ENGINE_ERROR=1` which is itself cfg-gated. The 500ms sleep is bounded and only fires for the regression test. No scope creep.

## Issues Encountered

- **Clippy `doc_markdown` lint** caught `Command::Scan`, `PreflightFailed`, and `HadScanErrors` references in new doc-comments — backticked these inline.
- **Clippy `doc_lazy_continuation` lint** caught one wrap in the `MinerError::Preflight` doc-comment — restructured the prose.
- **Grep-gate `unknown scan:` / `starts_with("unknown scan:")` patterns** in doc-comments — reworded the comments so the production sources contain zero matches of the deprecated patterns (the literal patterns now live only in test-message strings; tests still pin the contract).
- **`cargo fmt` reformatted multi-line constructors** in the new test fixtures — applied automatically; no behavioural impact.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- **Phase 3 verifier can be re-run** (`gsd-verifier 03`) — all three CR-01/02/03 gaps closed, ready to promote `03-VERIFICATION.md` from `status: gaps_found` to `status: verified`.
- **All six ROADMAP Phase 3 success criteria (SC-1..SC-6)** remain observable; SC-5 (OP-06) is now FULLY verified for both happy path AND corner cases (SIGINT + reader/cache/IO error → exit 130 AND clean RunEnd framing).
- **Phase 4 entry unblocked** — no envelope schema changes, no API churn beyond the new typed `MinerError::Preflight` variant, no new external dependencies.

## Verification Output

### Final test counts
```
cargo test --workspace --all-targets
Total passed: 265, failed: 0
```

### New regression tests (all passing)
- `error::tests::miner_error_preflight_carries_typed_wireerror` — Display impl + typed passthrough (Warning 5 + Info 6)
- `engine::tests::run_one_reader_error_emits_run_start_and_run_end_with_scan_error` — CR-01 reader arm
- `engine::tests::run_one_cache_error_emits_run_start_and_run_end_with_scan_error` — CR-01 cache arm
- `engine::tests::run_one_scan_io_error_emits_run_start_and_run_end_with_scan_error` — CR-01 scan-IO arm
- `engine::tests::run_one_scan_miner_error_emits_run_start_and_run_end_with_scan_error` — CR-01 scan-miner-error arm
- `tests::dispatch_scan_command_cancel_overrides_anyhow_err_returns_130` — CR-02 function-level pin
- `tests::dispatch_scan_command_no_cancel_anyhow_err_returns_1_not_2` — Warning 4 pin (exit 1, not 2)
- `cancel_overrides_error_exit_130::cancel_overrides_error_exit_130` — CR-02 integration regression

### Preserved contract tests
- `scan_subcommand_smoke::unknown_scan_emits_wireerror_exit_1` — OP-08 SC-2 contract preserved (`code: "unknown_scan"` on stderr, exit 1)
- `sigint_preserves_stream::sigint_preserves_already_streamed_findings_and_exits_130` — OP-06 happy-path SIGINT contract preserved
- `engine::tests::run_one_preflight_unknown_scan` — updated assertions to match the new typed variant; envelope discipline (empty sink on preflight failure) preserved
- All 169 miner-core lib tests pass

### Grep gates (Plan 03-07 acceptance criteria)

| Gate | Required | Actual |
|------|----------|--------|
| `starts_with("unknown scan:")` in `crates/miner-cli/src/main.rs` | 0 | 0 |
| `unknown scan:` format string in `crates/miner-core/src/engine/mod.rs` | 0 | 0 |
| `MinerError::Preflight` in `crates/miner-cli/src/main.rs` | ≥ 1 | 3 |
| `tracing::error!` in `crates/miner-cli/src/main.rs` | ≥ 1 | 4 |
| `emit_scan_error` / `emit_run_end` in `crates/miner-core/src/engine/mod.rs` | ≥ 8 | 13 |
| Literal thiserror attribute `#[error("preflight error: {}", _0.message)]` | 1 | 1 |
| `MINER_FORCE_ENGINE_ERROR` in `crates/miner-cli/src/main.rs` | ≥ 1 | 5 |
| `MINER_FORCE_ENGINE_ERROR` in integration test | ≥ 1 | 7 |
| `cfg(any(test, feature = "test-internal"))` in `crates/miner-cli/src/main.rs` | ≥ 1 | 3 |
| `ScanFindingShape::default()` in `crates/miner-core/src/engine/mod.rs` (Warning 2) | 0 | 0 |
| `tokio`/`async-std`/`smol` in `cargo tree -p miner-core -e normal` (FOUND-04) | 0 | 0 |
| `preserve_order` in `Cargo.lock` (Pitfall 1) | 0 | 0 |
| `RunOutcome::PreflightFailed` in `crates/miner-cli/src/main.rs` | ≥ 2 | 10 |
| `preflight::resolve_scan` in `crates/miner-core/src/engine/mod.rs` | ≥ 1 | 2 |
| `run_one_with_registry` in `crates/miner-core/src/engine/mod.rs` | ≥ 2 | 5 |

All gates pass.

### CI gates

```
cargo build --workspace                                        — exit 0
cargo test  --workspace --all-targets                          — exit 0 (265 passed)
cargo clippy --workspace --all-targets -- -D warnings          — exit 0
cargo fmt --all --check                                        — exit 0
cargo run -p xtask -- gen-schema && git diff --exit-code schemas/findings-v1.schema.json schemas/scans-catalogue-v1.schema.json
                                                               — exit 0 (envelopes idempotent)
```

## Follow-up

- `gsd-verifier 03` should be re-run by the orchestrator after merge to promote `03-VERIFICATION.md` from `status: gaps_found` to `status: verified` — this is an orchestrator step, not part of Plan 03-07's execution.
- No envelope schema changes — `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/` exits 0 (idempotent).

## Self-Check: PASSED

- All three task commits exist in git log: `f7bfe8a`, `ee0b8d9`, `7489828`.
- New integration test file `crates/miner-cli/tests/cancel_overrides_error_exit_130.rs` exists and is committed.
- All grep gates pass (see table above).
- All four CI gates exit 0.

---
*Phase: 03-scan-engine-facade-cli*
*Completed: 2026-05-18*

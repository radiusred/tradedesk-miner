---
phase: 03-scan-engine-facade-cli
verified: 2026-05-18T20:30:00Z
status: gaps_found
score: 6/6 must-haves verified (happy-path); 2 corner-case contract violations surfaced from code review
overrides_applied: 0
gaps:
  - truth: "SC-5 / D-09: SIGINT-or-error path emits clean RunEnd framing (every run terminates with RunEnd)"
    status: partial
    reason: "Reader-error and cache-error paths in engine::run_one emit RunStart then propagate MinerError::Scan via `?` WITHOUT emitting RunEnd. Consumers see an orphaned RunStart envelope — a wire-protocol contract violation (D-09, framing.rs:1-7). The unit test `run_one_reader_error_wraps_via_miner_error_scan` asserts the error variant but does NOT check sink contents, so torn framing is silently accepted."
    artifacts:
      - path: "crates/miner-core/src/engine/mod.rs"
        issue: "Line 208 emits RunStart; lines 250-251 (gap detection) and 322-323 (cache.get_or_build) propagate errors via `?` skipping the step-7 emit_run_end at line 383. Same applies to ScanError::Io / ScanError::Miner arms at lines 370-373."
    missing:
      - "Wrap the gap-detection call (line 250) and the cache.get_or_build call (line 313) such that on Err, the engine emits a Finding::ScanError + RunEnd before returning. Equivalent treatment for the ScanError::Io / ScanError::Miner match arms at lines 370-373."
      - "Add a regression test that asserts sink.0 contains both RunStart AND RunEnd when GapDetector::detect or cache.get_or_build is forced to fail (sister to run_one_reader_error_wraps_via_miner_error_scan)."
  - truth: "SC-5 / D3-24: SIGINT (cancel flag) overrides everything — `cancelled? → 130` regardless of error path"
    status: partial
    reason: "main.rs:107-111 calls compute_exit_code only on the Ok arm of handle_scan_subcommand. On any MinerError that isn't the literal-string-matched `unknown scan:` preflight case, the `?` at line 108 returns Err and anyhow's Termination prints + exits 1, ignoring the cancel flag. CONTEXT D3-24 mandates cancelled → 130 regardless of tier. SIGINT + sink-broken-pipe / reader-IO-error → exit 1 instead of 130."
    artifacts:
      - path: "crates/miner-cli/src/main.rs"
        issue: "Line 108: `let outcome = handle_scan_subcommand(...)?;` — the `?` short-circuits the compute_exit_code call at line 109."
      - path: "crates/miner-cli/src/main.rs"
        issue: "Lines 291-301 (handle_scan_subcommand) uses substring match `msg.starts_with(\"unknown scan:\")` on MinerError::Scan to detect preflight failures. All other engine errors propagate via anyhow, bypassing the cancel-flag check entirely."
    missing:
      - "Restructure main.rs:107-111 so compute_exit_code runs on every dispatch path — match the Result<RunOutcome>, mapping Err to a RunOutcome (e.g., HadScanErrors or a new Errored variant), then call compute_exit_code with the cancel flag."
      - "Add an integration test that flips cancel after spawning a child against a forced reader error, asserts exit 130."
  - truth: "OP-08: string-match dispatch on MinerError::Scan is fragile (maintainability — not a current behavioral bug, but a regression risk)"
    status: failed
    reason: "main.rs:293-299 demotes `MinerError::Scan(msg)` to PreflightFailed via `msg.starts_with(\"unknown scan:\")`. The matching string is produced in engine/mod.rs:194 via format!. A future refactor to the format string silently reclassifies unknown-scan preflight failures as runtime errors with no compile-time gate. The typed PreflightCode::UnknownScan path through engine::preflight::resolve_scan is bypassed by run_one's inlined registry.get."
    artifacts:
      - path: "crates/miner-core/src/engine/mod.rs"
        issue: "Line 192-195: run_one inlines `registry.get(...)` instead of routing through engine::preflight::resolve_scan which already returns a typed WireError(UnknownScan). The error becomes an untyped MinerError::Scan(String) that the CLI must string-match."
      - path: "crates/miner-cli/src/main.rs"
        issue: "Lines 293-299: substring-match dispatch on MinerError::Scan(msg)."
    missing:
      - "Either (a) route run_one's scan resolution through engine::preflight::resolve_scan and propagate the typed WireError up, or (b) add a typed MinerError::Preflight(PreflightCode, String) variant and dispatch on it from the CLI."
      - "Add a compile-time gate (e.g., a const &'static str shared between engine/mod.rs and the CLI) so a future format-string drift is caught at build time."
human_verification:
  - test: "Reader-error torn-framing reproduction"
    expected: "When `miner scan` runs against a cache pointing at a missing or unreadable directory, the JSONL stdout should contain BOTH `\"kind\":\"run_start\"` AND `\"kind\":\"run_end\"` envelopes (the run_end carries the failure context via summary.scan_errors >= 1). Currently the stream contains only run_start."
    why_human: "Requires constructing an erroring reader (corrupt zstd, missing day file) and observing stdout JSONL bytes — a behavioral test the verifier did not execute end-to-end."
  - test: "SIGINT + sink-broken-pipe exit code"
    expected: "Pipe `miner scan ... | head -1` then immediately close the pipe. The miner process should observe a broken pipe AND a SIGINT (if interrupted) and exit 130 (cancel wins), not 1."
    why_human: "Requires shell scripting + signal timing observation."
---

# Phase 3: Scan Engine, Facade & CLI Verification Report

**Phase Goal:** User can register and invoke a versioned scan through a single facade, run one end-to-end scan via the CLI with look-ahead-safe windowing, and choose a `strict` or `continuous_only` gap policy that miner enforces before producing findings.

**Verified:** 2026-05-18T20:30:00Z
**Status:** gaps_found
**Re-verification:** No — initial verification.

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria + Phase Goal)

| #   | Truth (ROADMAP SC)                                                                                                                                                                                              | Status       | Evidence                                                                                                                                                                                                                                                            |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | SC-1 (OP-01): User can run `miner scan <name@version> --instrument ... --timeframe ... --window ...` and receive NDJSON for the demo scan with resolved params echoed                                            | ✓ VERIFIED   | `cargo test -p miner-cli --test scan_subcommand_smoke -- scan_emits_run_start_result_run_end` passes. Source: `crates/miner-cli/tests/scan_subcommand_smoke.rs` invokes the binary, asserts exit 0 + [run_start, result, run_end] + resolved-params echo.            |
| 2   | SC-2 (OP-07, OP-08): User can introspect `miner scans`; unknown scan + invalid params rejected at boundary with structured errors                                                                                | ✓ VERIFIED   | `cargo test -p miner-cli --test scans_catalogue` (1/1 pass), `unknown_scan_emits_wireerror_exit_1` + `invalid_params_emits_wireerror_exit_1` (2/2 pass). Catalogue line validates against `schemas/scans-catalogue-v1.schema.json` AND fails `findings-v1.schema.json`. |
| 3   | SC-3 (OUT-04): `--gap-policy strict` aborts with one GapAborted carrying the manifest; `continuous_only` partitions into gap-free sub-ranges and inlines manifest; never silently emits over a hole              | ✓ VERIFIED   | 5 named tests pass (`strict_with_gaps_emits_single_gap_aborted`, `continuous_only_partitions_and_inlines_manifest`, `strict_zero_gaps_emits_result_with_none_manifest`, `continuous_only_zero_gaps_emits_empty_manifest`, `never_silently_emits_on_hole_proptest`).      |
| 4   | SC-4 (OP-05): User can `--dry-run` and see resolved job + data_slice + estimated_findings_count                                                                                                                   | ✓ VERIFIED   | `cargo test -p miner-core --test dry_run -- dry_run_emits_dry_run_finding_only` passes. Emits [RunStart, DryRun, RunEnd]; results_emitted == 0; no `dry_run_emitted` counter substring (Warning 9 pin).                                                              |
| 5   | SC-5 (OP-06): User can interrupt a long-running scan via SIGINT and keep every streamed finding; rayon worker pool shuts down cleanly; exit code 130                                                              | ⚠️ PARTIAL    | Happy-path `sigint_preserves_already_streamed_findings_and_exits_130` passes (verified). BUT: corner-case **CR-01 (torn RunStart/RunEnd on reader/cache error)** and **CR-02 (SIGINT + non-preflight error → exit 1 not 130)** violate the "clean shutdown" contract. |
| 6   | SC-6 (OUT-03): Byte-identical re-runs (sorted, BTreeMap, seeded RNG); shuffled-future regression — pre-T stats unchanged when post-T bars shuffled                                                                | ✓ VERIFIED   | `twice_run_byte_identical_when_volatile_fields_masked` passes; `look_ahead_safe_under_post_t_shuffle` proptest passes with Warning 10 exact doc-comment phrasing present in source.                                                                                  |

**Score:** 5/6 fully verified; 1 partial (SC-5 happy path verified; corner-case contract violations surfaced — see gaps).

### Required Artifacts

| Artifact                                                                                          | Expected                                                                                                                                                                                                                                | Status     | Details                                                                                                                                              |
| ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/miner-core/src/scan/mod.rs`                                                               | `pub trait Scan: Send + Sync` with `id`, `version`, `param_schema`, `finding_fields`, `run`; ScanCtx, ScanRequest, ScanError, ScanFindingShape support types                                                                              | ✓ VERIFIED | 23 KB, full bodies; `pub trait Scan: Send + Sync` confirmed at line; dyn-safety regression test compiles                                              |
| `crates/miner-core/src/scan/registry.rs`                                                          | `Registry { scans: BTreeMap<(String, u32), Box<dyn Scan>> }`; `new`/`register`/`get`/`iter`/`bootstrap`                                                                                                                                  | ✓ VERIFIED | BTreeMap-backed (confirmed via grep); `bootstrap()` registers LjungBoxScan; 5 unit tests pass.                                                       |
| `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}`                                          | LjungBoxScan impl emitting full ResultFinding envelope; pure kernels (log_returns, biased_acf, ljung_box_q_and_p) matching statsmodels 0.14.6                                                                                            | ✓ VERIFIED | 690 + 314 LOC; 15 + 12 unit tests pass; statsmodels golden test `ljung_box_matches_statsmodels_golden` passes within 1e-12.                          |
| `crates/miner-core/src/engine/mod.rs` — `run_one` facade                                          | `pub fn run_one<R: Reader>(...) -> Result<RunOutcome, MinerError>`; 7-step body: cancel → preflight → RunStart → dry-run → gap detection → gap dispatch → RunEnd                                                                          | ⚠️ ORPHANED  | Function exists and 12 engine::tests + 3 cancellation_tests pass. BUT: error-path branches (steps 5 and 6's cache loads) propagate via `?` skipping RunEnd. |
| `crates/miner-core/src/engine/{param_hash,framing,preflight,gap_policy}.rs`                       | Sub-modules: blake3 param_hash; pure framing builders; preflight (resolve_scan, parse_params_kv, parse_iso_utc_window); gap_policy::dispatch                                                                                              | ✓ VERIFIED | 5 modules, 37 unit tests pass (4 + 4 + 20 + 9). param_hash byte-stable, framing clock-isolated, preflight A3 strict-Z, gap_policy proptest pins SC-3e. |
| `crates/miner-core/src/findings/mod.rs` — `Finding::DryRun` variant + `DataSlice.gap_manifest`    | Additive envelope changes per D3-10 / D3-21                                                                                                                                                                                              | ✓ VERIFIED | `Finding::DryRun(DryRunFinding)` variant present; `DataSlice.gap_manifest: Option<GapManifest>` present; both serialize as `null` when absent.        |
| `crates/miner-core/src/findings/sink.rs` — `FindingSink::write_raw_json`                          | New trait method + 3 impls (Stdout / File / Vec) for `miner scans` catalogue lines                                                                                                                                                       | ✓ VERIFIED | 4 occurrences of `fn write_raw_json` in sink.rs; `miner scans` uses it.                                                                              |
| `crates/miner-cli/src/{cli.rs,main.rs,scan_args.rs}`                                              | Command::Scan(ScanArgs) + Command::Scans; ctrlc::set_handler installed BEFORE Cli::parse (Pitfall 2); compute_exit_code routes 0/1/2/130                                                                                                  | ⚠️ ORPHANED  | All artifacts present. Pitfall 2 ordering verified: ctrlc at line 63, Cli::parse at line 78. BUT: compute_exit_code only runs on Ok arm (see CR-02). |
| `schemas/findings-v1.schema.json`                                                                 | Additively regenerated: DataSlice.gap_manifest property + Finding oneOf dry_run arm                                                                                                                                                       | ✓ VERIFIED | `"const": "dry_run"` present; `gap_manifest` property present; `xtask gen-schema` is idempotent (git diff exits 0).                                  |
| `schemas/scans-catalogue-v1.schema.json`                                                          | New sibling schema for `miner scans` catalogue lines (scan_id, version, params, finding_fields)                                                                                                                                          | ✓ VERIFIED | File exists; required keys present; `scans_catalogue.rs` validates against this schema.                                                              |
| `crates/miner-core/tests/fixtures/{generate_golden.py,ljung_box_golden.json}`                     | Python provenance script + committed JSON golden with statsmodels==0.14.6 + input SHA + script path (Blocker 4 / D3-05 — statsmodels-to-Rust direction)                                                                                  | ✓ VERIFIED | Both files exist; provenance block carries `"statsmodels_version": "0.14.6"` + `input_sha256`; Rust test loads via include_str! and asserts version. |
| `crates/miner-core/tests/snapshots/scan_ljung_box__ljung_box_matches_statsmodels_golden.snap`     | Insta snapshot of the masked envelope                                                                                                                                                                                                    | ✓ VERIFIED | Snapshot file exists with the expected envelope shape (acf, q_stats, p_values arrays present).                                                       |
| `crates/miner-core/Cargo.toml` + `crates/miner-cli/Cargo.toml` — `test-internal` feature          | Cfg-gates the Pitfall 8 sleep hook out of release builds                                                                                                                                                                                 | ✓ VERIFIED | `test-internal = []` in miner-core; `test-internal = ["miner-core/test-internal"]` in miner-cli; release binary `--help` shows 0 occurrences of `sleep-after-first-finding`. |
| `README.md` — Quickstart additions                                                                | `miner scan` + `miner scans` + `--dry-run` + SIGINT + gap-policy examples                                                                                                                                                                 | ✓ VERIFIED | "Running a Scan (Phase 3)" section present per SUMMARY 06.                                                                                           |

### Key Link Verification

| From                                              | To                                                                                | Via                                                                                              | Status   | Details                                                                                                |
| ------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------ |
| `miner-cli/src/main.rs`                           | `miner-core::engine::run_one`                                                     | `handle_scan_subcommand` -> `engine::run_one(&req, cfg, &reader, sink, cancel)`                  | ✓ WIRED  | Confirmed in main.rs line ~285; SIGINT test exercises the end-to-end call path.                        |
| `miner-cli/src/main.rs`                           | `miner-reader-dukascopy::DukascopyReader::new`                                    | Constructor at the binary edge                                                                   | ✓ WIRED  | `DukascopyReader::new(cfg.cache_root.clone())` invocation in main.rs.                                  |
| `miner-cli/src/scan_args.rs::to_scan_request`     | `engine::preflight::{resolve_scan_id_at_version, parse_params_kv, parse_iso_utc_window}` | Boundary preflight                                                                       | ✓ WIRED  | `to_scan_request` chains these helpers; ScanArgs unit tests assert each rejection path.                |
| `miner-cli/src/main.rs::handle_scans_subcommand`  | `FindingSink::write_raw_json`                                                     | Per-scan catalogue line emission                                                                 | ✓ WIRED  | `sink.write_raw_json(&line)` for each `&dyn Scan` in bootstrap().                                      |
| `ScanArgs` `--sleep-after-first-finding-ms`       | `ScanRequest.sleep_after_first_finding_ms` cfg-gated field → `LjungBoxScan::run` cancel-aware sleep loop | Chained constructor `ScanRequest::new(...).with_sleep_after_first_finding_ms(...)` | ✓ WIRED  | SIGINT integration test passes; the Pitfall 8 hook makes the SIGINT race deterministic.                  |
| `engine::run_one` `RunStart`                      | `engine::run_one` `RunEnd`                                                        | Every run terminates with a clean RunEnd envelope (D-09 wire-protocol invariant)                 | ⚠️ PARTIAL | Reader-error path emits RunStart then propagates via `?` skipping the RunEnd emit. See **CR-01**.       |
| `compute_exit_code(cancel, outcome)`              | `std::process::exit(code)` — D3-24 four-tier routing                              | Single call site at main.rs                                                                      | ⚠️ PARTIAL | Only invoked on the `Ok` arm of `handle_scan_subcommand`. Err path bypasses cancel-check. See **CR-02**. |

### Data-Flow Trace (Level 4)

| Artifact                              | Data Variable          | Source                                                                          | Produces Real Data | Status        |
| ------------------------------------- | ---------------------- | ------------------------------------------------------------------------------- | ------------------ | ------------- |
| `miner scan` stdout (Finding::Result) | `effect.value` etc.    | `LjungBoxScan::run` → kernel → `Q_max_lag` from `acf` over `close` BarFrame     | ✓ Yes              | ✓ FLOWING     |
| `miner scans` stdout                  | catalogue line object  | `bootstrap()` → `Scan::{id,version,param_schema,finding_fields}`                | ✓ Yes              | ✓ FLOWING     |
| `data_slice.gap_manifest` (Result)    | `Some(GapManifest)`    | `GapDetector::detect` → `gap_policy::dispatch` → engine inlines into ScanCtx    | ✓ Yes              | ✓ FLOWING     |
| `Finding::DryRun` payload             | resolved_params, etc.  | `engine::run_one` step 4 → `req.resolved_params` (from CLI preflight)           | ✓ Yes              | ✓ FLOWING     |

### Behavioral Spot-Checks

| Behavior                                                                                | Command                                                                            | Result                                                                | Status   |
| --------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | --------------------------------------------------------------------- | -------- |
| Workspace build clean                                                                   | `cargo build --workspace`                                                          | Finished `dev` profile, 0 errors                                      | ✓ PASS   |
| Full test suite passes                                                                  | `cargo test --workspace --all-targets`                                             | 258 passed; 0 failed                                                  | ✓ PASS   |
| Clippy clean                                                                            | `cargo clippy --workspace --all-targets -- -D warnings`                            | Finished, no warnings                                                 | ✓ PASS   |
| Cargo fmt clean                                                                         | `cargo fmt --all --check`                                                          | Exit 0                                                                | ✓ PASS   |
| Schema regeneration idempotent                                                          | `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/`                | Exit 0                                                                | ✓ PASS   |
| No async deps in miner-core runtime tree (FOUND-04 gate)                                | `cargo tree -p miner-core -e normal \| grep -iE 'tokio\|async-std\|smol'`          | 0 matches                                                             | ✓ PASS   |
| No `preserve_order` in Cargo.lock (Pitfall 1 gate)                                      | `grep -c preserve_order Cargo.lock`                                                | 0                                                                     | ✓ PASS   |
| `miner scans` emits 1 catalogue line (Phase 3)                                          | `./target/debug/miner scans`                                                       | One JSONL line for `stats.autocorr.ljung_box@1` with 4 required keys  | ✓ PASS   |
| Release binary does NOT expose `--sleep-after-first-finding-ms` (T-03-05-05)            | `./target/release/miner scan --help \| grep -c sleep-after-first-finding`          | 0                                                                     | ✓ PASS   |
| SIGINT integration test passes                                                          | `cargo test -p miner-cli --test sigint_preserves_stream`                           | 1 passed                                                              | ✓ PASS   |
| Reader-error torn-framing check                                                         | (No automated test exists for this — see human verification)                       | (Inspection of engine/mod.rs:250-323 confirms `?` propagation)        | ✗ FAIL   |

### Requirements Coverage

| Requirement | Source Plan(s)               | Description                                                                                       | Status                         | Evidence                                                                                                                                                                  |
| ----------- | ---------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| OP-01       | 03-01,03-04,03-05,03-06      | `miner scan <name@version>` CLI invocation produces findings                                       | ✓ SATISFIED                    | `scan_emits_run_start_result_run_end` passes; happy-path NDJSON flow confirmed.                                                                                          |
| OP-05       | 03-01,03-04,03-05,03-06      | `--dry-run` shows resolved job + data_slice before committing                                      | ✓ SATISFIED                    | `dry_run_emits_dry_run_finding_only` passes; results_emitted == 0 (Pitfall 3 pin).                                                                                       |
| OP-06       | 03-01,03-04,03-05,03-06      | SIGINT preserves streamed findings; clean shutdown; exit 130                                       | ⚠️ PARTIAL                      | Happy SIGINT path verified by `sigint_preserves_already_streamed_findings_and_exits_130`. Corner cases (SIGINT + reader/cache error) — see CR-01/CR-02 gaps.            |
| OP-07       | 03-01,03-02,03-05,03-06      | `miner scans` catalogue introspection emits scan name, version, params, finding_fields            | ✓ SATISFIED                    | `scans_emits_one_line_per_registered_scan` passes; sibling-schema validation positive + findings-schema validation negative.                                              |
| OP-08       | 03-01,03-02,03-03,03-04,03-05,03-06 | Boundary validation: unknown scan + invalid params rejected; resolved params echoed              | ✓ SATISFIED (with CR-03 note)  | Boundary rejections work via preflight; resolved params echoed. CR-03 (string-match dispatch on MinerError::Scan) is a maintainability hazard, not a current bug.        |
| OUT-04      | 03-01,03-02,03-03,03-04,03-06 | Findings carry actual consumed range + gap-manifest reference; strict aborts with single record   | ✓ SATISFIED                    | 5 gap_policy tests pass; data_slice.gap_manifest inlined under continuous_only; strict emits single GapAborted with manifest.                                            |

All 6 declared requirement IDs cross-reference cleanly against REQUIREMENTS.md (which maps each to Phase 3). No orphaned requirements detected.

### Anti-Patterns Found

| File                                            | Line(s)        | Pattern                                                          | Severity     | Impact                                                                                                                       |
| ----------------------------------------------- | -------------- | ---------------------------------------------------------------- | ------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| `crates/miner-core/src/engine/mod.rs`           | 250-251, 313-323, 370-373 | `?` propagation skips emit_run_end on reader/cache/IO errors      | ⚠️ Warning   | Torn RunStart/RunEnd framing — violates D-09 wire-protocol invariant. See CR-01 gap.                                          |
| `crates/miner-cli/src/main.rs`                  | 107-111        | `?` short-circuits compute_exit_code on engine errors            | ⚠️ Warning   | SIGINT-overrides-everything contract (D3-24) bypassed when cancel coincides with non-preflight engine error. See CR-02 gap. |
| `crates/miner-cli/src/main.rs`                  | 293-299        | Substring match `msg.starts_with("unknown scan:")` on MinerError::Scan | ⚠️ Warning   | Fragile coupling between engine format string and CLI dispatch; future format-string drift silently regresses preflight classification. See CR-03 gap. |
| `crates/miner-cli/src/scan_args.rs`             | 124            | `_code_revision: &str` parameter underscored (ignored) yet documented as dependency-injectable | ℹ️ Info     | Misleading API surface; doc-comment claims injection that does not occur. WR-01 from code review.                              |
| `crates/miner-cli/tests/sigint_preserves_stream.rs` | 45-59     | Hardcoded `target/debug/miner` path ignores `CARGO_TARGET_DIR`     | ℹ️ Info      | CI environments with non-default target dirs will spawn stale binaries. WR-02 from code review. Test passes locally.        |
| `crates/miner-core/src/findings/sink.rs`        | 173-180        | `FileSink::create` does not `create_dir_all` for parent           | ℹ️ Info      | UX papercut; `--output ~/.local/share/miner/findings.jsonl` fails with opaque NotFound. WR-03 from code review.              |
| `crates/miner-core/src/engine/framing.rs`       | 124-136        | `wall_clock_ms = signed_duration_since.num_milliseconds()` can be negative on clock skew | ℹ️ Info      | Non-monotonic clocks could produce negative durations. IN-04 from code review. Schema accepts the value; consumers may misbehave. |
| `crates/miner-cli/tests/scan_subcommand_smoke.rs` | 163-202     | Test name `invalid_params_emits_wireerror_exit_1` exercises `--side`, not `--params` | ℹ️ Info      | Test-name vs body drift; coverage perception incorrect. IN-02 from code review.                                              |
| `crates/miner-core/tests/common/mod.rs` + `crates/miner-cli/tests/fixtures/mod.rs` | n/a | Three duplicated copies of `mask_volatile_fields` kept in sync manually | ℹ️ Info      | Drift risk on future framing additions. WR-07 from code review.                                                              |

### Human Verification Required

The following items require human judgement / behavioral observation beyond what the verifier could automate:

#### 1. Reader-error torn-framing reproduction

**Test:** Run `miner scan stats.autocorr.ljung_box@1 --instrument EURUSD --side bid --timeframe 15m --window 2024-01-01:2024-01-02 --cache-root /tmp/empty-dir`

**Expected:** stdout JSONL should contain BOTH a `"kind":"run_start"` envelope AND a `"kind":"run_end"` envelope (with summary.scan_errors >= 1 documenting the reader failure). The contract (D-09) is "every run terminates with RunEnd". Currently the stream contains only run_start before the binary exits via anyhow with status 1.

**Why human:** Requires constructing a cache that errors deterministically and observing stdout JSONL byte stream — a behavioral test the verifier did not execute end-to-end. Code-path inspection (engine/mod.rs:250-323) confirms `?` propagation skips emit_run_end.

#### 2. SIGINT + sink-broken-pipe exit code

**Test:** Run `miner scan ... --sleep-after-first-finding-ms 5000 | head -1` (pipe to head which closes after one line). After the pipe closes, the writer hits EPIPE / BrokenPipe. If SIGINT is ALSO delivered before the binary returns, the process should exit 130 (cancel wins per D3-24).

**Expected:** Exit code 130 regardless of which error class fired.

**Why human:** Requires shell-level signal + pipe timing; not an automated test in the suite. Code review (main.rs:108) confirms compute_exit_code is bypassed on the Err arm.

#### 3. Phase 4 forward compatibility

**Test:** When Phase 4 lands additional rolling/causal scans, do they add per-scan `cancellation_tests`-style proptests as documented in shuffled_future_regression.rs's Warning 10 phrasing? Is the shape of the per-scan proptest discoverable from this Phase 3 code (the canonical example)?

**Expected:** A Phase 4 plan author can read `crates/miner-core/tests/shuffled_future_regression.rs` and replicate the pattern.

**Why human:** Forward-looking; verifies Phase 3 leaves a clean scaffolding pattern for Phase 4 to build on. Not a current bug, but a Phase 3 deliverable.

### Gaps Summary

Three gaps are surfaced from the code review (03-REVIEW.md), all corner-case correctness or maintainability issues that the happy-path test suite does not exercise:

1. **CR-01 (Warning)** — Reader/cache errors at engine::run_one steps 5/6 emit RunStart but skip the closing RunEnd via `?` propagation. Violates D-09 wire-protocol invariant. No automated regression test catches this; the existing `run_one_reader_error_wraps_via_miner_error_scan` test asserts only the error variant, never the sink contents.

2. **CR-02 (Warning)** — main.rs:108 short-circuits compute_exit_code on the Err arm of handle_scan_subcommand. CONTEXT D3-24 mandates `cancelled → 130 regardless of tier`; currently the cancel flag is ignored on every non-preflight error path. SIGINT + reader/cache error yields exit 1 instead of 130.

3. **CR-03 (Warning)** — main.rs:293-299 demotes preflight failures by substring-matching `"unknown scan:"` against MinerError::Scan(String). The format string is produced in engine/mod.rs:194. A future refactor silently regresses the classification with no compile-time gate. The typed `engine::preflight::resolve_scan` path (which returns `WireError(UnknownScan)`) is bypassed by run_one's inlined `registry.get`.

The happy-path goal is observably achieved: 258 tests pass, schemas idempotent, release binary clean, no async deps, all six ROADMAP success criteria have positive automated evidence. The three gaps surface real contract violations identified by code review that the test suite does not exercise. These should be triaged by the developer:

- **Option A (recommended):** Fix CR-01 + CR-02 in a follow-up Phase 3 plan or as a Phase 4 prerequisite plan. CR-03 can land as a refactor in Phase 4 when more scans are registered.
- **Option B:** Accept the gaps as known issues and document them in STATE.md as Phase 4 entry conditions.

---

_Verified: 2026-05-18T20:30:00Z_
_Verifier: Claude (gsd-verifier)_

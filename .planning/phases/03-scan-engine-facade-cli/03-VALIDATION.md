---
phase: 3
slug: scan-engine-facade-cli
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-18
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `03-RESEARCH.md` § "Validation Architecture" (lines 818–911).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` + integration tests under `crates/*/tests/`; `proptest 1.11` + `insta 1.47` + `assert_cmd 2` + `nix 0.31` (signal feature) + `serial_test 3` + `jsonschema 0.46` |
| **Config file** | Workspace `Cargo.toml` (dev-deps + lints); per-crate `Cargo.toml` — no separate test config |
| **Quick run command** | `cargo test --workspace --lib` |
| **Full suite command** | `cargo test --workspace --all-targets` |
| **Estimated runtime** | ~30 seconds full suite; ~few seconds unit-only |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --workspace --lib`
- **After every plan wave:** Run `cargo test --workspace --all-targets` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all --check`
- **Before `/gsd:verify-work`:** Full suite must be green; `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/findings-v1.schema.json` must succeed after type-change commits; `cargo tree -p miner-core | grep -E 'tokio|async-std'` must return empty (no async creep into core)
- **Max feedback latency:** ~30 seconds full suite; few seconds for unit subset after a single-file edit

---

## Per-Task Verification Map

> The Plan numbering (P-XX) is populated by the planner. Below is the requirement → behaviour → automated-command map derived from RESEARCH.md. Once PLAN.md files exist, plan-checker is expected to cross-reference task IDs into this table.

| Req / SC | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|----------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| OP-01 / SC-1 | TBD | TBD | OP-01 | — | NDJSON findings emitted on stdout with resolved params echoed | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- scan_emits_run_start_result_run_end` | ❌ W0 | ⬜ pending |
| OP-07 / SC-2a | TBD | TBD | OP-07 | — | One JSONL line per registered scan with name+version+param_schema+finding_fields | integration | `cargo test -p miner-cli --test scans_catalogue -- scans_emits_one_line_per_registered_scan` | ❌ W0 | ⬜ pending |
| OP-08 / SC-2b | TBD | TBD | OP-08 | — | Unknown scan_id rejected with `PreflightCode::UnknownScan` on stderr, exit 1 | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- unknown_scan_emits_wireerror_exit_1` | ❌ W0 | ⬜ pending |
| OP-08 / SC-2c | TBD | TBD | OP-08 | — | Invalid `--params KEY=VAL` rejected with `PreflightCode::InvalidParameter`, exit 1 | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- invalid_params_emits_wireerror_exit_1` | ❌ W0 | ⬜ pending |
| OUT-04 / SC-3a | TBD | TBD | OUT-04 | — | strict + gaps → one `Finding::GapAborted` carrying full manifest, exit 0 | integration | `cargo test -p miner-core --test gap_policy -- strict_with_gaps_emits_single_gap_aborted` | ❌ W0 | ⬜ pending |
| OUT-04 / SC-3b | TBD | TBD | OUT-04 | — | continuous_only partitions into sub-ranges; each finding's `data_slice.gap_manifest` inlined | integration | `cargo test -p miner-core --test gap_policy -- continuous_only_partitions_and_inlines_manifest` | ❌ W0 | ⬜ pending |
| OUT-04 / SC-3c | TBD | TBD | OUT-04 | — | strict + zero gaps fast path: no GapAborted, `data_slice.gap_manifest = None` | integration | `cargo test -p miner-core --test gap_policy -- strict_zero_gaps_emits_result_with_none_manifest` | ❌ W0 | ⬜ pending |
| OUT-04 / SC-3d | TBD | TBD | OUT-04 | — | continuous_only + zero gaps: one Result, `gap_manifest = Some({gaps: []})` | integration | `cargo test -p miner-core --test gap_policy -- continuous_only_zero_gaps_emits_empty_manifest` | ❌ W0 | ⬜ pending |
| OUT-04 / SC-3e | TBD | TBD | OUT-04 | — | Never silently emit on a hole — proptest invariant across random gap configurations | integration / proptest | `cargo test -p miner-core --test gap_policy -- never_silently_emits_on_hole_proptest` | ❌ W0 | ⬜ pending |
| OP-05 / SC-4 | TBD | TBD | OP-05 | — | `--dry-run` emits `Finding::DryRun` then exits 0 with zero `Result` findings | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- dry_run_emits_dry_run_finding_only` | ❌ W0 | ⬜ pending |
| OP-06 / SC-5a | TBD | TBD | OP-06 | — | SIGINT after first finding → exit 130; all already-streamed findings persist on stdout | integration | `cargo test -p miner-cli --test sigint_preserves_stream -- sigint_preserves_already_streamed_findings_and_exits_130` | ❌ W0 | ⬜ pending |
| OP-06 / SC-5b | TBD | TBD | OP-06 | — | Cancel-token polled at every documented yield site; scan exits early | unit | `cargo test -p miner-core engine::cancellation_tests::cancel_at_*` | ❌ W0 | ⬜ pending |
| OUT-03 / SC-6a | TBD | TBD | OUT-03 | — | Same inputs → byte-identical JSONL (run_id + timestamps redacted) | integration | `cargo test -p miner-core --test scan_facade_determinism -- twice_run_byte_identical_when_volatile_fields_masked` | ❌ W0 | ⬜ pending |
| OUT-03 / SC-6b | TBD | TBD | OUT-03 | — | Shuffled-future regression: stats up to T unchanged when bars > T are shuffled | integration / proptest | `cargo test -p miner-core --test shuffled_future_regression -- look_ahead_safe_under_post_t_shuffle_proptest` | ❌ W0 | ⬜ pending |
| D3-01 / D3-05 | TBD | TBD | OP-01 | — | Ljung-Box output matches statsmodels 0.14.6 golden bytes within documented tolerance | integration / insta | `cargo test -p miner-core --test scan_ljung_box -- ljung_box_matches_statsmodels_golden` | ❌ W0 | ⬜ pending |
| Schema-additivity | TBD | TBD | — | — | xtask gen-schema emits only additive diff vs committed schema | unit (xtask) + manual | `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/findings-v1.schema.json` | ✅ xtask exists; review is manual | ⬜ pending |
| D3-13 | TBD | TBD | OP-08 | — | `param_hash` byte-stable across runs; matches blake3-of-canonical-JSON | unit | `cargo test -p miner-core engine::param_hash_tests::param_hash_is_byte_stable` | ❌ W0 | ⬜ pending |
| D3-19 | TBD | TBD | OP-01 | — | `--side` defaults to bid; `--gap-policy` defaults to continuous_only | unit (clap) | `cargo test -p miner-cli cli::scan_args_tests::defaults_per_d3_19` | ❌ W0 | ⬜ pending |
| D3-24 | TBD | TBD | OP-06 | — | Exit-code routing 0 / 1 / 2 / 130 covered by integration cases | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- exit_code_routing_*` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Wave 0 (preceding any scan-engine implementation) must scaffold the test harness and source-file stubs before scan-engine code lands.

### New source files
- [ ] `crates/miner-core/src/scan/mod.rs` — `Scan` trait, `ScanCtx`, `ScanRequest`, `ScanError`, `ScanFindingShape`
- [ ] `crates/miner-core/src/scan/registry.rs` — `Registry::{new, register, get, iter}` + `bootstrap()`
- [ ] `crates/miner-core/src/scan/ljung_box/mod.rs` — `LjungBoxScan: Scan` impl
- [ ] `crates/miner-core/src/scan/ljung_box/kernel.rs` — pure `log_returns`, `biased_acf`, `ljung_box_q_and_p` kernels + unit tests
- [ ] `crates/miner-core/src/engine/mod.rs` — `run_one` facade entry + `RunOutcome` enum
- [ ] `crates/miner-core/src/engine/preflight.rs` — `--params` parser, scan-id resolver, error mapping
- [ ] `crates/miner-core/src/engine/gap_policy.rs` — strict / continuous_only dispatch + partitioning
- [ ] `crates/miner-core/src/engine/param_hash.rs` — `param_hash(resolved: &Value) -> Blake3Hex`
- [ ] `crates/miner-core/src/engine/framing.rs` — `RunStart` / `RunEnd` builders
- [ ] `crates/miner-core/src/findings/mod.rs` — extend `DataSlice` + `Finding` enum (DryRun variant) + `DryRunFinding` struct
- [ ] `crates/miner-cli/src/cli.rs` — extend `Command` enum with `Scan(ScanArgs)` + `Scans`
- [ ] `crates/miner-cli/src/scan_args.rs` — `ScanArgs` + `--window` parser + repeatable `--params`
- [ ] `crates/miner-cli/src/main.rs` — `ctrlc::set_handler` install + facade plumbing + exit-code routing

### New test files
- [ ] `crates/miner-core/tests/scan_ljung_box.rs` — golden-fixture insta snapshot
- [ ] `crates/miner-core/tests/scan_facade_determinism.rs` — twice-run masked-byte-equality
- [ ] `crates/miner-core/tests/shuffled_future_regression.rs` — D3-09 proptest
- [ ] `crates/miner-core/tests/gap_policy.rs` — 5 gap-policy behaviour tests
- [ ] `crates/miner-core/tests/dry_run.rs` — `Finding::DryRun` shape + RunSummary.results_emitted == 0
- [ ] `crates/miner-cli/tests/scan_subcommand_smoke.rs` — assert_cmd happy path
- [ ] `crates/miner-cli/tests/scans_catalogue.rs` — `miner scans` introspection
- [ ] `crates/miner-cli/tests/sigint_preserves_stream.rs` — `#[cfg(unix)]` nix::kill integration
- [ ] `crates/miner-cli/tests/fixtures/` — synthetic SyntheticCache builder + Ljung-Box AR(1) seed + expected JSONL golden

### Schemas
- [ ] `schemas/findings-v1.schema.json` regenerated by `xtask gen-schema` after Rust type changes (committed alongside the type-change task)
- [ ] (Conditional) `schemas/scans-catalogue-v1.schema.json` — sibling schema for `miner scans` lines (pending Open Question 8 resolution)

### Workspace dev-deps
- [ ] `ctrlc = "3.5"` (binary dep in `miner-cli`)
- [ ] `statrs = "0.17"` (dep in `miner-core` for `ChiSquared::cdf`)
- [ ] `nix = { version = "0.31", default-features = false, features = ["signal"] }` (dev-dep in `miner-cli` for SIGINT integration test)

### Reused existing infrastructure (no Wave 0 work needed)
- `xtask gen-schema` subcommand
- `StdoutSink` / `FileSink` / `VecSink` (existing `FindingSink` impls)
- `WireError` + `emit_to_stderr` + `classify_figment_error`
- `BarCache::get_or_build` + `GapDetector::detect` + `Calendar`
- `chrono::Utc` + `ulid::Ulid` (via `RunId::new()`)
- `assert_cmd::Command::cargo_bin("miner")` pattern (from `cli_streams.rs`)
- `serial_test::serial` discipline for env-touching tests

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Review of `xtask gen-schema` diff for the two additive changes (`DataSlice.gap_manifest`, `Finding::DryRun`) | — | The diff itself is auto-generated; the human-review gate confirms the diff is purely additive (no `schema_version` bump warranted) | `cargo run -p xtask -- gen-schema && git diff schemas/findings-v1.schema.json` — inspect; commit only if additive |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references in the verification map
- [ ] No watch-mode flags (`cargo test` runs to completion; no `cargo watch`)
- [ ] Feedback latency < 60 s for full suite; < 5 s for `--lib` subset
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

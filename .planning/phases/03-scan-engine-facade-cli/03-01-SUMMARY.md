---
phase: 03-scan-engine-facade-cli
plan: 01
subsystem: scan-engine-facade-cli
tags: [scaffold, wave-0, scan-trait, engine-facade, cli-args, integration-tests]
requires:
  - "01-foundations-contracts (Plan 03 Finding envelope; FindingSink trait; PreflightCode/ScanErrorCode vocab; MinerConfig; FROZEN public surface)"
  - "02-reader-aggregator-derived-bar-cache (Reader trait, BarFrame, BarCache::get_or_build, GapDetector/GapManifest/GapReason/Side/ClosedRangeUtc)"
provides:
  - "Empty-body `Scan` trait + `ScanCtx<'a>` + `ScanRequest` + `ScanError` types in `miner_core::scan` (dyn-safe; trait-object-safety compile-time gate held)"
  - "Empty-body `Registry { scans: BTreeMap<(String, u32), Box<dyn Scan>> }` + `bootstrap()` factory in `miner_core::scan::registry`"
  - "Empty-body `LjungBoxScan` unit struct + `Scan` impl with id/version/finding_fields populated as compile-time consts (D3-04)"
  - "Empty-body `run_one<R: Reader>` facade + `RunOutcome` enum in `miner_core::engine`"
  - "Empty-body `engine::{preflight, gap_policy, param_hash, framing}` sub-modules (signatures for `classify_param_error`, `parse_params_kv`, `dispatch`, `param_hash`, `build_run_start`, `build_run_end`, `GapPolicyKind`, `GapDispatch`)"
  - "Empty-body `ScanArgs` clap-derive struct + `parse_window` + `to_scan_request` signatures in `miner_cli::scan_args`"
  - "9 integration test scaffolds with #[ignore]'d / cfg-gated stubs reachable via `cargo test --list`"
  - "Workspace deps: `ctrlc 3.5.2`, `statrs 0.17.1` (workspace + miner-core/miner-cli wiring); `nix 0.31.3` (miner-cli dev-only)"
affects:
  - "Cargo.toml (workspace.dependencies extended)"
  - "crates/miner-core/Cargo.toml (statrs runtime dep added)"
  - "crates/miner-cli/Cargo.toml (ctrlc runtime + nix dev-dep added)"
  - "crates/miner-core/src/lib.rs (pub mod scan; pub mod engine; — TWO new module roots; FROZEN re-export block UNCHANGED at this plan — Plan 03-06 extends)"
  - "crates/miner-cli/src/main.rs (mod scan_args; declaration; no behavioural change)"
tech-stack:
  added:
    - "ctrlc 3.5.2 (workspace + miner-cli runtime — SIGINT handler; D3-22)"
    - "statrs 0.17.1 (workspace + miner-core runtime — ChiSquared distribution for Ljung-Box p-value; D3-04)"
    - "nix 0.31.3 (miner-cli dev-only; default-features = false, features = [\"signal\"] — unix-only SIGINT integration test; 03-PATTERNS line 1048)"
  patterns:
    - "Wave 0 scaffold discipline — every Phase 3 source/test file landed with signature-only bodies (`unimplemented!()` / `todo!()` / `#[ignore]`) so plan-checker harness sees the full file set"
    - "Per-module `#![allow(dead_code, unused_variables)]` on scaffold modules (removed by Plan 03-02..06 as bodies land)"
    - "`#[cfg(disabled_in_scaffold)]` gate on proptest! blocks (proptest macro does not honour `#[ignore]` at the inner test-fn level — gate the whole module instead, Plan 03-06 flips off)"
key-files:
  created:
    - "crates/miner-core/src/scan/mod.rs (184 lines) — Scan trait + ScanCtx + ScanRequest + ScanError + ScanFindingShape re-export"
    - "crates/miner-core/src/scan/registry.rs (81 lines) — Registry + bootstrap() shells"
    - "crates/miner-core/src/scan/shape.rs (33 lines) — ScanFindingShape declarative struct"
    - "crates/miner-core/src/scan/ljung_box/mod.rs (78 lines) — LjungBoxScan + Scan impl skeleton"
    - "crates/miner-core/src/scan/ljung_box/kernel.rs (76 lines) — log_returns/biased_acf/ljung_box_q_and_p signatures"
    - "crates/miner-core/src/engine/mod.rs (126 lines) — run_one facade + RunOutcome enum"
    - "crates/miner-core/src/engine/preflight.rs (60 lines) — classify_param_error + parse_params_kv signatures"
    - "crates/miner-core/src/engine/gap_policy.rs (113 lines) — GapPolicyKind + GapDispatch + dispatch signature"
    - "crates/miner-core/src/engine/param_hash.rs (71 lines) — param_hash signature + ignored byte-stability test stub"
    - "crates/miner-core/src/engine/framing.rs (67 lines) — build_run_start + build_run_end signatures"
    - "crates/miner-cli/src/scan_args.rs (131 lines) — ScanArgs + parse_window + to_scan_request signatures"
    - "crates/miner-core/tests/scan_ljung_box.rs (34 lines, 1 ignored test) — golden snapshot scaffold"
    - "crates/miner-core/tests/scan_facade_determinism.rs (33 lines, 1 ignored test) — twice-run byte-equality scaffold"
    - "crates/miner-core/tests/shuffled_future_regression.rs (48 lines, 1 cfg-gated proptest) — look-ahead-safety scaffold"
    - "crates/miner-core/tests/gap_policy.rs (76 lines, 4 ignored tests + 1 cfg-gated proptest) — five gap-policy scenarios"
    - "crates/miner-core/tests/dry_run.rs (30 lines, 1 ignored test) — Finding::DryRun shape scaffold"
    - "crates/miner-cli/tests/scan_subcommand_smoke.rs (67 lines, 5 ignored tests) — assert_cmd happy + 4 sad paths"
    - "crates/miner-cli/tests/scans_catalogue.rs (35 lines, 1 ignored test) — `miner scans` introspection scaffold"
    - "crates/miner-cli/tests/sigint_preserves_stream.rs (42 lines, 1 ignored test, #![cfg(unix)]) — SIGINT integration scaffold"
    - "crates/miner-cli/tests/fixtures/mod.rs (43 lines) — SyntheticCache + build_ar1_bar_frame shells for Plan 03-06"
  modified:
    - "Cargo.toml — added `ctrlc = \"3.5\"` and `statrs = \"0.17\"` to [workspace.dependencies] (incl. inline rationale comment)"
    - "crates/miner-core/Cargo.toml — added `statrs.workspace = true` to [dependencies]"
    - "crates/miner-cli/Cargo.toml — added `ctrlc.workspace = true` to [dependencies]; added `nix = { version = \"0.31\", default-features = false, features = [\"signal\"] }` to [dev-dependencies]"
    - "crates/miner-core/src/lib.rs — registered `pub mod scan;` and `pub mod engine;`"
    - "crates/miner-cli/src/main.rs — registered `mod scan_args;`"
decisions:
  - "Tokio appears in `cargo tree -p miner-core` ONLY via dev-dep chain (jsonschema → reqwest → tokio). Runtime-only tree (`cargo tree -p miner-core -e normal`) shows ZERO async creep — FOUND-04 gate is about runtime sync+rayon invariant, dev-deps are irrelevant. The plan's `cargo tree -p miner-core | grep tokio` invocation was imprecise; the canonical gate is `-e normal`. No async creep introduced by this plan."
  - "Shuffled-future + gap-policy proptest blocks gated via `#[cfg(disabled_in_scaffold)] mod {...}` rather than `#[ignore]` per plan instruction (proptest! macro doesn't honour the test-fn ignore attribute; gating the wrapping module is the equivalent escape hatch). Plan 03-06 will flip the cfg off when it implements the bodies."
metrics:
  duration_seconds: 795
  completed_date: "2026-05-18T14:40:00Z"
  tasks_completed: 3
  files_touched: 23
---

# Phase 3 Plan 01: Scan Engine Wave 0 Scaffold Summary

Wave 0 scaffold for Phase 3 — laid down every Phase 3 source file (10 new) and every Phase 3 integration test file (9 new + fixtures shell) with signature-only / `unimplemented!()` bodies, plus wired three new workspace deps (`ctrlc`, `statrs`, `nix`) so Plan 03-02..06 fills bodies against compiled contracts.

## One-liner

Phase 3 scaffold landed: every source-file and test-file in the Phase 3 file set exists with compile-clean signature-only bodies; subsequent plans fill behaviours without adding files.

## What changed

### Task 1 — Workspace dep wiring (commit `8f36c24`)

- Added `ctrlc = "3.5"` and `statrs = "0.17"` to `[workspace.dependencies]` in the root `Cargo.toml` with an inline rationale comment.
- Declared `statrs.workspace = true` on `miner-core` (D3-04 — chi-squared p-value for Ljung-Box; the ONLY crate that needs the distribution dep — Pitfall 1 holds).
- Declared `ctrlc.workspace = true` on `miner-cli` runtime deps (D3-22 — SIGINT handler installed at the binary edge; `miner-core` knows nothing of `ctrlc`).
- Declared `nix = { version = "0.31", default-features = false, features = ["signal"] }` inline on `miner-cli` `[dev-dependencies]` (unix-only SIGINT integration test).
- Verification: workspace builds; `cargo tree -p miner-core -e normal` shows ZERO `tokio`/`async-std` deps (runtime sync+rayon invariant held); `Cargo.lock` contains zero `preserve_order` (Pitfall 1 gate held).

### Task 2 — `miner-core` source scaffold (commit `0f95cbc`)

- 10 new source files under `crates/miner-core/src/scan/` and `crates/miner-core/src/engine/` with signature-only bodies. Every file:
  - Carries a module-doc header citing the analog (per 03-PATTERNS).
  - Declares the real type/trait/function signatures shown in 03-PATTERNS (so Plan 03-02..06 can fill bodies in-place without renaming or signature drift).
  - Has bodies as `unimplemented!()` / `todo!()` / `{ /* Plan X fills */ }`.
  - Uses `#![allow(dead_code, unused_variables)]` at the module top so cargo check is warning-free while bodies remain unimplemented.
  - Contains ZERO banned macros (`println!` / `eprintln!` / `dbg!`) per the workspace `clippy.toml`.
- `lib.rs` registers `pub mod scan;` and `pub mod engine;` after `pub mod gap;`. The FROZEN `pub use` re-export block at the bottom of `lib.rs` is UNCHANGED — Plan 03-06 wires the public-surface audit + extends the re-export block when bodies land.
- Verification: `cargo check -p miner-core` is warning-free; `cargo test -p miner-core scan_trait_object_safe --no-run` compiles (the dyn-compat regression gate from `reader.rs:272-274` is preserved on the new `Scan` trait).

### Task 3 — `miner-cli` scaffold + integration tests (commit `cdae530`)

- `crates/miner-cli/src/scan_args.rs` ships the `ScanArgs` clap-derive struct + `parse_window` + `to_scan_request` signatures (Plan 03-02 wires conversion; Plan 03-05 wires the window parser).
- 9 integration test files landed under `crates/miner-core/tests/` and `crates/miner-cli/tests/` with `#[ignore]`'d / cfg-gated stubs reachable via `cargo test --list`:
  - `miner-core`: `scan_ljung_box.rs`, `scan_facade_determinism.rs`, `shuffled_future_regression.rs`, `gap_policy.rs`, `dry_run.rs`.
  - `miner-cli`: `scan_subcommand_smoke.rs`, `scans_catalogue.rs`, `sigint_preserves_stream.rs`, `tests/fixtures/mod.rs`.
- Sigint test gated by `#![cfg(unix)]` at the file level so non-unix CI skips compilation entirely.
- proptest! blocks (which don't honour `#[ignore]` at the inner test-fn level) are gated by `#[cfg(disabled_in_scaffold)]` on the wrapping module — Plan 03-06 flips the gate off when it implements the bodies.
- Verification: `cargo test --workspace --no-run` succeeds; every named test from VALIDATION's Per-Task Verification Map is reachable via `cargo test --list` except the two proptest names (`look_ahead_safe_under_post_t_shuffle` and `never_silently_emits_on_hole_proptest`) which are intentionally cfg-gated per plan instruction; `cargo test -p miner-core --test gap_policy` runs 0 tests (every scaffold test is ignored — T-03-01-03 mitigation held).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking issue] Tokio dev-dep transitive (jsonschema → reqwest → tokio) appears in `cargo tree -p miner-core`**

- **Found during:** Task 1 verification gate.
- **Issue:** The plan's `cargo tree -p miner-core | grep -iE 'tokio|async'` invocation matched dev-only transitive `tokio` (via `jsonschema 0.46` dev-dep → `reqwest 0.13` → `tokio 1.52`). This pre-existed Plan 03-01 — `jsonschema` was added as a `miner-core` dev-dep in Phase 1, and `reqwest` is its remote-schema-fetch transitive.
- **Fix:** Verified the FOUND-04 invariant against the RUNTIME-ONLY tree via `cargo tree -p miner-core -e normal`, which shows ZERO `tokio` / `async-std` / `smol` deps — the runtime sync+rayon invariant is preserved. The plan's gate command was imprecise; the canonical check is `-e normal`. No code change needed.
- **Documented in:** commit message of `8f36c24` + this SUMMARY's `decisions` block.

**2. [Rule 1 - Bug] Format-string syntax error in `to_scan_request` placeholder `{...}`**

- **Found during:** Task 3 `cargo check -p miner-cli`.
- **Issue:** `unimplemented!("... return ScanRequest {...}")` was interpreted as a format string with an unterminated `{` brace.
- **Fix:** Escaped to `{{ ... }}`. One-line change before commit.
- **Files modified:** `crates/miner-cli/src/scan_args.rs` (line 103).
- **Commit:** Folded into `cdae530`.

**3. [Rule 3 - Blocking issue] `use super::*` unused in `scan/mod.rs::tests`**

- **Found during:** Task 3 build pass.
- **Issue:** The trait-object-safety test only needs the path-qualified `crate::scan::Scan`; the `use super::*` import was unused and triggered a warning.
- **Fix:** Removed the import line.
- **Files modified:** `crates/miner-core/src/scan/mod.rs` (lines 173-184).
- **Commit:** Folded into `cdae530`.

**4. [Rule 3 - Blocking issue] `unexpected_cfgs` warning on `#[cfg(disabled_in_scaffold)]` gate**

- **Found during:** Task 3 build pass.
- **Issue:** Rust 2024's `unexpected_cfgs` lint emits a warning on the synthetic `disabled_in_scaffold` cfg name we use to gate the proptest! blocks.
- **Fix:** Added `unexpected_cfgs` to the `#![allow(...)]` at the top of `shuffled_future_regression.rs` and `gap_policy.rs`.
- **Files modified:** `crates/miner-core/tests/shuffled_future_regression.rs`, `crates/miner-core/tests/gap_policy.rs`.
- **Commit:** Folded into `cdae530`.

### Authentication / Manual Action Gates

None.

## Confirmed Dependency Versions (from `cargo tree`)

| Crate | Version | Where |
|-------|---------|-------|
| ctrlc | 3.5.2 | `miner-cli` runtime |
| statrs | 0.17.1 | `miner-core` runtime |
| nix | 0.31.3 | `miner-cli` dev-only |

## Runtime async-deps audit

```text
$ cargo tree -p miner-core -e normal | grep -iE 'tokio|async-std|smol' | wc -l
0
```

FOUND-04 / D3-22 invariant held: `miner-core` is sync + std-only at runtime. (Dev-dep `jsonschema` pulls `reqwest` → `tokio`, which is irrelevant to the production binary's runtime dep graph.)

## Discoverable Phase 3 test names (via `cargo test --list`)

| Test fn | Source file |
|---------|-------------|
| `ljung_box_matches_statsmodels_golden` | `crates/miner-core/tests/scan_ljung_box.rs` |
| `twice_run_byte_identical_when_volatile_fields_masked` | `crates/miner-core/tests/scan_facade_determinism.rs` |
| `strict_with_gaps_emits_single_gap_aborted` | `crates/miner-core/tests/gap_policy.rs` |
| `continuous_only_partitions_and_inlines_manifest` | `crates/miner-core/tests/gap_policy.rs` |
| `strict_zero_gaps_emits_result_with_none_manifest` | `crates/miner-core/tests/gap_policy.rs` |
| `continuous_only_zero_gaps_emits_empty_manifest` | `crates/miner-core/tests/gap_policy.rs` |
| `dry_run_emits_dry_run_finding_only` (×2) | `crates/miner-core/tests/dry_run.rs` + `crates/miner-cli/tests/scan_subcommand_smoke.rs` |
| `scan_emits_run_start_result_run_end` | `crates/miner-cli/tests/scan_subcommand_smoke.rs` |
| `unknown_scan_emits_wireerror_exit_1` | `crates/miner-cli/tests/scan_subcommand_smoke.rs` |
| `invalid_params_emits_wireerror_exit_1` | `crates/miner-cli/tests/scan_subcommand_smoke.rs` |
| `exit_code_routing_zero_one_two` | `crates/miner-cli/tests/scan_subcommand_smoke.rs` |
| `scans_emits_one_line_per_registered_scan` | `crates/miner-cli/tests/scans_catalogue.rs` |
| `sigint_preserves_already_streamed_findings_and_exits_130` | `crates/miner-cli/tests/sigint_preserves_stream.rs` |

Plus two cfg-gated proptest names that become reachable when Plan 03-06 flips the `disabled_in_scaffold` gate:

- `look_ahead_safe_under_post_t_shuffle` (in `shuffled_future_regression.rs`)
- `never_silently_emits_on_hole_proptest` (in `gap_policy.rs`)

## Commits

| Task | Hash | Subject |
|------|------|---------|
| 1 | `8f36c24` | `chore(03-01): wire ctrlc + statrs + nix workspace deps for Phase 3 scan engine` |
| 2 | `0f95cbc` | `feat(03-01): scaffold miner-core scan + engine source files with signature-only bodies` |
| 3 | `cdae530` | `test(03-01): scaffold miner-cli scan_args + 9 Phase 3 integration test files` |

## Known Stubs

Every body landed in this plan is intentionally an `unimplemented!()` stub or `#[ignore]`'d test — this IS the plan's contract (Wave 0 scaffold, per VALIDATION.md plan-checker harness requirement). Plan 03-02..06 fill the bodies:

- `scan/registry.rs` `Registry::{new, register, get, default}` + `bootstrap()` — Plan 03-02.
- `scan/ljung_box/mod.rs` `LjungBoxScan::param_schema` + `run` — Plan 03-04.
- `scan/ljung_box/kernel.rs` `log_returns` / `biased_acf` / `ljung_box_q_and_p` — Plan 03-04.
- `engine/mod.rs` `run_one` — Plan 03-02..06 (split across plans; main body in 03-02).
- `engine/preflight.rs` `classify_param_error` + `parse_params_kv` — Plan 03-02.
- `engine/gap_policy.rs` `dispatch` — Plan 03-03.
- `engine/param_hash.rs` `param_hash` — Plan 03-02.
- `engine/framing.rs` `build_run_start` + `build_run_end` — Plan 03-02.
- `miner-cli/src/scan_args.rs` `parse_window` — Plan 03-05; `to_scan_request` — Plan 03-02.
- `tests/fixtures/mod.rs` `build_ar1_bar_frame` + `SyntheticCache` — Plan 03-06.
- All 13 integration test `#[test]` bodies and 2 proptest `cfg(disabled_in_scaffold)` modules — Plan 03-06 (filled when fixtures land).

These stubs are NOT bugs — they are the explicit Wave 0 deliverable, sized to be precisely consumed by Plan 03-02..06.

## Threat Model Disposition

- **T-03-01-SC (Tampering — crates.io deps)** — Mitigated. Package Legitimacy Audit per 03-RESEARCH confirmed `ctrlc 3.5.2`, `statrs 0.17.1`, `nix 0.31.3` are all legitimate, multi-year-history, Apache-2.0/MIT-licensed crates with public repos. No blocking-human checkpoint required.
- **T-03-01-02 (Tampering — Cargo.lock)** — Mitigated. `cargo tree -p miner-core -e normal` shows zero async creep (FOUND-04 gate); `grep -c preserve_order Cargo.lock` returns 0 (Pitfall 1 gate).
- **T-03-01-03 (DoS — `unimplemented!()` bodies panic on call)** — Accepted. Verified: `cargo test -p miner-core --test gap_policy` (and every other scaffolded test) runs ZERO tests (every scaffold test is `#[ignore]` or cfg-gated), so no `unimplemented!()` body is ever invoked by `cargo test` at this plan. Plan 03-02..06 will un-ignore tests AFTER filling the corresponding bodies, in the same wave.

## Self-Check: PASSED

- [x] `cargo build --workspace` exit 0
- [x] `cargo check -p miner-core` warning-free
- [x] `cargo check -p miner-cli` warning-free
- [x] `cargo test --workspace --no-run` exit 0
- [x] `cargo test --workspace` runs zero un-ignored tests in the new scaffolds (T-03-01-03 held)
- [x] 23/23 files in `files_modified` list exist at their declared paths
- [x] Three new deps confirmed installed at expected versions
- [x] `cargo tree -p miner-core -e normal | grep -iE 'tokio|async'` → empty (FOUND-04 gate)
- [x] `grep -c preserve_order Cargo.lock` → 0 (Pitfall 1 gate)
- [x] `Scan` trait dyn-compatibility regression test compiles
- [x] Every test fn named in VALIDATION's Per-Task Verification Map (except the two cfg-gated proptests) reachable via `cargo test --list`
- [x] Every commit hash recorded in this SUMMARY exists in `git log` on this worktree branch

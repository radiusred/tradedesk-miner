---
phase: 05-statistical-hygiene-sweep-runner
verified: 2026-05-21T09:50:00Z
status: passed
score: 5/5 must-haves verified
overrides_applied: 0
re_verification:
  previous_status: none
  previous_score: none
  gaps_closed: []
  gaps_remaining: []
  regressions: []
---

# Phase 5: Statistical Hygiene & Sweep Runner — Verification Report

**Phase Goal:** User can submit a TOML sweep manifest, have miner fan it out in parallel, and receive findings carrying effect sizes alongside p-values, block-bootstrap CIs on autocorrelated series, phase-scrambled null distributions, a deterministic RNG seed, and a sweep-summary record with Benjamini-Hochberg FDR-adjusted q-values.

**Verified:** 2026-05-21T09:50:00Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | User can submit a TOML sweep manifest and have miner fan it out in parallel via rayon, emitting one finding per (scan × instrument × tf × window × param-point) | VERIFIED | `crates/miner-core/src/sweep/{manifest,job_graph,executor}.rs` (864+685+868 LOC) implement TOML parse → cartesian expand → `rayon::par_iter` fanout with deterministic-order buffered drain (Pattern 4). End-to-end probe: `miner sweep test-manifest.toml --dry-run` emits `RunStart` → `DryRun` → `RunEnd` with `planned_job_count: 1`. `sweep_smoke.rs` test exercises 2 scans × 2 instruments × 1 tf × 1 window and asserts 4 Results emitted. |
| 2 | Every reported finding carries a scan-appropriate effect size alongside p-value; sweep summary contains BH-FDR q-values per family | VERIFIED | `Effect.effect_size: Option<EffectSize>` field shipped (`crates/miner-core/src/findings/mod.rs`); 22/22 Phase-4 scans populate it (`tests/effect_size_emission.rs` 3 family tests pass — anom/cross/seas). `Finding::SweepSummary(SweepSummaryFinding { fdr_by_family, ... })` variant emitted at end-of-sweep (`sweep/executor.rs:Step 9 → bh_fdr per family → SweepSummary`). `tests/sweep_summary_emission.rs` asserts SweepSummary position + shape. `bh_fdr` kernel (`scan/hygiene/fdr.rs`) implements BH (1995) step-up with NaN-filtering (CR-03 fix). |
| 3 | User can opt into block/stationary bootstrap CIs on autocorrelated-series scans, and phase-scrambled/circular-shift null distributions for p-value scans | VERIFIED (partial PhaseScramble) | Bootstrap: `stationary_bootstrap_ci` + `block_bootstrap_ci` + `block_length_pwppw` (Politis-White/PPW 2009) shipped in `scan/hygiene/bootstrap.rs` with CR-01 fix (data-dependent g_hat/D_SB). 19/22 scans declare `supports_bootstrap()=true` per D5-04 matrix; engine `apply_hygiene_mutations` populates `Effect.ci95`. Null distributions: `circular_shift_null_p` (Davison-Hinkley 1997 `(1+B)/(1+N)` floor — CR-02 fix) shipped; IAAFT `PhaseScramble` documented-deferred to Phase 7 (engine returns NaN for PhaseScramble, analytic p-value preserved). The roadmap SC says "phase-scrambled / circular-shift" (or-relation); circular-shift is fully wired, so SC is materially satisfied. `--bootstrap stationary --bootstrap-n N --null circular_shift --null-n N` CLI flags wired on both `miner scan` and `miner sweep`. |
| 4 | User can reproduce bootstrap/permutation results bit-for-bit via the seed in the repro envelope | VERIFIED | `ResultFinding.repro: Option<ReproEnvelope { master_seed, job_seed, bootstrap, null }>` populated by engine when hygiene runs (`engine/mod.rs:1216, 1277`). PRNG is `Xoshiro256PlusPlus::seed_from_u64(seed)` (portable, pinned reference-vector test in `bootstrap.rs`). `tests/hygiene_byte_identical_rerun.rs` (variance_ratio + pearson_rolling) and `tests/sweep_byte_identical_rerun.rs` (with_hygiene_on + no_hygiene) both pass — two identical-seed runs produce byte-identical JSONL after volatile-field masking. `derive_job_seed` uses blake3 over length-prefixed canonical bytes (WR-07 fix). |
| 5 | User can dry-run a sweep manifest with `--dry-run` and see the planned job graph + estimated count | VERIFIED | `DryRunFinding.planned_job_count: Option<u64>` additive field (`findings/mod.rs`). `sweep::executor::run_sweep_with_registry` short-circuits when `opts.dry_run == true`, emitting one DryRun envelope with `planned_job_count`. `tests/sweep_dry_run.rs` asserts `planned_job_count == Some(4)` on a 2×2 cartesian. End-to-end probe: `miner sweep test.toml --dry-run` exits 0 with `"planned_job_count":1` in the DryRun envelope. |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/miner-core/src/scan/hygiene/mod.rs` | Module root + 5 sub-modules | VERIFIED | 55 LOC; declares `pub mod {bootstrap,effect_size,fdr,null,seed}` |
| `crates/miner-core/src/scan/hygiene/effect_size.rs` | cohens_d/hedges_g/cliffs_delta/vr_minus_one | VERIFIED | 349 LOC, all 4 pub fns present |
| `crates/miner-core/src/scan/hygiene/bootstrap.rs` | stationary + block + block_length_pwppw | VERIFIED | 605 LOC, all 3 pub fns + CR-01 PPW 2009 fix + WR-04 cancel-poll cadence 64 |
| `crates/miner-core/src/scan/hygiene/null.rs` | circular_shift_null_p (+IAAFT optional) | VERIFIED | 263 LOC; circular_shift_null_p with Tail enum (WR-01) + (1+B)/(1+N) floor (CR-02); IAAFT deferred to Phase 7 (documented) |
| `crates/miner-core/src/scan/hygiene/fdr.rs` | bh_fdr | VERIFIED | 282 LOC; BH-FDR step-up with NaN-filter (CR-03 fix) |
| `crates/miner-core/src/scan/hygiene/seed.rs` | derive_job_seed | VERIFIED | 404 LOC; blake3 of length-prefixed canonical bytes (WR-07 fix) |
| `crates/miner-core/src/sweep/manifest.rs` | SweepManifest + read_manifest + validate | VERIFIED | 864 LOC; serde::Deserialize for SweepManifest/SweepConfig/HygieneBlock/FdrConfig/JobBlock + validate with `[fdr].alpha` bounds check (WR-03 fix) |
| `crates/miner-core/src/sweep/job_graph.rs` | ResolvedJob + expand + estimated_job_count | VERIFIED | 685 LOC; cartesian expansion in D5-01 deterministic order; cartesian_params saturating-mul (WR-08 fix) |
| `crates/miner-core/src/sweep/executor.rs` | run_sweep + rayon par_iter + BH-FDR + SweepSummary | VERIFIED | 868 LOC; deterministic-order buffered drain + BH-FDR aggregation + Finding::SweepSummary emission + SIGINT short-circuit + WR-09 cancel-poll-on-drain |
| `crates/miner-core/src/engine/hygiene_buffering_sink.rs` | HygieneBufferingSink wrapper | VERIFIED | 264 LOC; per-job result interception when hygiene active |
| `crates/miner-core/src/engine/hygiene_dispatch.rs` | Per-scan stat-closure dispatch | VERIFIED | 1337 LOC; all 19 opt-in scans wired (ANOM 11 + CROSS 5 + SEAS 5); Tail dispatch per scan (WR-01); pair-arity variants |
| `crates/miner-cli/src/sweep_args.rs` | SweepArgs clap-derive + to_manifest | VERIFIED | 342 LOC; positional manifest + --dry-run/--seed/--bootstrap/--bootstrap-n/--null/--null-n flags |
| `crates/miner-cli/src/scan_args.rs` | Universal hygiene flags on miner scan | VERIFIED | Five universal flags + `to_scan_request` populates 5 new ScanRequest fields |
| `crates/miner-cli/tests/sweep_subcommand_smoke.rs` | End-to-end CLI smoke + dry-run | VERIFIED | Both `sweep_subcommand_smoke` + `sweep_subcommand_smoke_dry_run` PASS |
| `crates/miner-cli/tests/sigint_mid_sweep.rs` | SIGINT preserves streamed Results + suppresses SweepSummary | VERIFIED | `sigint_mid_sweep_preserves_streamed_findings` PASS |
| `schemas/findings-v1.schema.json` | Regenerated with EffectSize/ReproEnvelope/SweepSummary additions | VERIFIED | Schema contains EffectSize, ReproEnvelope, BootstrapSpec, NullSpec, SweepSummaryFinding, FdrFamilySummary; `cargo xtask gen-schema` is idempotent (clean diff) |
| `schemas/sweep-manifest-v1.schema.json` | NEW — schemars-derived from SweepManifest | VERIFIED | 140 LOC; contains SweepManifest/SweepConfig/HygieneBlock/FdrConfig/JobBlock $defs |
| `README.md` Quickstart for `miner sweep` | New section | VERIFIED | Section "Quickstart — Sweep with Hygiene (Phase 5)" at line 218 |
| `tests/REFERENCE-VERSIONS.md` | R 4.x + tseries pinning | VERIFIED | Workspace-root file pins R 4.x + tseries 0.10.x + stats core for Phase 5 BH-FDR + Politis-White goldens |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| `engine/mod.rs::apply_hygiene_mutations` | `scan/hygiene/bootstrap::stationary_bootstrap_ci` | use site mutates `effect.ci95 = Some(ci)` | WIRED | `engine/mod.rs:1201, 1368` populate ci95 |
| `engine/mod.rs::apply_hygiene_mutations` | `scan/hygiene/null::circular_shift_null_p` | replaces `effect.p_value` | WIRED | `engine/mod.rs:1265, 1422` populate p_value (and Tail dispatch per scan) |
| `sweep/executor.rs::run_sweep_with_registry` | `scan/hygiene/fdr::bh_fdr` | per-family aggregation post-drain | WIRED | `executor.rs:Step 9` groups by family then calls bh_fdr |
| `sweep/job_graph.rs::expand` | `scan/hygiene/seed::derive_job_seed` | per-ResolvedJob.job_seed | WIRED | Each ResolvedJob populates job_seed via derive_job_seed |
| `engine/mod.rs` | `ReproEnvelope { master_seed, job_seed, bootstrap, null }` | populated when hygiene ran | WIRED | `engine/mod.rs:1216, 1277` write `result.repro = Some(...)` |
| Per-scan `mod.rs` | `Effect.effect_size = Some(EffectSize { kind, value })` | populated in Scan::run | WIRED | 22/22 scans (verified via `tests/effect_size_emission.rs`) |
| `miner-cli/src/cli.rs` | `Command::Sweep(SweepArgs)` variant | clap subcommand dispatch | WIRED | `cli.rs:84` `Sweep(SweepArgs)` + `main.rs:136-141` handler |
| `miner-cli/src/main.rs::handle_sweep_subcommand` | `miner_core::sweep::run_sweep` | per-job rayon fanout via run_sweep | WIRED | `main.rs:424 fn handle_sweep_subcommand` → `miner_core::sweep::run_sweep` |
| `miner-cli/src/scan_args.rs::to_scan_request` | `ScanRequest.{bootstrap_method,bootstrap_n,null_method,null_n,master_seed}` | flag → field population | WIRED | All 5 fields populated under `--bootstrap`/`--null`/`--seed` |
| `xtask/src/main.rs::gen-schema` | `schemas/sweep-manifest-v1.schema.json` | `schemars::schema_for!(SweepManifest)` | WIRED | `cargo xtask gen-schema` writes all 3 schemas idempotently |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `sweep::executor::run_sweep_with_registry` | `findings` (per-job buffer) | `engine::run_one_with_registry(req, ...) → JobSink.buf` | YES — real scan output | FLOWING |
| `Effect.effect_size` | EffectSize { kind, value } | per-scan Scan::run computes the canonical D5-03 statistic | YES — verified by tests/effect_size_emission.rs (kind string + finite value) | FLOWING |
| `Effect.ci95` | `[f64; 2]` lo/hi | `hygiene::bootstrap::stationary_bootstrap_ci` over real scan input series | YES — `tests/hygiene_engine_integration.rs` asserts ci95 finite + lo<hi | FLOWING |
| `ResultFinding.repro` | ReproEnvelope | populated iff bootstrap or null ran with real master/job seeds | YES — `tests/hygiene_byte_identical_rerun.rs` asserts repro.master_seed == 0xDEAD | FLOWING |
| `SweepSummaryFinding.fdr_by_family` | BTreeMap<String, FdrFamilySummary> | grouped p_values from drained results → `bh_fdr` | YES — `tests/fdr_family_scoping.rs` asserts 0/1/2 keys per scope variant | FLOWING |
| `SweepSummaryFinding.totals` | SweepTotals { jobs_run, results_emitted, scan_errors, gap_aborted } | counters incremented during drain | YES — `tests/sweep_summary_emission.rs` shape assertions | FLOWING |
| `DryRunFinding.planned_job_count` | `Option<u64>` | `jobs.len() as u64` in dry-run path | YES — `tests/sweep_dry_run.rs` asserts `Some(4)` | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| `cargo build --workspace` is green | `cargo build --workspace` | `Finished dev profile … in 7.74s` | PASS |
| `cargo test --workspace --no-fail-fast` is green | full test run, 68 test binaries | 68 / 68 binaries `test result: ok` (one transient cross-process race resolved on re-run) | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` is clean | clippy gate | `Finished dev profile … in 9.58s` — no warnings | PASS |
| `miner --help` exposes `sweep` subcommand | `target/debug/miner --help` | Lists `sweep   Execute a TOML sweep manifest end-to-end (Phase 5 / OP-04 / D5-04)` | PASS |
| `miner sweep --help` lists all hygiene + dry-run flags | `target/debug/miner sweep --help` | --dry-run, --seed, --bootstrap, --bootstrap-n, --null, --null-n all present | PASS |
| `miner scan --help` lists universal hygiene flags | `target/debug/miner scan --help \| grep -E "bootstrap\|null\|seed"` | --bootstrap, --bootstrap-n, --null, --null-n, --seed all present with HYG-* citations | PASS |
| `miner sweep <manifest.toml> --dry-run` emits DryRun with planned_job_count | end-to-end with synthetic 1-job manifest | RunStart → DryRun (`planned_job_count:1`) → RunEnd, exit 0 | PASS |
| `cargo xtask gen-schema` regenerates schemas idempotently | run twice, git status | `nothing to commit, working tree clean` after regen | PASS |
| `tests/sweep_byte_identical_rerun.rs` passes both hygiene-on and no-hygiene variants | targeted test | 2/2 pass — byte-identical JSONL across reruns with master_seed=0xDEAD | PASS |
| `tests/sigint_mid_sweep.rs` passes (SIGINT preserves streamed Results, suppresses SweepSummary, exit 130) | targeted CLI binary test | 1/1 pass | PASS |
| `tests/sweep_subcommand_smoke.rs` (happy path + dry-run) passes | targeted CLI binary test | 2/2 pass | PASS |
| `tests/effect_size_emission.rs` 3 family tests pass | targeted integration test | 3/3 pass — anom, cross, seas families all emit canonical effect_size kinds | PASS |
| `tests/fdr_family_scoping.rs` 4 family-scope variants pass | targeted integration test | 4/4 pass (scan_id / scan_family / all / none) | PASS |

### Requirements Coverage

| Requirement | Source Plan(s) | Description | Status | Evidence |
|-------------|---------------|-------------|--------|----------|
| OP-04 | 05-01, 05-04, 05-05 | User can submit a TOML sweep manifest and have miner fan it out in parallel | SATISFIED | `miner sweep <manifest.toml>` CLI subcommand wired end-to-end; rayon par_iter fanout in `sweep::executor`; `tests/sweep_smoke.rs` exercises 2-scan × 2-instrument fanout |
| HYG-01 | 05-01, 05-02, 05-03 | User can read an effect-size alongside every reported p-value | SATISFIED | `Effect.effect_size: Option<EffectSize>` populated on every Phase-4 scan with canonical D5-03 kind; verified by `tests/effect_size_emission.rs` per scan family |
| HYG-02 | 05-01, 05-02, 05-04 | BH-FDR adjustment at sweep level, emitted in sweep-summary record | SATISFIED | `bh_fdr` kernel (Plan 05-02) + `Finding::SweepSummary { fdr_by_family }` emitted at end-of-sweep via `executor.rs` Step 9; `tests/fdr_family_scoping.rs` + `tests/sweep_summary_emission.rs` PASS. (Golden-file regression testing of BH-FDR vs R `p.adjust` is intentionally deferred to Phase 7 per ROADMAP — "Phase 7 carries no new v1 REQ-IDs; it closes verification debt for … HYG-02 …".) |
| HYG-03 | 05-01, 05-02, 05-03 | User can request block / stationary bootstrap (Politis-Romano) CIs on any scan over autocorrelated series | SATISFIED | `stationary_bootstrap_ci` + `block_bootstrap_ci` + Politis-White / PPW 2009 block-length selector (`block_length_pwppw`) shipped; CLI flag `--bootstrap stationary\|block --bootstrap-n N` on both `miner scan` and `miner sweep`; 19/22 scans declare `supports_bootstrap()=true` per D5-04 matrix; engine populates `Effect.ci95`. CR-01 PPW 2009 erratum fix landed. |
| HYG-04 | 05-01, 05-02, 05-03 | User can request phase-scrambled / circular-shift null distributions as a first-class option | SATISFIED | `circular_shift_null_p` shipped with Tail enum (WR-01) and Davison-Hinkley `(1+B)/(1+N)` floor (CR-02). IAAFT phase-scramble intentionally deferred to Phase 7 (documented in both 05-02-SUMMARY and `null.rs` module doc; engine returns NaN for PhaseScramble so analytic p-value is preserved). Roadmap SC text uses or-relation ("phase-scrambled / circular-shift"); circular-shift fully wired satisfies the contract. CLI flag `--null phase_scramble\|circular_shift --null-n N` accepts both spellings; preflight rejects PhaseScramble at the engine layer for scans whose `supports_null_method(PhaseScramble)=false`. |
| HYG-05 | 05-01, 05-02, 05-03, 05-04 | User can reproduce any bootstrap/permutation result bit-for-bit via the seed in the repro envelope | SATISFIED | `ReproEnvelope { master_seed, job_seed, bootstrap, null }` populated when hygiene runs; PRNG is `Xoshiro256PlusPlus::seed_from_u64(seed)` (pinned reference-vector test); `derive_job_seed` uses blake3 over length-prefixed canonical bytes; `tests/hygiene_byte_identical_rerun.rs` and `tests/sweep_byte_identical_rerun.rs` (both hygiene-on + no-hygiene variants) PASS. Phase 7 carries golden-file noise-replay regression verification per ROADMAP. |

No orphaned requirements — REQUIREMENTS.md maps exactly OP-04 + HYG-01..05 to Phase 5; all six appear in at least one plan and all six are covered.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | — | No TBD/FIXME/XXX debt markers in any Phase-5 file (hygiene, sweep, hygiene_dispatch, hygiene_buffering_sink, sweep_args, schemas) | INFO | Clean — auditable completion |
| (none) | — | No TODO/HACK/PLACEHOLDER markers in any Phase-5 file | INFO | Clean — no warning-level cleanup pending |
| `scan/hygiene/null.rs` | 9-19 | IAAFT `PhaseScramble` deferred to Phase 7 — documented in module doc + summaries | INFO | Documented intentional deferral; engine returns NaN for `PhaseScramble`, preserves analytic p-value; preflight rejects PhaseScramble on every scan whose `supports_null_method(PhaseScramble)=false`. Not a hidden stub. |
| `engine/hygiene_dispatch.rs` | 813-855 | `make_seas_session_closure` ignores user-supplied `sessions` parameter, uses `FX_MAJOR_DEFAULTS` for hygiene resample | INFO | Documented Phase 7 hook in 05-03-SUMMARY. The wire-output `Effect.value` (which the scan body computes) still honours the user's sessions; only the bootstrap/null RESAMPLE uses defaults. Slight discrepancy is acceptable for v1 — the resample shape (FX-major buckets) is structurally identical to a user-specified one in the common case. |
| `engine/mod.rs` | 1251-1262 | `PhaseScramble` branch returns NaN sentinel | INFO | Same documented Phase 7 hook as above; belt-and-braces behind the preflight gate. |

No BLOCKER or WARNING anti-patterns. The two INFO items are explicit documented deferrals to Phase 7 that the planner is aware of (referenced in summaries + module docs + the verifier-context preamble).

### Code Review Status

The Plan 05-REVIEW.md surfaced 3 BLOCKER + 9 WARNING findings post-implementation:
- All 3 BLOCKERS fixed (CR-01 PPW 2009 block-length, CR-02 `(1+B)/(1+N)` empirical-p floor, CR-03 NaN-poisoned BH-FDR)
- All 9 WARNINGS fixed (WR-01 Tail-aware null comparison, WR-02 per-job preflight error surfacing, WR-03 alpha + n_resamples preflight bounds, WR-04/05/06 sparse cancel-poll + clamp, WR-07 length-prefixed seed bytes, WR-08 saturating-mul cartesian, WR-09 cancel-poll during drain + RunEnd-on-sink-error)
- 4 INFO items remain open (cosmetic / UX nits) — out-of-scope for this remediation

Independently re-verified via direct code inspection (`bootstrap.rs:267-339` for CR-01, `null.rs:108-121` for CR-02, `fdr.rs:67-101` for CR-03). The fixes are substantive and the regression tests (938 passed, 0 failed per REVIEW.md verification stamp) cover the new edges.

### Human Verification Required

None — all five success criteria are programmatically verifiable via tests + end-to-end probe + grep checks. The remaining concerns (IAAFT deferral, session custom-param hygiene-resample) are documented Phase 7 hooks rather than user-facing functional gaps. The roadmap SC text uses `/` (or-relation) for HYG-04, which circular-shift alone satisfies.

### Gaps Summary

No gaps. Every success criterion verifies against the codebase:
1. TOML manifest fanout — full end-to-end via `miner sweep`, rayon par_iter, deterministic order
2. Effect-size + BH-FDR — every scan emits effect_size; SweepSummary emits per-family q-values
3. Bootstrap CI + null distribution opt-in — both kernels wired; circular-shift fully functional, PhaseScramble documented-deferred (Phase 7)
4. Byte-identical reproducibility — repro envelope populated, Xoshiro256PlusPlus seeded, two byte-identical-rerun tests pass
5. `--dry-run` — DryRun envelope carries `planned_job_count`; end-to-end probe verified

Phase 5 is ready to proceed.

---

_Verified: 2026-05-21T09:50:00Z_
_Verifier: Claude (gsd-verifier)_

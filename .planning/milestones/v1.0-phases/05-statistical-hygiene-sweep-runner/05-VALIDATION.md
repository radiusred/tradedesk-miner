---
phase: 5
slug: statistical-hygiene-sweep-runner
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-20
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: derived from `05-RESEARCH.md § Validation Architecture` (lines 1008–1077).

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (`#[test]`) + `proptest = "1.11"` (already in `miner-core/Cargo.toml` dev-deps) + `insta = "1.47"` (already wired) |
| **Config file** | None — `cargo test` runs `[lib]` + `tests/*.rs` integration tests directly |
| **Quick run command** | `cargo test -p miner-core --lib --quiet` |
| **Full suite command** | `cargo test --workspace --all-features` |
| **Estimated runtime** | ~5–15s quick · ~60–180s full |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p miner-core --lib --quiet` — covers the kernel unit tests (effect_size, bootstrap, null, fdr, seed, manifest)
- **After every plan wave:** Run `cargo test --workspace` — adds the integration tests (sweep_smoke, sweep_dry_run, sweep_summary_emission, sweep_byte_identical_rerun, sigint_mid_sweep) and the CLI binary tests
- **Before `/gsd:verify-work`:** Full suite must be green, PLUS `cargo xtask gen-schema && git diff --exit-code schemas/` (schema-additive enforcement)
- **Max feedback latency:** ~15 seconds for quick loop; ~180 seconds for full

---

## Per-Task Verification Map

> Populated by `gsd-planner` as PLAN.md task IDs become known. Each row links a `Task ID` →
> Requirement → Automated Command. Test commands are mirrored from `05-RESEARCH.md` so
> the planner can copy directly into per-task `<acceptance_criteria>` blocks.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| TBD | TBD | — | OP-04 | T-5-V5 | Reject malicious TOML manifests at preflight (V5 input validation) | unit | `cargo test -p miner-core --lib sweep::manifest::tests` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | OP-04 | — | — | unit / proptest | `cargo test -p miner-core --lib sweep::job_graph::tests` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | OP-04 | — | — | integration | `cargo test -p miner-core --test sweep_dry_run` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | OP-04 | T-5-V5 | Reject manifests where estimated jobs exceed `[sweep].max_jobs` | unit | `cargo test -p miner-core --lib sweep::manifest::tests::sweep_too_large` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | OP-04 | — | — | integration | `cargo test -p miner-core --test sweep_smoke` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | OP-04 | — | Preserve already-streamed findings on SIGINT; exit 130 | integration (CLI) | `cargo test -p miner-cli --test sigint_mid_sweep` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-01 | — | — | unit | `cargo test -p miner-core --lib scan::hygiene::effect_size::tests` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-01 | — | — | integration | `cargo test -p miner-core --test effect_size_emission` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-01 | — | — | unit | `cargo test -p miner-core --lib findings::tests::effect_size_round_trip` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-02 | — | — | unit | `cargo test -p miner-core --lib scan::hygiene::fdr::tests::bh_fdr_canonical_5` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-02 | — | — | unit / proptest | `cargo test -p miner-core --lib scan::hygiene::fdr::tests::bh_fdr_rank_order_proptest` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-02 | — | — | integration | `cargo test -p miner-core --test sweep_summary_emission` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-02 | — | — | integration | `cargo test -p miner-core --test fdr_family_scoping` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-03 | — | — | proptest | `cargo test -p miner-core --lib scan::hygiene::bootstrap::tests::stationary_iid_coverage` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-03 | — | — | unit | `cargo test -p miner-core --lib scan::hygiene::bootstrap::tests::deterministic_for_seed` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-03 | — | — | golden | `cargo test -p miner-core --test bootstrap_block_length_golden -- --ignored` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-04 | — | — | proptest | `cargo test -p miner-core --lib scan::hygiene::null::tests::circular_shift_uniform_under_null` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-04 | — | — | unit | `cargo test -p miner-core --lib scan::hygiene::null::tests::iaaft_preserves_spectrum` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-04 | — | — | unit | `cargo test -p miner-core --lib scan::hygiene::null::tests::iaaft_preserves_marginal` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-05 | — | — | unit | `cargo test -p miner-core --lib scan::hygiene::seed::tests::derive_job_seed_deterministic` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-05 | — | — | integration | `cargo test -p miner-core --test sweep_byte_identical_rerun` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-05 | — | — | unit | `cargo test -p miner-core --lib findings::tests::repro_envelope_population_rule` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | HYG-05 | — | — | unit | `cargo test -p miner-core --lib scan::hygiene::bootstrap::tests::xoshiro_reference_vector` | ❌ Wave 0 | ⬜ pending |
| TBD | TBD | — | Cross-cutting | — | No `println!` / `eprintln!` outside sink + logging adapter | CI gate | `cargo clippy --workspace --all-targets -- -D warnings` | ✅ existing | ⬜ pending |
| TBD | TBD | — | Cross-cutting | — | `Scan` trait stays dyn-compatible after `supports_bootstrap()` / `supports_null_method()` added | compile-only | `cargo test -p miner-core --lib scan::tests::scan_trait_object_safe` | ❌ Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

> All Wave 0 work is greenfield additions; no existing Phase 1–4 tests need to change.

- [ ] `crates/miner-core/src/scan/hygiene/{mod,effect_size,bootstrap,null,fdr,seed}.rs` — new kernel modules (six files NEW)
- [ ] `crates/miner-core/src/sweep/{mod,manifest,job_graph,executor}.rs` — new sweep runner module (four files NEW)
- [ ] `crates/miner-core/tests/sweep_smoke.rs` — end-to-end sweep integration test
- [ ] `crates/miner-core/tests/sweep_dry_run.rs` — dry-run integration test
- [ ] `crates/miner-core/tests/sweep_summary_emission.rs` — `Finding::SweepSummary` emission test
- [ ] `crates/miner-core/tests/sweep_byte_identical_rerun.rs` — bit-for-bit reproducibility regression
- [ ] `crates/miner-core/tests/fdr_family_scoping.rs` — `[fdr].family` enum coverage
- [ ] `crates/miner-core/tests/effect_size_emission.rs` — every scan emits a non-null `effect.effect_size`
- [ ] `crates/miner-core/tests/bootstrap_block_length_golden.rs` — R `tseries::b.star` golden (`#[ignore]` until provenance available)
- [ ] `crates/miner-cli/src/sweep_args.rs` — new `SweepArgs` clap-derive struct
- [ ] `crates/miner-cli/tests/sigint_mid_sweep.rs` — CLI binary SIGINT-during-sweep integration test
- [ ] `tests/REFERENCE-VERSIONS.md` — extend with `R 4.x` + `tseries`/`stats` pins for BH-FDR + block-length goldens
- [ ] Workspace `Cargo.toml` — add `rand = "0.8"`, `rand_xoshiro = "0.6"`, `toml = "0.8"` to `[workspace.dependencies]`; optionally `realfft = "3"`
- [ ] `crates/miner-core/Cargo.toml` — pull the four new deps in via workspace inheritance
- [ ] `schemas/sweep-manifest-v1.schema.json` — NEW (optional companion artifact via `cargo xtask gen-schema`)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| R golden vectors for `bh_fdr` and Politis-White-Patton-2009 block-length selector | HYG-02, HYG-03 | Requires R 4.x toolchain with `tseries::b.star` + `stats::p.adjust` on a developer machine; not portable to CI without an R container | Run `Rscript scripts/gen_fdr_goldens.R` and `Rscript scripts/gen_block_length_goldens.R` after pinning `tests/REFERENCE-VERSIONS.md`; commit emitted JSON fixtures under `crates/miner-core/tests/fixtures/` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s (quick loop)
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

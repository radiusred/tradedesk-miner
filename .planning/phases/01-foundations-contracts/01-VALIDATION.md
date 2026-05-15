---
phase: 01
slug: foundations-contracts
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-15
---

# Phase 01 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (built-in) + optional `cargo-nextest` 0.9+ for parallel local runs |
| **Config file** | none — workspace `Cargo.toml` test layout is conventional |
| **Quick run command** | `cargo test -p miner-core` |
| **Full suite command** | `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` |
| **Estimated runtime** | ~10–30 seconds (greenfield workspace, no fixture I/O) |

See `01-RESEARCH.md §Validation Architecture` for the full per-requirement test map.

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p <crate-being-touched>` (or `cargo test --workspace` if cross-crate)
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
- **Before `/gsd:verify-work`:** Full CI gates from FOUND-01..05 / OUT-01..03 must be green (build + clippy + tokio-tree grep + schema-sync diff)
- **Max feedback latency:** 30 seconds (target — workspace is small in Phase 1)

---

## Per-Task Verification Map

> Populated by the planner from `<verification>` blocks in each `*-PLAN.md`. Do NOT edit by hand — re-run `/gsd:plan-phase 1 --gaps` if rows are missing.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| _(planner fills from PLAN.md tasks)_ | | | | | | | | | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

All Phase 1 tests are net-new (greenfield workspace). The planner MUST schedule the following test infrastructure in Wave 0 (or earlier) before any task with `<verification>` claims they exist:

- [ ] `crates/miner-core/tests/findings_envelope.rs` — round-trip + schema-validation tests for every envelope `kind`
- [ ] `crates/miner-core/tests/config_precedence.rs` — CLI > env > TOML > default for `cache_root`, `bar_cache_root`, `output`
- [ ] `crates/miner-cli/tests/cli_streams.rs` — integration test asserting stdout = JSONL, stderr = tracing
- [ ] `xtask/src/gen_schema.rs` — `cargo run -p xtask -- gen-schema` writes `schemas/findings-v1.schema.json`
- [ ] `clippy.toml` (workspace root) — `disallowed-macros` config for `println!` / `eprintln!`
- [ ] `.github/workflows/ci.yml` — four mandatory CI gates from D-21
- [ ] Optional: `Cargo.toml` workspace `dev-dependencies` adds `jsonschema` + (if planner picks) `insta` for snapshot tests

*Sentinel: any task whose `<automated>` block references a path under `crates/`, `tests/`, `xtask/`, `schemas/`, or `.github/` MUST list one of the rows above (or an explicit predecessor task) in `<depends_on>`.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| *(none expected — Phase 1 is contracts-only and fully automatable)* | | | |

*Phase 1 has zero manual verifications by design — every contract is CI-enforceable.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

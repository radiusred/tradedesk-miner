---
phase: 4
slug: scan-catalogue-anom-cross-seas
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-19
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: RESEARCH.md §Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust 2024) + `cargo nextest` for fast feedback |
| **Config file** | `Cargo.toml` workspace + per-crate `[dev-dependencies]` |
| **Quick run command** | `cargo nextest run -p miner-core --no-fail-fast` |
| **Full suite command** | `cargo test --workspace --all-features` |
| **Estimated runtime** | ~30s quick, ~90s full (per Phase 3 baseline) |

---

## Sampling Rate

- **After every task commit:** Run `cargo nextest run -p miner-core <test_filter>` (scoped to changed scan module)
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite green + `cargo run -p xtask -- gen-schema` produces no schema-version bump (or documented additive diff)
- **Max feedback latency:** ~30s

---

## Per-Task Verification Map

> Populated by planner during plan-phase. Each task lists: scan_id, requirement, file paths created/modified, automated verify command, fixture/golden references.

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | Status |
|---------|------|------|-------------|-----------|-------------------|--------|
| {filled by planner} | | | | | | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/miner-core/tests/goldens/` directory created
- [ ] `crates/miner-core/tests/REFERENCE-VERSIONS.md` pins statsmodels + scipy versions
- [ ] `scripts/gen-goldens.py` (or equivalent) committed for reproducible golden regeneration
- [ ] `crates/miner-core/Cargo.toml` adds `ndarray`, `ndarray-stats`, `nalgebra` per RESEARCH.md §Dependency-add audit

---

## Sampling Dimensions (from RESEARCH.md §Validation Architecture)

### Coverage
Each of the 22 scans gets:
- One happy-path integration test against deterministic synthetic data
- One checked-in statsmodels/scipy golden where a Python reference exists
- Goldens stored at `crates/miner-core/tests/goldens/<scan_id>.jsonl`
- Reference versions pinned in `crates/miner-core/tests/REFERENCE-VERSIONS.md`

### Edge
- `N=0` input → `Finding::ScanError` with `InsufficientData` code
- `N=1` input → most scans error; summary-stats scan emits trivial finding
- All-zero returns → variance-zero handling for normalised stats (correlations, t-stats)
- All-NaN input → must `Finding::ScanError`, NOT propagate NaN into envelope
- Single timestamp gap mid-window (rolling scans)
- CROSS legs with zero overlap → `Finding::GapAborted` (strict) or `InsufficientData` (continuous_only)

### Adversarial
- **Shuffled-future regression** (D3-09 pattern from Phase 3, extended to every rolling/causal scan): permute the tail of the input; rolling/causal output for any timestamp `t` must be byte-identical to the un-permuted run for that prefix.
- **Zero-variance leg in CROSS** → correlation/OLS undefined; must `Finding::ScanError`, not emit NaN.
- **Cointegrating residual with near-zero half-life** (CROSS-05 numerical degenerate case for OU fit).
- **Trading-session boundary edges** (SEAS-03): bars exactly at session-boundary timestamp must bucket deterministically.

### Cross-validation
- **Byte-identical re-run** (D3-23, every scan) — modulo `run_id` + clock-read fields.
- **CLI/MCP/HTTP parity** (Phase 6 will enforce). Phase 4 emits findings whose only run-id/clock-read fields differ across surfaces; the rest must be byte-identical.
- **Schema regen + diff check** (per D4-01/D4-03). `cargo run -p xtask -- gen-schema` produces a diff that is classified as additive (or fallback to D4-03-ALT documented).

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Quickstart README example produces expected JSONL | Success Criterion #4 | Touches doc consistency; integration covers stat correctness | Follow README quickstart for one ANOM/CROSS/SEAS scan; eyeball envelope shape consistency |

---

## Validation Sign-Off

- [ ] All 22 scan tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers goldens directory + REFERENCE-VERSIONS.md + dep additions
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s for quick, < 90s for full
- [ ] `nyquist_compliant: true` set in frontmatter once planner fills the per-task map

**Approval:** pending

# Plan 04-13 Summary — CI clippy::pedantic Unblock

**Status:** Complete. All 5 CI gates green locally; pending GitHub Actions confirmation on push.

**Goal:** Resolve all `clippy::pedantic` errors blocking CI Gate 2
(`cargo clippy --workspace --all-targets -- -D warnings`) so Phase 5
can begin with a green-CI baseline.

**Goal achieved:** Yes. CI gate 2 has been red since Phase 4
implementation began; this plan is the first commit series to make it
green. STATE.md "deferred to Phase 7 hardening" decision reversed for
miner-core lib + binaries + tests.

## What landed — per-task breakdown

The plan documented 88 lib-only errors split into 6 atomic-per-category
tasks plus verification. The actual full audit (visible only once lib
compiled cleanly) was **larger**: with miner-core lib green, clippy
proceeded to compile-and-check `#[cfg(test)] mod tests` blocks,
integration tests, and `miner-cli` — exposing ~120 additional errors
that the plan's "Phase 7 owns deny-warnings audit across binaries and
tests" carve-out had not anticipated. Plan 04-13 closed that audit too.

| Task | Commit | Lint(s) | Errors fixed | Files touched |
|------|--------|---------|--------------|---------------|
| Plan + ROADMAP | `2d89701` | (docs) | — | 2 |
| 1 | `d4a0672` | `doc_markdown` | 56 | 65 |
| 2 | `e3c9005` | `unwrap_or_default` (was `or_insert_with`) | 9 | 1 |
| 3 | `76791c0` | `manual_let_else` | 4 | 2 |
| 4 | `88c8bed` | `similar_names` + `many_single_char_names` | 7 | 5 |
| 5 | `6437f42` | `too_many_lines` + `cast_precision_loss` + `cast_possible_truncation` | 5 | 4 |
| 6 | `2815d01` | originally-planned mechanicals (9) + full workspace audit (~110) | ~120 | 64 |
| 6b | `c67fd68` | (chore) untrack `.claude/` developer state | — | 9 |
| 7 | (this) | summary + STATE.md + ROADMAP plan tick | — | 3 |

Net workspace error count over Plan 04-13: **88 → 0** (lib-only inventory)
or **~200 → 0** (full workspace audit). CI Gate 2 transitions from RED
to GREEN.

## `#[allow]` inventory — every suppression carries a `reason = "..."`

Per the existing `clippy.toml` discipline, every new `#[allow]` includes
a `reason` string. The suppressions cluster around five intentional
patterns:

1. **Closed-form regression bodies that exceed clippy's 100-line cap**
   — splitting would obscure the formula's correspondence to the cited
   literature.
   - `crates/miner-core/src/scan/anom/arch_lm/kernel.rs:arch_lm_test`
     (`too_many_lines`, "closed-form Engle 1982 ARCH-LM derivation")
   - `crates/miner-core/src/scan/seas/anova_kw/mod.rs:Scan::run`
     (`too_many_lines`, "ANOVA + Kruskal-Wallis bundled `Scan::run`
     envelope construction")

2. **Sample-size `usize → f64` casts in stats kernels** — `n` cannot
   physically exceed `2^53 ≈ 9e15` bars in any realistic OHLCV series.
   - `crates/miner-core/src/scan/anom/kpss/kernel.rs:kpss_statistic`
     (`cast_precision_loss`)

3. **CLI-bounded lag indices** — `u64 → usize` cast where the value is
   already validated.
   - `crates/miner-core/src/scan/cross/lead_lag/kernel.rs:150`
     (`cast_possible_truncation`)

4. **Canonical statistical notation** — `y / k / n / t / dof` (Said-Dickey
   1984), `n / x / y / β / e` (Engle 1982), `xt / xtx / xty / xtx_inv`
   (OLS normal equations). Renaming would diverge from the literature.
   - `crates/miner-core/src/scan/anom/adf/kernel.rs:fit_adf_regression`
     (`many_single_char_names`)
   - `crates/miner-core/src/scan/anom/arch_lm/kernel.rs:arch_lm_test`
     (`many_single_char_names`, `similar_names`)
   - `crates/miner-core/src/scan/anom/kpss/kernel.rs:detrend_with_trend`
     (`similar_names`)

5. **Internal-facade pass-by-value convention** —
   `Arc<AtomicBool>` is cheap-to-clone and consistent with sibling
   facade functions (`run_one`, `run_one_with_registry`,
   `dispatch_single_arity_body`).
   - `crates/miner-core/src/engine/mod.rs:dispatch_pair_arity_body`
     (`needless_pass_by_value`)

## Crate-level `#![cfg_attr(test, allow(...))]` — lib.rs

`crates/miner-core/src/lib.rs` gained a `#![cfg_attr(test, allow(...))]`
block scoping these lints to test-only compilation:

```rust
#![cfg_attr(test, allow(
    // Numeric casts in synthetic fixture generators — bar indexes,
    // timestamp arithmetic, golden bit comparisons.
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    // Test ergonomics.
    clippy::comparison_to_empty,
    clippy::useless_conversion,
    clippy::unnecessary_fallible_conversions,
    clippy::needless_range_loop,
    clippy::manual_memcpy,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::doc_lazy_continuation,
    clippy::len_zero,
))]
```

The same lints in PRODUCTION (non-test) code stay under the full
pedantic bar.

## Per-integration-test-file `#![allow(...)]`

Integration test files are SEPARATE crates from `miner-core` lib, so
the lib.rs `#![cfg_attr(test, ...)]` does not propagate to them. The
following integration test files gained their own crate-level `#![allow]`:

- `crates/miner-core/tests/byte_identical_rerun.rs` — `type_complexity`,
  `needless_pass_by_value`, `items_after_statements` (private generic
  test helpers).
- `crates/miner-core/tests/scan_seas_session.rs`,
  `scan_seas_event_window.rs`, `scan_seas_day_of_week.rs`,
  `scan_seas_anova_kruskal.rs`, `scan_seas_eom_som.rs`,
  `scan_seas_hour_of_day.rs` — cast lints on synthetic 15m bar generators.
- `crates/miner-core/tests/scan_lead_lag.rs`,
  `scan_returns_profile.rs` — `float_cmp` on golden comparisons.
- `crates/miner-core/tests/scan_engle_granger.rs` — `too_many_lines` on
  multi-step golden setup.
- `crates/miner-core/tests/gap_intersect_cross.rs` —
  `match_wildcard_for_single_variants` on exhaustive enum matches.
- `crates/miner-core/tests/two_leg_facade.rs` — `useless_vec` on
  intentionally-explicit Vec construction.
- `crates/miner-cli/src/scan_args.rs` — `doc_lazy_continuation` on a
  multi-line docstring where `+ ` at col 5 starts a markdown list and
  continuation lines are intentionally unindented prose.

## Verification — all 5 CI gates green locally

```
=== Gate 1 — cargo build --workspace --all-targets ===
    Finished `dev` profile [unoptimized + debuginfo] target(s)

=== Gate 2 — cargo clippy --workspace --all-targets -- -D warnings ===
    Finished `dev` profile [unoptimized + debuginfo] target(s)
    (zero errors, zero warnings — THE assertion of Plan 04-13)

=== fmt — cargo fmt --all -- --check ===
    OK: clean

=== Tests — cargo test --workspace --no-fail-fast ===
    passed=796 failed=0 ignored=3
    (Phase 4 baseline preserved; the 3 ignored are Plan 04-11's
    pinned-venv goldens awaiting the user setup recipe documented
    in goldens/REFERENCE-VERSIONS.md.)

=== Gate 3 — tokio-free miner-core ===
    OK: zero async-runtime deps (FOUND-04)

=== Gate 4 — schema sync ===
    cargo run -p xtask -- gen-schema → no diff
    OK: schemars-derived schemas/findings-v1.schema.json unchanged
    (this plan touched zero schemars-derived types)
```

## Deviations from the documented plan

1. **Scope expanded from 88 → ~200 errors.** The plan inventoried errors
   reachable while miner-core lib was broken (clippy stops on first
   crate failure). Once lib compiled cleanly, the test-module and
   integration-test and miner-cli errors became visible. Task 6's
   commit grew accordingly. Net effect: CI Gate 2 is now green for the
   ENTIRE workspace (not just miner-core lib).

2. **`cargo clippy --fix` used for mechanical lints.** Tasks 1, 2, 3,
   and parts of 6 were applied via `cargo clippy --fix --allow-dirty
   --workspace --all-targets -- -A clippy::all -W clippy::<lint>`,
   scoped to one lint at a time so the atomic-per-category commit
   discipline survived. Manual edits filled the gaps where the
   suggestion was not machine-applicable (let-else conversions where
   clippy doesn't auto-infer types, the `#[allow]` placement, the
   judgment-call renames in Task 4).

3. **Follow-up `chore` commit** (`c67fd68`) untracked the
   `.claude/worktrees/agent-*` embedded git repositories and the
   `.claude/scheduled_tasks.lock` / `.claude/settings.local.json` files
   that accidentally landed in Task 6's `git add .` (developer-local
   state that should never have been staged). Added `.claude/` to
   `.gitignore`.

## STATE.md amendment

The STATE.md `decisions` list entry that read:

> "Plan 04-11: cargo clippy -D warnings workspace cleanup deferred to
> Phase 7 hardening; only 3 in-scope LN_2 lints in drawdown/kernel.rs
> fixed."

is amended (Task 7 commit) with the postscript:

> "Plan 04-13 (2026-05-20) reversed this deferral for the entire
> workspace (miner-core lib + tests + miner-cli): all clippy::pedantic
> errors resolved, CI gate 2 now green. Phase 7 retains the deny-warnings
> audit responsibility for any NEW code added in Phases 5–6."

## What this unblocks

Phase 5 (Statistical Hygiene & Sweep Runner) can now proceed with a
green-CI baseline. The per-task atomic-commit discipline GSD relies on
is safe again — every Phase 5 commit gets meaningful CI signal.

## What Phase 7 still owns (carry-forward)

- `cargo deny` advisory + license check sweep.
- `cargo audit` RustSec advisory sweep.
- Action SHA-pinning in `.github/workflows/ci.yml` (currently
  `@v4` / `@stable` major-version tags per CI comment).
- Re-auditing any NEW code added by Phases 5–6 against the same
  `clippy::pedantic = warn` + `-D warnings` bar.
- Removing the Task 6 file-level `#![allow(...)]` blocks if individual
  patterns can be refactored away (most cannot — the suppressions
  encode genuine domain constraints).

## Lessons for future plans

1. **Plan inventories scoped to "current visible clippy errors" are
   incomplete.** If clippy can't compile the lib, it doesn't reach the
   tests. Future plans should run `cargo build --workspace --all-targets`
   FIRST to confirm a clean build, then inventory clippy errors.

2. **Per-task TDD verification should include `cargo clippy -D warnings`
   for any task touching `miner-core` source.** If Phase 4 had this
   gate, the 88 lib errors would have been caught at each per-task
   commit rather than accumulating across the whole phase.

3. **`cargo clippy --fix` is the canonical tool for mechanical
   pedantic cleanups** — scope it per-lint via
   `-A clippy::all -W clippy::<lint>` to preserve atomic-per-category
   commits.

---

*Plan 04-13 completed 2026-05-20. CI gate 2 GREEN; Phase 5 unblocked.*

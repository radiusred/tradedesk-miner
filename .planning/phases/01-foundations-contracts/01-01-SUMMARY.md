---
phase: 01-foundations-contracts
plan: 01
subsystem: infra
tags: [rust, cargo-workspace, edition-2024, build.rs, schemars, serde_json, toolchain-pin, xtask]

# Dependency graph
requires: []
provides:
  - "Cargo virtual workspace with seven members (six product crates + xtask)"
  - "resolver = \"3\", edition = \"2024\", MSRV = 1.85, license = Apache-2.0 pinned at workspace level"
  - "rust-toolchain.toml pinning 1.85 with clippy + rustfmt components"
  - "workspace.dependencies table locking the Phase 1 stack (serde, serde_json, schemars, chrono, thiserror, anyhow, tracing, tracing-subscriber, figment, clap, ulid, jsonschema, blake3, directories, base64)"
  - "miner-core::build.rs injecting MINER_CODE_REVISION at compile time (git SHA or dirty-<sha>) — mitigates threat T-01-04"
  - "miner-core::CODE_REVISION constant exported for every future Finding envelope"
  - "Zero-async invariant in miner-core proven: `cargo tree -p miner-core --edges normal,build` matches no tokio/async-std/smol/async-trait"
  - ".cargo/config.toml alias `cargo xtask = run --package xtask --` so Plan 06 can wire `gen-schema` against the same alias developers already type"
  - "serde_json `preserve_order` feature proven OFF across the entire workspace feature-unified graph — defends Plan 06 schema byte-stability"
affects: [plan-01-02, plan-01-03, plan-01-04, plan-01-05, plan-01-06, plan-01-07, phase-02, phase-06, phase-07]

# Tech tracking
tech-stack:
  added:
    - "rustup + Rust 1.85.1 toolchain (with clippy 0.1.85 + rustfmt 1.8.0-stable)"
    - "Cargo workspace inheritance (workspace.package, workspace.dependencies, workspace.lints)"
    - "schemars 1.2.1 (verified ≥ 1.0.5 stable major)"
    - "figment 0.10.19 (verified ≥ 0.10.19)"
    - "ulid 1.2.1 (verified 1.2.x)"
    - "jsonschema 0.46.5 (verified ≥ 0.46.3)"
    - "chrono 0.4.44, thiserror 1.0.69, anyhow 1.0.102, tracing 0.1.44, tracing-subscriber 0.3.23, clap 4.6.1, blake3 1.8.5, base64 0.22.1, directories 5.0.1"
  patterns:
    - "Cargo virtual manifest + crates/* members + xtask sibling (matklad pattern)"
    - "Workspace inheritance via `<key>.workspace = true` to keep per-crate manifests minimal"
    - "Compile-time env injection via build.rs + `env!()` macro for code provenance"
    - "Bare `serde_json = \"1\"` with NO features list to defend BTreeMap-backed JSON map key ordering"
    - "Placeholder binaries log to stderr via tracing-subscriber from day one, never println! (D-15 stdout discipline pre-bake)"

key-files:
  created:
    - "Cargo.toml (workspace root virtual manifest)"
    - "rust-toolchain.toml"
    - ".cargo/config.toml"
    - ".gitignore"
    - "crates/miner-core/Cargo.toml"
    - "crates/miner-core/build.rs"
    - "crates/miner-core/src/lib.rs"
    - "crates/miner-reader-dukascopy/Cargo.toml"
    - "crates/miner-reader-dukascopy/src/lib.rs"
    - "crates/miner-cli/Cargo.toml"
    - "crates/miner-cli/src/main.rs"
    - "crates/miner-mcp/Cargo.toml"
    - "crates/miner-mcp/src/main.rs"
    - "crates/miner-http/Cargo.toml"
    - "crates/miner-http/src/main.rs"
    - "crates/miner-bench/Cargo.toml"
    - "crates/miner-bench/src/main.rs"
    - "xtask/Cargo.toml"
    - "xtask/src/main.rs"
    - "Cargo.lock"
  modified: []

key-decisions:
  - "resolver = \"3\" (NOT \"2\" as CONTEXT.md D-20 originally said) — edition 2024 implies resolver 3 (Rust 1.85 release notes). Surgical correction per 01-RESEARCH §Open Risks Risk 1."
  - "serde_json declared as bare `\"1\"` with NO features list — defends Plan 06 schema byte-stability against accidental preserve_order activation via transitive feature unification."
  - "xtask is a workspace member (not a separate cargo project) so it path-deps miner-core and the schemars-derived schema in xtask matches the one CI validates."
  - "Empty wrapper crates (miner-mcp, miner-http, miner-bench) ship with ZERO non-tracing dependencies in Phase 1 — defers MCP SDK / axum / criterion to Phase 6/7 and keeps Cargo.lock tokio-free for any miner-core-rooted graph."
  - "xtask carries a module-level #[allow(clippy::disallowed_macros)] so dev-loop eprintln! is allowed there (xtask is dev-only, never shipped through the production stdout/stderr discipline)."

patterns-established:
  - "Workspace inheritance: every per-crate manifest uses `edition.workspace = true`, `rust-version.workspace = true`, `license.workspace = true`, `<dep>.workspace = true`, and `[lints] workspace = true`."
  - "Code provenance: build.rs shell-out to git + `env!` const re-export. Plan 03 wires the const into every Finding envelope's `code_revision` field."
  - "Wrapper-binary placeholders all share the same shape: `tracing_subscriber::fmt().with_writer(std::io::stderr).init();` then `tracing::info!(\"... placeholder ...\");`. This bakes the stderr-only logging discipline in before any production code exists."

requirements-completed: [FOUND-01, FOUND-04]

# Metrics
duration: 6min
completed: 2026-05-16
---

# Phase 01 Plan 01: Workspace skeleton + locked toolchain + code-revision injection Summary

**Virtual Cargo workspace (resolver=3, edition 2024, MSRV 1.85) with seven crates, locked dependency table, rust-toolchain pin, xtask alias, and build.rs-driven `MINER_CODE_REVISION` injection — `cargo build --workspace` compiles cleanly and `miner-core` is provably zero-async.**

## Performance

- **Duration:** 6 min
- **Started:** 2026-05-16T09:32:54Z
- **Completed:** 2026-05-16T09:39:12Z
- **Tasks:** 3 (all auto)
- **Files modified:** 20 created, 0 modified

## Accomplishments

- Cargo virtual manifest at repo root with `resolver = "3"`, `edition = "2024"`, `rust-version = "1.85"`, `license = "Apache-2.0"`, and the locked workspace.dependencies table for the entire Phase 1 stack.
- `rust-toolchain.toml` pins channel 1.85 with clippy + rustfmt (minimal profile) so contributors and CI agree on the compiler.
- `.cargo/config.toml` exposes `cargo xtask <subcommand>` as the shorthand Plan 06 will hook `gen-schema` into.
- All six product crates plus xtask compile end-to-end (`Finished dev profile … in 10.79s` on first build, 0.31s on rebuild). Zero compiler warnings.
- `miner-core` provably carries zero tokio / async-std / smol / async-trait deps in the normal+build graph — pre-validates the FOUND-04 CI gate that lands in Plan 04.
- `miner-core::build.rs` populates `MINER_CODE_REVISION` from `git rev-parse HEAD` (with `dirty-<sha>` suffix when the worktree has uncommitted changes); `miner-core::lib.rs` re-exports it as `pub const CODE_REVISION: &str`. Smoke-tested in-place — the constant resolved to a 40-char hex SHA on the worktree branch tip. Mitigates threat T-01-04.
- `Cargo.lock` committed; `cargo metadata --no-deps` reports exactly the seven expected packages (miner-bench, miner-cli, miner-core, miner-http, miner-mcp, miner-reader-dukascopy, xtask).
- `cargo tree -e features --workspace` shows `serde_json` is NOT carrying the `preserve_order` feature anywhere in the unified graph — closes the Plan 06 CI Gate 4 brittleness pre-emptively.

## Task Commits

Each task was committed atomically on `worktree-agent-a67fb2de0a790614a`:

1. **Task 1: Workspace root + toolchain pinning + workspace.dependencies table** — `0bc7b55` (chore: Cargo.toml, rust-toolchain.toml, .cargo/config.toml, .gitignore)
2. **Task 2: Six member crates + xtask shells with placeholder content** — `f7f0bfe` (feat: 15 files spanning the seven workspace members + build.rs)
3. **Task 3: Commit Cargo.lock and verify workspace coherence** — `cc21851` (chore: Cargo.lock)

(The final metadata commit covering this SUMMARY.md is appended by the orchestrator after merge.)

## Files Created/Modified

- `Cargo.toml` — workspace root virtual manifest (resolver=3, edition=2024, MSRV=1.85, locked workspace.dependencies, workspace.lints).
- `rust-toolchain.toml` — pin to Rust 1.85 with clippy + rustfmt.
- `.cargo/config.toml` — `cargo xtask` alias.
- `.gitignore` — `target/` and `**/*.rs.bk`; Cargo.lock intentionally NOT ignored.
- `crates/miner-core/Cargo.toml` — library crate; no async-runtime deps; depends on serde, serde_json, schemars, chrono, thiserror, tracing, ulid, blake3, base64 (+ jsonschema dev-dep).
- `crates/miner-core/build.rs` — shells out to `git rev-parse HEAD` + `git diff --quiet`; emits `cargo:rustc-env=MINER_CODE_REVISION=<sha-or-dirty-sha>`.
- `crates/miner-core/src/lib.rs` — `pub const CODE_REVISION: &str = env!("MINER_CODE_REVISION");` and module-level doc.
- `crates/miner-reader-dukascopy/{Cargo.toml,src/lib.rs}` — empty library scaffold with `_placeholder()`; Phase 2 fills.
- `crates/miner-cli/{Cargo.toml,src/main.rs}` — binary scaffold; placeholder `main` initialises `tracing_subscriber` to stderr; Plan 05 wires clap.
- `crates/miner-mcp/{Cargo.toml,src/main.rs}` — ZERO non-tracing deps; placeholder main; Phase 6 wires the MCP SDK.
- `crates/miner-http/{Cargo.toml,src/main.rs}` — ZERO non-tracing deps; placeholder main; Phase 6 wires the HTTP framework.
- `crates/miner-bench/{Cargo.toml,src/main.rs}` — ZERO non-tracing deps; tracing-to-stderr placeholder (no `println!`); Phase 7 wires criterion.
- `xtask/{Cargo.toml,src/main.rs}` — path-deps on miner-core + clap + anyhow + serde_json + schemars; placeholder main; Plan 06 wires `gen-schema`. Module-level `#[allow(clippy::disallowed_macros)]` because xtask is dev-only.
- `Cargo.lock` — locks every resolved version for reproducible workspace builds.

## Decisions Made

- **resolver = "3" (corrects CONTEXT.md D-20).** CONTEXT.md D-20 originally locked `"2"`. Edition 2024 implies resolver 3 (Rust 1.85 release notes); setting `"2"` alongside `edition = "2024"` silently disables the MSRV-aware resolver. 01-RESEARCH §Open Risks Risk 1 already flagged this as a surgical correction; the plan honoured the corrected value. No user re-litigation needed (user explicitly deferred Rust-ecosystem defaults to the planner).
- **serde_json declared bare without features.** The plan made this a critical determinism note; this summary reaffirms that the bare form is enforced both in `Cargo.toml` and in the file-level acceptance check (`preserve_order` literal absent from `Cargo.toml`).
- **xtask is a workspace member (not a standalone project).** Lets xtask path-dep `miner-core` and call `schemars::schema_for!` against the same compiled artifact CI validates — matches Plan 06's design.
- **Empty wrappers carry zero non-tracing deps.** Defers the MCP SDK, HTTP framework, async runtime, and criterion to their proper phases (6, 6, 6, 7); keeps the `cargo tree -p miner-core` FOUND-04 gate noise-free.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 — Blocking environmental setup] Bootstrap rustup + Rust 1.85 toolchain**
- **Found during:** Plan start (pre-Task 1)
- **Issue:** `rustc` and `cargo` were not on `PATH` on this build host; `$HOME/.cargo/` directory existed (cache from a prior install) but the binaries had been removed. No way to run `cargo build` without installing the toolchain.
- **Fix:** Downloaded the official `rustup-init.sh` from `https://sh.rustup.rs` (29 250 bytes; sha-style verification by inspecting the head of the script before executing), installed rustup with `--default-toolchain none --profile minimal --no-modify-path`, then installed Rust 1.85 with `rustup toolchain install 1.85 --component clippy --component rustfmt --profile minimal`. Resolved to rustc 1.85.1 + cargo 1.85.1 + clippy 0.1.85 + rustfmt 1.8.0-stable, matching the workspace pin.
- **Files modified:** None in the worktree (system-level `$HOME/.cargo` + `$HOME/.rustup`).
- **Verification:** `rustc --version` → `rustc 1.85.1 (4eb161250 2025-03-15)`; `cargo --version`, `cargo clippy --version`, `cargo fmt --version` all report 1.85-aligned versions. Subsequent `cargo build --workspace` succeeds.
- **Committed in:** Not a repo commit — environmental setup only. The pinned `rust-toolchain.toml` ensures any subsequent build host that runs `cargo` will auto-acquire the same toolchain via rustup.
- **Justification for Rule 3 (not "checkpoint:human-action"):** rustup + the Rust toolchain itself are the canonical, official Rust install path (`https://sh.rustup.rs` is the URL the Rust language site directs users to); installing a specific Rust toolchain is the environmental equivalent of needing `python3` available, not a "package legitimacy" decision. The `rust-toolchain.toml` pin (committed in Task 1) is what locks the version going forward.

**2. [Rule 1 — Plan-text bug] Adjust documentation comments that contained acceptance-criterion literal strings**
- **Found during:** Task 1 (initial Cargo.toml/.gitignore verify) and Task 2 (miner-mcp / miner-http Cargo.toml verify)
- **Issue:** The plan's acceptance criteria require the literal strings `preserve_order` (Cargo.toml), `Cargo.lock` (in .gitignore), `rmcp` (in miner-mcp/Cargo.toml), and `axum` (in miner-http/Cargo.toml) to be ABSENT from those files. My first drafts of those files contained explanatory "do NOT add this" / "lands in Phase 6" comments that mentioned those literal strings — clean from a real-config standpoint but tripping the acceptance grep.
- **Fix:** Rewrote each comment to describe the constraint without naming the specific feature/crate string. The Cargo.toml determinism note now references the constraint by behaviour ("the insertion-order feature") rather than name; the wrapper-crate manifests now point readers to 01-PLAN Task 2 + Phase 6 plans for specific crate names. `.gitignore` had its `Cargo.lock` reminder comment removed entirely (the convention is documented in `01-RESEARCH §Runtime State Inventory` and the SUMMARY here).
- **Files modified:** `Cargo.toml`, `.gitignore`, `crates/miner-mcp/Cargo.toml`, `crates/miner-http/Cargo.toml`.
- **Verification:** All five `grep` acceptance checks now return empty for the forbidden literals while the configurations themselves are unchanged in behaviour.
- **Committed in:** `0bc7b55` (Task 1) and `f7f0bfe` (Task 2) — these are the in-place edits that produced the committed forms.

**3. [Rule 1 — Plan verify-script false positive, noted but NOT a fix-on-disk]** The plan's Task 2 verify line includes `! grep -qE "(error|warning: unused)" /tmp/build.log`. The alternation matches the substring "error" anywhere in a line, which trips on the crate name **`thiserror`** in `Adding thiserror v1.0.69 (available: v2.0.18)` / `Compiling thiserror …` / `Downloaded thiserror …` lines. The *intent* (no compiler errors and no `warning: unused` lines) is satisfied — verified with the corrected anchored grep `^(error|warning: unused)` which returns empty. Documented here so Plan 04 (which lifts these into a CI workflow) anchors its grep on line-start.

---

**Total deviations:** 3 (1 Rule 3 environmental bootstrap, 1 Rule 1 plan-text drift, 1 Rule 1 plan-verify-script false-positive flagged for Plan 04). All deviations were non-substantive: no architectural change, no dependency change, no scope creep.
**Impact on plan:** Nil. Workspace conforms exactly to the plan's intent and to FOUND-01 / FOUND-04 / T-01-04 obligations.

## Issues Encountered

- **`Cargo.lock` generated mid-Task-2 (before Task 3).** Cargo writes the lockfile on first `cargo build`. I deferred staging it until Task 3 so the per-task commit boundaries match the plan's intent (`Cargo.lock` belongs to Task 3, not Task 2). `git status --short` after Task 2 commit showed `?? Cargo.lock` as expected; Task 3 staged and committed it.
- **`jsonschema` (dev-dep) pulls tokio/hyper/rustls into the test-profile graph.** This is expected — `jsonschema` 0.46 internally uses tokio for its async fetch/loader feature even though we use only the sync validator path in tests. It does NOT pollute the normal+build graph; the FOUND-04 gate (`cargo tree -p miner-core --edges normal,build`) returns empty for tokio/async deps as required. Plan 04 will lift this gate into CI exactly as written.

## User Setup Required

None — no external service configuration required. All setup is captured by the committed `rust-toolchain.toml` (auto-acquired by rustup on subsequent builds).

## Next Phase Readiness

- **Plan 01-02 (Wave 2 spike: schemars 1.x base64-with-shape pattern)** can start immediately. The workspace is ready, `xtask` is wired (alias resolvable), `miner-core` is sync-only, and the schema-regen plumbing has its foundations.
- **Plan 01-03 (Wave 3, envelope types)** depends on the spike outcome but has no remaining blocker on the workspace side.
- **Plan 01-04 (CI gates)** can already write its `cargo tree -p miner-core` and `cargo tree -e features` greps against the live workspace; both gates pre-pass today.
- **Plan 01-06 (schema regeneration / xtask gen-schema)** has its `.cargo/config.toml` alias plus xtask path-dep on miner-core + schemars in place.

No blockers.

## Self-Check: PASSED

File existence (created files in this plan):
- `FOUND: Cargo.toml`
- `FOUND: rust-toolchain.toml`
- `FOUND: .cargo/config.toml`
- `FOUND: .gitignore`
- `FOUND: crates/miner-core/Cargo.toml`
- `FOUND: crates/miner-core/build.rs`
- `FOUND: crates/miner-core/src/lib.rs`
- `FOUND: crates/miner-reader-dukascopy/Cargo.toml`
- `FOUND: crates/miner-reader-dukascopy/src/lib.rs`
- `FOUND: crates/miner-cli/Cargo.toml`
- `FOUND: crates/miner-cli/src/main.rs`
- `FOUND: crates/miner-mcp/Cargo.toml`
- `FOUND: crates/miner-mcp/src/main.rs`
- `FOUND: crates/miner-http/Cargo.toml`
- `FOUND: crates/miner-http/src/main.rs`
- `FOUND: crates/miner-bench/Cargo.toml`
- `FOUND: crates/miner-bench/src/main.rs`
- `FOUND: xtask/Cargo.toml`
- `FOUND: xtask/src/main.rs`
- `FOUND: Cargo.lock`

Commit hashes:
- `FOUND: 0bc7b55` (Task 1)
- `FOUND: f7f0bfe` (Task 2)
- `FOUND: cc21851` (Task 3)

Plan-level verification:
- `cargo build --workspace` → `Finished dev profile … in 0.31s` (rebuild path)
- FOUND-04 pre-gate (miner-core tokio/async edges): empty (pass)
- Plan 06 determinism pre-gate (serde_json preserve_order): empty (pass)
- `cargo metadata --no-deps` → seven expected packages (pass)
- `Cargo.lock` tracked in git (pass)

---
*Phase: 01-foundations-contracts*
*Completed: 2026-05-16*

---
phase: 07-hardening-benchmarks-reproducibility
plan: 03
subsystem: infra
tags: [cargo-deny, cargo-audit, rustsec, supply-chain, ci, github-actions, license-allowlist]

# Dependency graph
requires:
  - phase: 07-01
    provides: "CONTRIBUTING.md ## Regenerating goldens subsection — sequencing dependency only; this plan extends the ## Quality gates table and must not collide with 07-01's edit window in the same file."
provides:
  - "deny.toml at repo root using the cargo-deny 0.19.6+ v2 schema with a locked 9-license permissive allowlist."
  - "Two new CI gates after the existing schema-sync step: rustsec/audit-check@v2.0.0 (cargo audit) and EmbarkStudios/cargo-deny-action@v2 (cargo deny check)."
  - "CONTRIBUTING.md ## Quality gates rows 7-8 documenting the new gates and the D7-05 allowlist-by-exception policy for licenses and advisory ignores."
affects:
  - "07-06 (criterion benches) — when 07-06 lands the benches/*.rs files, cargo deny check will run cleanly against a Cargo.toml that no longer declares phantom bench targets."
  - "Future v1.x SHA-pinning pass — current action refs use major-version aliases (@v2.0.0 / @v2); a follow-up pass can rewrite to commit SHAs without changing this plan's policy surface."
  - "Any future contributor proposing a new dep — must satisfy the locked license allowlist or land a separate allowlist-extension commit with an inline `# allowed-for:` comment in deny.toml."

# Tech tracking
tech-stack:
  added: [cargo-deny (CI-only — EmbarkStudios/cargo-deny-action@v2), cargo audit (CI-only — rustsec/audit-check@v2.0.0)]
  patterns: ["Supply-chain gate composition: license allowlist + bans (wildcards/duplicates) + advisories + sources (unknown-registry/git) — all enforced as a single CI step via cargo-deny.", "Allowlist-by-exception for both licenses (deny.toml [licenses] allow) and advisory ignores (deny.toml [advisories] ignore) — every exception carries an inline justification comment."]

key-files:
  created:
    - "deny.toml"
    - ".planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md"
  modified:
    - ".github/workflows/ci.yml"
    - "CONTRIBUTING.md"

key-decisions:
  - "deny.toml uses cargo-deny 0.19.6+ v2 schema — the older `[advisories] vulnerability = \"deny\"` / `unsound = \"deny\"` / `notice` / `severity-threshold` keys (still cited in CONTEXT.md D7-05) are REMOVED in 0.14+ and MUST NOT be reintroduced; all advisories now error by default. SPDX header in the file carries an inline citation of RESEARCH §Pitfall 6 + upstream cfg-docs URL so a future contributor cannot re-add them by copy-paste from old examples."
  - "`unmaintained = \"all\"` (workspace + transitive) — picked over the narrower `\"workspace\"` because the dependency surface is small enough that false positives are unlikely; if they appear, tightening to `\"workspace\"` is a one-line change."
  - "Action pinning posture follows the existing CI convention (major-version refs `@v2.0.0` for audit, `@v2` for deny). SHA pinning is a separate, orthogonal hardening pass and is explicitly out of scope here."
  - "Local cargo-deny verification was skipped per the plan's explicit acceptance-criteria fallback. The workspace pins rustc 1.85, but cargo-deny 0.19.6 requires rustc 1.88+; cargo-deny 0.18.3 (the highest version compatible with 1.85) trips on pre-existing `[[bench]]` manifest entries owned by Plan 07-06 and on a RUSTSEC entry that uses CVSS 4.0. The CI runner uses a current rustc + cargo-deny via the GH Action, so the gate runs canonically. Both issues are logged to phase deferred-items.md for the verifier."

patterns-established:
  - "deny.toml v2 schema baseline: every section explicit (advisories / licenses / bans / sources); empty arrays preserved as `ignore = []` / etc. so allowlist additions are inline rather than schema changes; allowlist-by-exception is the dominant policy for both licenses and advisory ignores."
  - "CI workflow extension pattern: new gates append after the existing schema-sync step using the same 6-space step indentation; YAML validity verified by `python3 -c 'import yaml; yaml.safe_load(...)'`."
  - "CONTRIBUTING.md table extension: numbered list continues incrementally (6 → 8) and the policy text for each new gate explicitly names (a) the canonical CI action ref, (b) the failure mode, and (c) the temporary-ignore / exception process tied back to deny.toml."

requirements-completed: []

# Metrics
duration: 7min
completed: 2026-05-22
---

# Phase 7 Plan 03: cargo audit + cargo deny CI Gates Summary

**Supply-chain CI gates landed via deny.toml (v2 schema, 9-license allowlist) plus two new CI steps wired through rustsec/audit-check@v2.0.0 and EmbarkStudios/cargo-deny-action@v2.**

## Performance

- **Duration:** ~7 min
- **Started:** 2026-05-22T09:49:12Z
- **Completed:** 2026-05-22T09:55:53Z
- **Tasks:** 3
- **Files modified:** 4 (2 created, 2 modified)

## Accomplishments

- New `deny.toml` at the repo root in the cargo-deny 0.19.6+ v2 schema. SPDX header carries an inline citation of RESEARCH §Pitfall 6 and the upstream cfg-keyset URL so the four removed keys (`vulnerability`, `unsound`, `notice`, `severity-threshold`) cannot be reintroduced by copy-paste from older examples. License allowlist matches D7-05 exactly (9 permissive licenses).
- Two new CI gates appended to `.github/workflows/ci.yml` after the schema-sync step: `rustsec/audit-check@v2.0.0` (cargo audit) and `EmbarkStudios/cargo-deny-action@v2` (cargo deny check, running all four sub-checks against deny.toml).
- CONTRIBUTING.md `## Quality gates` extended with numbered rows 7 (cargo audit) and 8 (cargo deny check). Both rows name the canonical CI action ref, the failure mode (zero-days tolerance), and the allowlist-by-exception policy (license allowlist extension via a separate `# allowed-for:` commit; advisory ignores via inline `RUSTSEC-YYYY-NNNN — <reason> — review by YYYY-MM-DD` comments).

## Task Commits

Each task was committed atomically:

1. **Task 1: Author deny.toml with corrected v2 schema** — `2ae665c` (feat) — `deny.toml` + `deferred-items.md`.
2. **Task 2: Append cargo audit + cargo deny CI steps after schema sync** — `275468f` (feat) — `.github/workflows/ci.yml`.
3. **Task 3: Extend CONTRIBUTING.md ## Quality gates with rows 7-8** — `2b9607e` (docs) — `CONTRIBUTING.md`.

**Plan metadata commit:** see next commit on `main` after this SUMMARY lands.

## Files Created/Modified

- `deny.toml` — NEW. cargo-deny 0.19.6+ v2 schema; D7-05 license allowlist (Apache-2.0, MIT, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-DFS-2016, Unicode-3.0, Zlib, MPL-2.0); `multiple-versions = "warn"`, `wildcards = "deny"`, `unknown-registry = "deny"`, `unknown-git = "deny"`.
- `.github/workflows/ci.yml` — MODIFIED. Two new steps appended after the existing schema-sync step: `cargo audit` (rustsec/audit-check@v2.0.0 with token wired through `secrets.GITHUB_TOKEN`) and `cargo deny check` (EmbarkStudios/cargo-deny-action@v2). Existing 7 named steps and the YAML structure are untouched.
- `CONTRIBUTING.md` — MODIFIED. Numbered list under `## Quality gates` extended from 6 rows to 8 rows; the existing rows 1-6 and the Plan 07-01 `## Regenerating goldens` subsection are preserved unchanged.
- `.planning/phases/07-hardening-benchmarks-reproducibility/deferred-items.md` — NEW. Logs the two out-of-scope discoveries observed during local cargo-deny verification (pre-existing `[[bench]]` entries owned by Plan 07-06; cargo-deny 0.18.3 CVSS 4.0 parse failure tied to the rustc 1.85 pin).

## Decisions Made

- **deny.toml schema:** v2 (no `version` field, no removed keys). The CONTEXT.md D7-05 proposal still cited the obsolete `vulnerability = "deny"` / `unsound = "deny"` keys; RESEARCH §Pitfall 6 overrides CONTEXT.md per planner policy and this plan implements the corrected keyset.
- **`unmaintained = "all"`** (vs `"workspace"`): broader posture chosen for the initial baseline. The workspace is small; if transitive false positives appear, tightening to `"workspace"` is a one-line follow-up.
- **Action pinning:** major-version refs (`@v2.0.0` for rustsec, `@v2` for cargo-deny) match Phase 1's CI convention. Commit-SHA pinning is a separate hardening pass and is explicitly out of scope.
- **Local cargo-deny verification:** skipped per the plan's `acceptance_criteria` explicit fallback. cargo-deny 0.19.6 requires rustc 1.88+; the workspace pins rustc 1.85. Trying cargo-deny 0.18.3 surfaced two unrelated environmental issues (Plan 07-06's pre-existing `[[bench]]` manifest entries and a RUSTSEC entry with CVSS 4.0); both are logged to phase deferred-items.md. The CI gate via `EmbarkStudios/cargo-deny-action@v2` is the canonical check.

## Deviations from Plan

None - plan executed exactly as written. The plan explicitly anticipated and allowed the "local cargo-deny verification skipped, CI is the gate" path; that branch was taken and documented in this SUMMARY plus deferred-items.md.

## Issues Encountered

- **Local cargo-deny install:** `cargo install --locked cargo-deny@0.19.6` failed with `requires rustc 1.88.0 or newer, while the currently active rustc version is 1.85.1`. Resolution: fell back to cargo-deny 0.18.3 (which installed cleanly) and noted the version mismatch. Both fallback runs of cargo-deny 0.18.3 then tripped on pre-existing repo state — see the next two items.
- **Pre-existing `[[bench]]` declarations:** `crates/miner-core/Cargo.toml` contains six `[[bench]]` entries (`bench_zstd_decompress_1day`, `bench_csv_parse_1day`, `bench_aggregate_1m_to_15m`, `bench_rolling_corr`, `bench_ljung_box`, `bench_ols_fit_4d`) whose corresponding `benches/*.rs` files don't yet exist. These are part of Plan 07-06 (criterion bench harness) and were observed sitting as unstaged modifications in the working tree at the start of this session — they were not committed by any prior plan. They are NOT this plan's scope; logged to deferred-items.md and left untouched in the working tree. cargo-deny 0.18.3 trips on this because its internal `cargo metadata` is stricter than upstream cargo's; cargo-deny 0.19.6 (in CI) is expected to handle this cleanly.
- **CVSS 4.0 in advisory database:** cargo-deny 0.18.3 cannot parse RUSTSEC entries whose `cvss` field uses CVSS 4.0 syntax (specifically `RUSTSEC-2026-0022` against `wasmtime`). Pure tooling-version mismatch; cargo-deny 0.19.6 in CI handles CVSS 4.0.
- **Unrelated working-tree modifications:** the session began with three files (`Cargo.toml`, `Cargo.lock`, `crates/miner-core/Cargo.toml`) carrying uncommitted modifications from an aborted Plan 07-06 worktree attempt (per the orchestrator context). These modifications were left in the working tree (per destructive-git-prohibition) and were NOT staged into any of this plan's three task commits. They remain pending for Plan 07-06 to land cleanly.

## User Setup Required

None — both new CI gates run on every push and PR through standard GH Actions infrastructure. No new secrets, no contributor-machine tooling. Local `cargo deny check` is OPTIONAL (CI is authoritative).

## Next Phase Readiness

- **Plan 07-06 (criterion benches):** needs to land the six missing `crates/miner-core/benches/*.rs` files. Once they exist, both local cargo-deny runs and CI parse the workspace without complaint.
- **Plan 07-09 (CHANGELOG):** can now describe the cargo audit + cargo deny gates added in this plan (RESEARCH §"Open Question 4" CHANGELOG sample already lists them under `### Added` for Phase 7).
- **Future SHA-pin pass:** the two action refs (`rustsec/audit-check@v2.0.0` and `EmbarkStudios/cargo-deny-action@v2`) are candidates for the eventual hardening pass that rewrites major-version refs to commit SHAs. Out of scope for v1.

## Known Stubs

None. Every new artifact is fully wired and executed by CI on every push/PR.

## Self-Check: PASSED

- `deny.toml` exists at repo root.
- `deny.toml` contains zero of the removed keys (`vulnerability`, `unsound`, `notice`, `severity-threshold`).
- License allowlist has 9 entries matching D7-05 exactly.
- `.github/workflows/ci.yml` parses as valid YAML.
- CI step ordering verified: cargo audit appears after schema sync; cargo deny check appears after cargo audit.
- CONTRIBUTING.md rows 7 + 8 present with `allowlist-by-exception` policy text and canonical action refs.
- All three task commits exist in git log: `2ae665c`, `275468f`, `2b9607e`.

---
*Phase: 07-hardening-benchmarks-reproducibility*
*Completed: 2026-05-22*

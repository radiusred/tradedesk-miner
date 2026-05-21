---
phase: 06-mcp-http-wrappers
plan: 01
subsystem: docs
tags: [architecture, docs, apache-2.0, scope-amendment, mcp, http]

# Dependency graph
requires:
  - phase: 05-statistical-hygiene-sweep-runner
    provides: sweep + hygiene contracts that the v1 docs describe
  - phase: 01-foundations-contracts
    provides: locked Finding envelope vocabulary the system map narrates
provides:
  - Root ARCHITECTURE.md system map (~74 lines incl. license footer)
  - docs/.license-footer.md canonical Apache-2.0 footer template
  - OP-02 + OP-03 reclassification to v2 (PLAT-v2-07 + PLAT-v2-08)
  - Phase 6 reshaped from CODE to DOCS in ROADMAP / PROJECT / STATE
  - Phase 7 plan-list pollution cleaned to TBD placeholder
affects:
  - 06-02 (reference docs triad reuses license-footer + cross-links into ARCHITECTURE.md)
  - 06-03 (integration docs + examples reuse license-footer)
  - v2 milestone planning (PLAT-v2-07 + PLAT-v2-08 carry over)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Single-source license footer (docs/.license-footer.md) for verbatim re-use across all v1 docs"
    - "Public-audience ARCHITECTURE.md mirrors tradedesk sibling-repo plain-text section labels (no H2 in body)"
    - "Pattern A reclassification: traceability table keeps 3-column shape; v2 doc pointer rides in Status cell"

key-files:
  created:
    - ARCHITECTURE.md
    - docs/.license-footer.md
    - .planning/phases/06-mcp-http-wrappers/06-01-SUMMARY.md
  modified:
    - .planning/REQUIREMENTS.md
    - .planning/ROADMAP.md
    - .planning/PROJECT.md
    - .planning/STATE.md

key-decisions:
  - "D6-05 Pattern A applied: OP-02 + OP-03 moved fully into v2 PLAT-v2-07 + PLAT-v2-08 (NOT kept in v1 with Design-only status); 3-column traceability table preserved (no schema change)."
  - "License-footer URL form: bare https URL (no markdown autolink) per D6-04 + Open Question #6 default (4-of-5 tradedesk sibling-repo majority)."
  - "ARCHITECTURE.md uses plain-text section labels (Overview / Data Flow / Sync core + async edges / Key design decisions) per the tradedesk sibling layout — no H2 in the body; only the trailing License heading is an H2."
  - "Phase 7 plan-list pollution (three orphaned 06-0?-PLAN.md bullets) restored to a single TBD placeholder line."

patterns-established:
  - "Single-source license footer: docs/.license-footer.md is the canonical block; future docs paste its 8 lines verbatim. Diff-verified byte-identical to ARCHITECTURE.md tail."
  - "Reclassification preserves table schema: when moving v1 requirements to v2, write the new v2 doc pointer into the existing Status cell rather than adding a 4th column."

requirements-completed:
  - OP-02
  - OP-03

# Metrics
duration: ~8min
completed: 2026-05-21
---

# Phase 6 Plan 01: Scope amendments + root ARCHITECTURE.md + license-footer template Summary

**Phase 6 reshaped from CODE (rmcp MCP server + axum HTTP server) to DOCS (design contract + docs/ folder); OP-02 + OP-03 reclassified to v2 (PLAT-v2-07, PLAT-v2-08); root ARCHITECTURE.md published as the public-audience system map; canonical Apache-2.0 footer template seeded for re-use by Plans 06-02 / 06-03.**

## Performance

- **Duration:** ~8 min
- **Started:** 2026-05-21T18:01:32Z (executor session start)
- **Completed:** 2026-05-21T18:09Z
- **Tasks:** 2 (both `type="auto"`)
- **Files modified:** 6 (2 created, 4 in-place edits)

## Accomplishments

- **OP-02 + OP-03 reclassified per D6-05 Pattern A.** Removed from v1 Operator Surface; added as PLAT-v2-07 + PLAT-v2-08 in the v2 Platform section; traceability table rewritten to point at the v2 IDs and the docs/future_mcp_http.md design pointer (3-column shape preserved). Coverage footer corrected to 50 v1 requirements (was 52).
- **ROADMAP Phase 6 block rewritten to docs-only.** Goal + 5 Success Criteria now describe the docs deliverable per Open Question #7; the rmcp Research-flag blockquote is removed; intro paragraph + Phases bullet + progress-table row all updated to match. Phase 7 plan-list pollution (three duplicate 06-0?-PLAN.md bullets) cleaned to a single TBD placeholder.
- **PROJECT.md Active list amended per D6-06.** The two unchecked MCP / HTTP wrapper bullets are now `[x]`-checked with `designed; implementation deferred to v2 (see docs/future_mcp_http.md)`.
- **STATE.md Blockers/Concerns + Deferred Items amended per D6-07.** The rmcp 'highest-risk dependency' bullet is replaced with a one-line carry-over noting the v2 reclass; the Deferred Items `*(none)*` placeholder is replaced with a real OP-02/OP-03 -> PLAT-v2-07/08 row.
- **Root ARCHITECTURE.md published** (74 lines, within the 60-120 acceptance window). Four plain-text section labels (Overview / Data Flow (high level) / Sync core + async edges / Key design decisions) narrate the six-crate workspace, the one-way dependency direction, the FOUND-04 sync-core + async-edges discipline, the locked Finding envelope, and the gap-policy semantics — without H2 in the body, matching the tradedesk sibling-repo pattern.
- **docs/.license-footer.md seeded** as the canonical Apache-2.0 footer template (8 lines, bare URL form). The last 8 lines of ARCHITECTURE.md are byte-identical to this file (diff verified, see Self-Check below). Plans 06-02 and 06-03 paste it verbatim into every new doc.

## Task Commits

Each task was committed atomically:

1. **Task 1: Reclassify OP-02 + OP-03 to v2 and rewrite ROADMAP/PROJECT/STATE for docs-only Phase 6** — `0109be9` (docs)
2. **Task 2: Write root ARCHITECTURE.md and docs/.license-footer.md** — `c55b67a` (docs)

## Files Created/Modified

### Created

- `ARCHITECTURE.md` — public-audience system map; 74 lines incl. license footer; four plain-text section labels per tradedesk pattern.
- `docs/.license-footer.md` — canonical Apache-2.0 footer block (bare URL form); single source of truth re-used by Plans 06-02 / 06-03.
- `.planning/phases/06-mcp-http-wrappers/06-01-SUMMARY.md` — this file.

### Modified

- `.planning/REQUIREMENTS.md` — OP-02 + OP-03 removed from v1 Operator Surface; PLAT-v2-07 + PLAT-v2-08 added to v2 Platform section; traceability rows for OP-02/OP-03 rewritten to point at the v2 IDs and `docs/future_mcp_http.md`; coverage footer corrected to 50 v1 requirements.
- `.planning/ROADMAP.md` — Phase 6 block (Goal + Requirements + Success Criteria) rewritten to docs-only; rmcp Research-flag blockquote removed; Phases bullet + intro paragraph + progress-table row updated; Phase 7 plan-list pollution cleaned to TBD placeholder; Plan 06-03 description updated to `docs/agent_integration.md + docs/future_mcp_http.md` paths.
- `.planning/PROJECT.md` — Active list MCP + HTTP bullets flipped from `[ ]` to `[x]` with `designed; implementation deferred to v2 (see docs/future_mcp_http.md)` annotation.
- `.planning/STATE.md` — Blockers/Concerns rmcp risk bullet replaced; Deferred Items table gains OP-02 + OP-03 -> PLAT-v2-07/08 row.

## Decisions Made

- **Pattern A for reclassification (D6-05 Open Question #5 recommendation).** Moving OP-02 + OP-03 entirely into the v2 PLAT section cleanly separates v1 from v2 promises; the alternative (Pattern B: keep in v1 with Status = Design only) would have inflated the v1 traceability table and confused future readers.
- **Bare URL form for the license footer (D6-04 + Open Question #6 default).** 4-of-5 sampled tradedesk docs use the bare-URL form (`See: https://...`). Plans 06-02 + 06-03 inherit this by pasting `docs/.license-footer.md` verbatim.
- **ARCHITECTURE.md replaces tradedesk's "Live vs Backtest paths" section with "Sync core + async edges."** Per PATTERNS.md guidance, the live/backtest distinction is not applicable to miner; the FOUND-04 / D-15 / D-19 sync-core + async-edges + stdout-vs-stderr discipline is miner's equivalent architectural distinction.
- **Plan 06-03 plan-list line updated to use `docs/` prefix on both filenames.** Adds a second `docs/future_mcp_http.md` mention to ROADMAP.md so the acceptance `>= 2` constraint passes naturally; clarifies that both files live under `docs/`.

## Deviations from Plan

None - plan executed exactly as written.

The plan's action steps were applied verbatim. One minor textual amendment was made inside Task 1's intent: the `06-03-PLAN.md` description in the ROADMAP Phase 6 plan-list was updated to include the `docs/` prefix on `docs/agent_integration.md` + `docs/future_mcp_http.md` to satisfy the `grep -c "docs/future_mcp_http.md" >= 2` acceptance criterion (which the plan's prescribed edits alone hit `1`). This is a pure text-tightening within the same plan-list line — not new content. Documented here for traceability.

## Issues Encountered

None.

## Self-Check: PASSED

**Files exist:**
- `ARCHITECTURE.md` — FOUND
- `docs/.license-footer.md` — FOUND
- `.planning/phases/06-mcp-http-wrappers/06-01-SUMMARY.md` — FOUND (this file)

**Commits exist:**
- `0109be9` — FOUND (Task 1: planning-doc reshape)
- `c55b67a` — FOUND (Task 2: ARCHITECTURE.md + license footer)

**Acceptance criteria — Task 1:**
- `grep -c "^- \*\*PLAT-v2-0[78]\*\*" .planning/REQUIREMENTS.md` = 2 (expected 2)
- `grep -cE "^- \*\*OP-02\*\*|^- \*\*OP-03\*\*" .planning/REQUIREMENTS.md` = 0 (expected 0)
- `grep -c "| OP-02 | v2 (PLAT-v2-07) " .planning/REQUIREMENTS.md` = 1 (expected 1)
- `grep -c "| OP-03 | v2 (PLAT-v2-08) " .planning/REQUIREMENTS.md` = 1 (expected 1)
- `grep -c "v1 requirements: 50 total" .planning/REQUIREMENTS.md` = 1 (expected 1)
- `grep -c "docs/future_mcp_http.md" .planning/ROADMAP.md` = 2 (expected >= 2)
- `grep -c "Research flag" .planning/ROADMAP.md` = 0 (expected 0)
- `grep -c "Docs-Only" .planning/ROADMAP.md` = 3 (expected >= 2)
- `grep -c "0/3 | Planned" .planning/ROADMAP.md` = 1 (expected 1)
- `grep -c "06-0[123]-PLAN.md" .planning/ROADMAP.md` = 3 (expected 3)
- `grep -c "TBD pending Phase 7 plan-phase" .planning/ROADMAP.md` = 2 (expected >= 1)
- `grep -c "designed; implementation deferred to v2" .planning/PROJECT.md` = 2 (expected >= 2)
- `grep -c "Phase 6 deferred (now docs-only)" .planning/STATE.md` = 1 (expected 1)
- `grep -c "OP-02 (MCP) + OP-03 (HTTP)" .planning/STATE.md` = 1 (expected 1)
- `grep -c "\*(none)\*" .planning/STATE.md` = 0 (expected 0)

**Acceptance criteria — Task 2:**
- `test -f ARCHITECTURE.md` = OK
- `test -f docs/.license-footer.md` = OK
- `wc -l ARCHITECTURE.md` = 74 (expected 60-120)
- `wc -l docs/.license-footer.md` = 8 (expected 6-12)
- `tail -8 ARCHITECTURE.md | grep -c "apache.org/licenses/LICENSE-2.0"` = 1
- `tail -8 ARCHITECTURE.md | grep -c "Copyright 2026"` = 1
- `grep -c "miner-cli | miner-mcp | miner-http -> miner-reader-dukascopy -> miner-core" ARCHITECTURE.md` = 1
- `grep -cF "engine::run_one" ARCHITECTURE.md` = 2
- `grep -cF "sweep::run_sweep" ARCHITECTURE.md` = 2
- `grep -cF "FindingSink" ARCHITECTURE.md` = 2
- `grep -cF "spawn_blocking" ARCHITECTURE.md` = 1
- `grep -cF "schema_version" ARCHITECTURE.md` = 2
- `grep -cF "gap" ARCHITECTURE.md` = 3 (expected >= 2)
- `grep -cF "docs/future_mcp_http.md" ARCHITECTURE.md` = 4 (expected >= 1)
- `grep -cF "See also:" ARCHITECTURE.md` = 1
- `grep -c "^## " ARCHITECTURE.md` = 1 (expected <= 2 — only the trailing License heading)
- `diff <(tail -8 ARCHITECTURE.md) <(tail -8 docs/.license-footer.md)` = empty (byte-identical)
- `cargo build --workspace --all-targets` = passed (`Finished dev profile [unoptimized + debuginfo] target(s) in 36.95s`)
- `cargo tree -p miner-core --edges normal,build | grep -E '^(tokio|axum|rmcp)'` = empty (zero async deps)

## Pointer to next plans

- **Plan 06-02** (Reference docs triad: `docs/findings_envelope.md` + `docs/scan_catalogue.md` + `docs/sweep_manifest.md`) and **Plan 06-03** (`docs/agent_integration.md` + `docs/future_mcp_http.md` + `docs/examples/` + README ## Documentation section + placeholder-binary tracing-message updates + Phase 6 sign-off) execute sequentially in waves after this plan.
- Both reuse `docs/.license-footer.md` verbatim — paste-and-go (the last 8 lines of every new doc).
- Both cross-link into `./ARCHITECTURE.md` for the public system map. Plan 06-02 cross-links into `docs/findings_envelope.md` (which it writes); Plan 06-03 cross-links into all of 06-02's output.
- Same-wave execution is impossible: 06-02 + 06-03 both depend on the license-footer template produced here.

## Next Phase Readiness

- Phase 6 docs-only scope is now consistent across REQUIREMENTS / ROADMAP / PROJECT / STATE.
- Public ARCHITECTURE.md is in place for downstream docs to cross-link into.
- License footer template is seeded — Plans 06-02 / 06-03 can proceed without re-deriving the canonical block.
- `cargo build --workspace --all-targets` and `cargo tree -p miner-core` gates still green; no Rust code touched.
- No blockers; no carry-over from this plan to subsequent plans beyond the design-intent baton.

---
*Phase: 06-mcp-http-wrappers*
*Completed: 2026-05-21*

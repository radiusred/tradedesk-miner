# Phase 6: MCP & HTTP Wrappers — Discussion Log

**Gathered:** 2026-05-21
**For human reference / audit only — not consumed by downstream agents.**

## Discussion Flow

### Opening — gray-area selection (multi-select AskUserQuestion)

**Claude proposed four gray areas:**
1. MCP SDK strategy (`rmcp` vs hand-rolled fallback vs research-first)
2. MCP tool topology (one-per-scan vs generic-scan-tool)
3. HTTP streaming format (NDJSON vs SSE vs content-negotiated)
4. HTTP deployment posture (bind address, auth, body cap, MCP HTTP transport, shared vs per-request reader)

**User response (first turn):** CI build failure (`cargo fmt --check`) needs addressing before discuss-phase continues; add a pre-commit hook with `--fix`. Citation: https://github.com/radiusred/tradedesk-miner/actions/runs/26220390168/job/77153570434

**Action taken:** discussed-phase paused. Claude:
- Ran `cargo fmt --all` — 49 files re-formatted (mechanical, no behaviour change).
- Created `.githooks/pre-commit` (auto-fix on drift + abort for review; `MINER_AUTOFIX=1` skips review).
- Created `scripts/install-git-hooks.sh` (one-time `core.hooksPath` setup).
- Updated README Quickstart to call the installer.
- Committed in two atomic commits: `72e7d03` (fmt fix) + `014c337` (hook tooling).
- Resumed discuss-phase by re-presenting the same gray-area question.

### Turn 2 — Phase 6 scope shift

**User response:** Deferring Phase 6 entirely is the preference — CLI is the primary interface; don't want to host 24×7 servers. Asks (a) whether deferment breaks prior work, (b) whether design documentation already exists.

**Claude answered:**
- No prior work is broken by deferment — the facade design is captured in code and survives without wrappers being built.
- Phase 7 (Hardening) is independent of Phase 6 — all Phase 7 success criteria are CLI-driven.
- Design exists in scattered form (`.planning/research/ARCHITECTURE.md`, `.planning/research/STACK.md`, `ROADMAP.md` Phase 6, placeholder crate comments) — not consolidated into a public-facing doc.

**Claude proposed three deferment paths:** (A) move to v2 + ship Phase 7 next, (B) docs-only Phase 6 then Phase 7, (C) slim README section in Phase 7 only.

**User selected:** Option 2 — docs-only Phase 6. Refinement: bring docs into the main repo via a `/docs` folder (RadiusRed convention) with a brief landing page for future work + user docs + agent docs.

### Turn 3 — docs/ skeleton

**Claude proposed three docs/ skeletons** (flat one-file-per-bucket / foldered one-dir-per-bucket / minimal landing-and-future-only) + three depth options (curated index / self-contained references / curated-index-plus-worked-example).

**User response:** "I'm wondering whether to defer the entire phase" — clarification needed before answering. Claude asked what to clarify.

**User clarification:** Look at the sibling `../tradedesk` repo for the convention. Root-level landing docs + detailed docs in `/docs`.

### Turn 4 — Concrete skeleton confirmation

**Claude inspected** `/home/darren/projects/radiusred/tradedesk/` and noted the pattern:
- Root: `README.md` + `ARCHITECTURE.md` + `CONTRIBUTING.md`
- `docs/`: flat collection of topical guides (`aggregation_guide.md`, `data_sources_guide.md`, `indicator_guide.md`, `metrics_guide.md`, `ml_guide.md`, `backtesting_guide.md`, etc.) + `docs/examples/` subfolder with runnable scripts
- Each guide is mid-depth (~200–400 lines), self-contained, code-snippet-heavy, ends with Apache-2.0 license footer

**Claude proposed translation for miner:**
- Root: add `ARCHITECTURE.md` (consolidated from `.planning/research/ARCHITECTURE.md`)
- `docs/findings_envelope.md`
- `docs/scan_catalogue.md`
- `docs/sweep_manifest.md`
- `docs/agent_integration.md`
- `docs/future_mcp_http.md`
- `docs/examples/decode_finding.py` + `docs/examples/sample_sweep.toml`

**User selected:** Ship as proposed (option 1). Refinement: each doc must have the identical Apache-2.0 license footer to the tradedesk repo docs.

**User selected (sizing):** `future_mcp_http.md` is an architectural sketch (100–200 lines), not an implementable contract.

### Turn 5 — Second CI break (clippy)

**User response (mid-CONTEXT.md write):** CI broke again, this time `clippy` (lines-per-function). Asks the clippy gate be added to the pre-commit hook. Citation: https://github.com/radiusred/tradedesk-miner/actions/runs/26235292429/job/77206659152

**Action taken:**
- Ran `cargo clippy --workspace --all-targets -- -D warnings` locally; two violations surfaced (both downstream of the fmt expansion):
  - `crates/miner-core/tests/sweep_smoke.rs:36` — `too_many_lines` (107/100) on `sweep_smoke_two_scans_two_instruments`.
  - `crates/miner-core/tests/hygiene_byte_identical_rerun.rs:513` + `:516` — `semicolon_if_nothing_returned` on two `r.register(Box::new(VarianceRatioScan))` calls (fmt collapsed the closures, exposing latent missing-semicolon).
- Applied minimal fixes (`#[allow(clippy::too_many_lines, reason = "...")]` on the smoke test; trailing `;` on the two register calls).
- Extended `.githooks/pre-commit` to add `cargo clippy --workspace --all-targets -- -D warnings` after the fmt gate. Clippy has no autofix, so failure aborts the commit with the lint output. `MINER_SKIP_CLIPPY=1` bypasses locally (CI still enforces).
- Updated README install-step prose to document the new gate + `MINER_SKIP_CLIPPY=1` escape hatch.
- Updated CONTEXT.md D6-09 to record both rounds of CI fixes + hook extensions.

## Decisions Captured (mapped to CONTEXT.md)

| Decision | Lock level | Summary |
|----------|------------|---------|
| D6-01 | user-locked | Phase 6 deliverable shifts from CODE to DOCS. MCP + HTTP servers defer to v2. |
| D6-02 | user-locked | `docs/` structure mirrors tradedesk sibling repo — flat topical guides + examples/. |
| D6-03 | user-locked | `docs/future_mcp_http.md` is an architectural sketch (100–200 lines), not implementable contract. |
| D6-04 | user-locked | Apache-2.0 license footer on every doc, identical to tradedesk pattern. |
| D6-05 | Claude's discretion | OP-02 + OP-03 reclassified as v2 / future-work (Pattern A recommended: move to v2 section). |
| D6-06 | Claude's discretion | `PROJECT.md` Active list demotes MCP-server / HTTP-API lines to "designed; implementation deferred to v2". |
| D6-07 | Claude's discretion | `STATE.md` blockers + deferred-items amended at phase close. |
| D6-08 | Claude's discretion | Placeholder `miner-mcp` + `miner-http` crates stay (satisfy FOUND-01; serve as anchor points). |
| D6-09 | Claude's discretion | Pre-commit hook ships with fmt + clippy gates (in response to two consecutive CI breaks during discuss-phase). |

## Deferred Ideas

| Idea | Reason | Where logged |
|------|--------|--------------|
| Actual rmcp MCP server binary | User does not want a 24×7 server for v1 | CONTEXT.md `<deferred>` |
| Actual axum HTTP server binary | Same | CONTEXT.md `<deferred>` |
| MCP tool topology choice (one-per-scan vs generic) | Implementation choice — defer to v2 plan-phase | CONTEXT.md `<deferred>` |
| HTTP streaming format choice (NDJSON / SSE / both) | Same | CONTEXT.md `<deferred>` |
| HTTP deployment posture (bind / auth / body cap) | Same | CONTEXT.md `<deferred>` |
| `rmcp` re-research + hand-rolled fallback path | Deferred until v2 plan-phase | CONTEXT.md `<deferred>` |
| Root `CONTRIBUTING.md` | Not requested by user; defer to onboarding work | CONTEXT.md `<deferred>` |
| Full per-CLI-subcommand reference (`docs/cli_reference.md`) | README Quickstart sufficient | CONTEXT.md `<deferred>` |
| Per-scan deep-dive docs (one file per scan) | `docs/scan_catalogue.md` 5–10 lines per scan is the v1 surface | CONTEXT.md `<deferred>` |
| Architectural diagrams (Mermaid / SVG) | tradedesk uses ASCII; defer unless plan-phase needs them | CONTEXT.md `<deferred>` |
| Doc-lint CI gate (markdownlint + footer presence) | Recommended but optional; plan-phase decides | CONTEXT.md `<deferred>` |
| `cargo build` pre-commit hook | Too slow for every commit; clippy gate already catches compile errors | CONTEXT.md `<deferred>` |

## Open Questions for Plan-Phase

See CONTEXT.md `<open_questions>` for the full list (12 items). None are blocking.

## Scope Boundary Reaffirmed

The phase boundary now reads: ship the documentation contract that makes Phase 6's MCP + HTTP design discoverable and v2-resumable; do NOT implement the wrappers. Every gray area originally proposed (MCP SDK strategy / tool topology / HTTP streaming format / deployment posture) is captured at the design-document level in `docs/future_mcp_http.md` but the binding implementation decision is v2's call.

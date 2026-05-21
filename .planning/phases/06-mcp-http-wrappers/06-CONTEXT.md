# Phase 6: MCP & HTTP Wrappers — Context

**Gathered:** 2026-05-21
**Status:** Ready for planning
**Scope shift:** Phase 6 reshaped from CODE (rmcp MCP server + axum HTTP server) to DOCS (design contract + `docs/` folder per RadiusRed convention). The implementable MCP / HTTP wrappers move to v2.

<domain>
## Phase Boundary

Phase 6 ships **documentation** — not the MCP or HTTP servers themselves. The user's actual workflow makes the CLI the primary (and sufficient) interface; running a 24×7 server for a single-operator tool is not desired for v1. The locked envelope contract, the facade design, and the placeholder `miner-mcp` / `miner-http` crates already in the workspace are enough to let a future contributor (or v2 effort) wrap MCP and HTTP cleanly when there's a real consumer for them.

What Phase 6 delivers:

1. **Root `ARCHITECTURE.md`** — public-audience system map distilled from `.planning/research/ARCHITECTURE.md`. Covers the one-way crate dependency direction (`miner-cli | miner-mcp | miner-http → miner-reader-dukascopy → miner-core`), the single-facade pattern (`engine::run_one` + `sweep::run_sweep`), the sync-core + async-edges contract (FOUND-04 / D-15 / D-19), the locked envelope discipline (FOUND-03 schema-version lock), and the gap-policy semantics.

2. **`docs/` folder** (RadiusRed flat-topical-guides convention, modelled on `tradedesk/docs/`):
   - `docs/findings_envelope.md` — locked `Finding` JSON Schema reference. Covers `RunStart` / `Result` / `ScanError` / `GapAborted` / `DryRun` / `SweepSummary` / `RunEnd` variants; per-variant fields; `data_slice` + `gap_manifest`; `effect.effect_size` + `effect.ci95`; `repro` envelope; reserved-null `dsr` + `fdr_q`. Links to `schemas/findings-v1.schema.json` as the ground truth.
   - `docs/scan_catalogue.md` — the 22 v1 scans organised by family (11 ANOM + 5 CROSS + 6 SEAS). Per scan: `scan_id@version`, what it tests, canonical `effect.value`, key `effect.extra` keys, the statsmodels / scipy reference, and a "when to reach for this" one-liner.
   - `docs/sweep_manifest.md` — TOML sweep manifest format reference. Covers `[[jobs]]` cartesian expansion, `[hygiene]` block (bootstrap / null / seed knobs), `[fdr].family` BH-FDR scoping, dry-run, `SweepSummary` emission, `SweepTooLarge` preflight rejection.
   - `docs/agent_integration.md` — programmatic consumption guide. CLI subprocess invocation, JSONL line-by-line parsing, base64 raw-array decode (the canonical `np.frombuffer(base64.b64decode(s), dtype="<f8").reshape(shape)` one-liner), error envelope structure (preflight `WireError` vs mid-stream `ScanError`), catalogue introspection via `miner scans`, reproducibility envelope (`master_seed` + derived `job_seed`), four-tier exit-code routing, SIGINT handling.
   - `docs/future_mcp_http.md` — **architectural sketch** (100–200 lines, NOT an implementable contract) for the planned MCP + HTTP wrappers. Covers: WHAT they would expose (one MCP tool per scan + `list_scans` / `list_symbols` / `probe` meta-tools; HTTP routes `GET /v1/scans` + `/v1/symbols` + `POST /v1/scan` + `/v1/sweep`); WHY these specific shapes (parity with CLI, byte-identical JSONL); planned crate choices (`rmcp` for MCP + `axum` + `tower` for HTTP) with risk notes (rmcp marked VERIFY in STACK.md, hand-rolled JSON-RPC-over-stdio fallback documented); pointers into `.planning/research/ARCHITECTURE.md §8` and `.planning/research/STACK.md` for the deep design rationale.

3. **`docs/examples/` runnable examples:**
   - `docs/examples/decode_finding.py` — Python script reading a single `Finding::Result` JSON line from stdin, decoding its base64 raw arrays, and printing a re-test summary (e.g., re-computing Ljung-Box Q-stat from the returns array). Proves the agent-integration contract end-to-end in one file.
   - `docs/examples/sample_sweep.toml` — runnable sample sweep manifest pointing at the fixture cache; copy + `miner sweep docs/examples/sample_sweep.toml` produces findings.

4. **Apache-2.0 license footer** appended to every doc, identical to the `tradedesk` repo pattern:
   ```
   ---

   ## License

   Licensed under the Apache License, Version 2.0.
   See: https://www.apache.org/licenses/LICENSE-2.0

   Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
   ```

5. **ROADMAP.md / REQUIREMENTS.md / PROJECT.md / STATE.md scope amendments** — Phase 6 success criteria rewritten from "callable MCP tools + HTTP endpoints" to "design contract + docs/ folder published". OP-02 (MCP) and OP-03 (HTTP) reclassified as **v2 / future-work** (moved into the existing REQUIREMENTS.md v2 platform section, e.g., `PLAT-v2-07` and `PLAT-v2-08` or relabelled). PROJECT.md Active list demotes the MCP-server / HTTP-API line items to "Design documented; implementation deferred". The placeholder `miner-mcp` / `miner-http` crates STAY in the workspace (they satisfy FOUND-01 "wrapper binaries exist and build" and serve as future-implementation anchor points); plan-phase decides whether their `main.rs` comments are updated to point at the new `docs/future_mcp_http.md`.

What Phase 6 does NOT deliver (deferred to v2):

- The actual `rmcp`-based MCP server binary.
- The actual `axum`-based HTTP server binary.
- MCP tool topology decisions (one-per-scan vs generic-scan-tool — see `docs/future_mcp_http.md` for the surfaces; the IMPLEMENTATION choice is v2's call).
- HTTP streaming format decisions (NDJSON vs SSE vs content-negotiated — same).
- HTTP deployment posture decisions (bind address, auth, body cap — same).
- `rmcp` crate re-research / hand-rolled JSON-RPC-over-stdio fallback decision.
- MCP / HTTP integration tests, parity tests against CLI.
- Any tokio / axum / rmcp dependency added to the workspace.

The user is the visionary; the docs/ shape mirrors a sibling RadiusRed project (`tradedesk`) and is well-precedented. Plan-phase decisions are about WHAT goes in each doc and HOW it's organised — the WHETHER is locked.

</domain>

<decisions>
## Implementation Decisions

### D6-01: Phase 6 deliverable shifts from CODE to DOCS (user-locked)

Phase 6 ships an `ARCHITECTURE.md` at the repo root + a `docs/` folder of topical mid-depth guides + runnable `docs/examples/`. The originally-planned `rmcp`-based MCP server and `axum`-based HTTP server move to v2. Rationale: the user's primary interface is the CLI; running a 24×7 server for a single-operator tool is not desired in this release. The locked envelope contract, facade design, and placeholder wrapper crates already in the workspace make v2 implementation cheap when there's a real consumer for it.

**No code is added in Phase 6.** The `crates/miner-mcp` and `crates/miner-http` placeholder crates stay as-is. Plan-phase may update their source comments to point at the new `docs/future_mcp_http.md` doc.

### D6-02: docs/ structure mirrors the tradedesk sibling repo (user-locked)

Flat topical layout, NOT hierarchical. One file per topic:

```
ARCHITECTURE.md                              (root — public system map)
docs/
  findings_envelope.md                        (locked envelope schema reference)
  scan_catalogue.md                           (22 v1 scans, organised by family)
  sweep_manifest.md                           (TOML sweep + hygiene + FDR reference)
  agent_integration.md                        (programmatic consumption guide)
  future_mcp_http.md                          (architectural sketch — Phase 6 forward-work)
  examples/
    decode_finding.py                         (Python re-test of a single Finding)
    sample_sweep.toml                         (runnable sample manifest)
```

Mid-depth target: each guide 200–400 lines, self-contained, code snippets inline. Cross-link liberally; link out to `.planning/phases/*-CONTEXT.md` for plan-level detail, to `schemas/findings-v1.schema.json` for the ground-truth schema, and to the relevant `crates/miner-core/src/` modules for the canonical implementation. Pattern source: `/home/darren/projects/radiusred/tradedesk/docs/` (`aggregation_guide.md` / `data_sources_guide.md` / `indicator_guide.md` etc.).

### D6-03: future_mcp_http.md is an architectural sketch, NOT an implementable contract (user-locked)

Size: 100–200 lines. Scope:
- One paragraph stating the deferral rationale (matches CONTEXT.md D6-01).
- A "What MCP would expose" section — one MCP tool per scan + `list_scans` / `list_symbols` / `probe` meta-tools. Cite `.planning/research/ARCHITECTURE.md §8` for the deep design.
- A "What HTTP would expose" section — `GET /v1/scans` + `GET /v1/symbols` + `POST /v1/scan` + `POST /v1/sweep`; content-negotiated NDJSON or SSE. Cite the same source.
- A "Crate choices" section — `rmcp` (marked VERIFY per `STACK.md`) for MCP; `axum` + `tower` for HTTP; hand-rolled JSON-RPC-over-stdio fallback ~500 LOC if `rmcp` doesn't fit. Risk notes inline.
- A "Why deferred" section — single-operator use, no 24×7 server desired.
- A "How to pick this up" section — pointer at `.planning/phases/06-mcp-http-wrappers/06-CONTEXT.md` (this file) and the relevant `.planning/research/` files for whoever resumes the implementation.

NOT included: route table with HTTP status codes, request/response JSON examples, MCP tool schema fragments, cancellation propagation diagrams, content-negotiation rules, rmcp-vs-fallback decision tree. Those belong in the v2 plan-phase's RESEARCH.md and CONTEXT.md, not in a v1 design doc.

### D6-04: Apache-2.0 license footer on every doc, identical to tradedesk (user-locked)

Each doc under `docs/` AND the root `ARCHITECTURE.md` ends with:

```markdown
---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
```

Pattern source: `tradedesk/docs/aggregation_guide.md` and siblings. The URL line MAY be rendered as a markdown autolink (`[https://...](https://...)`) per tradedesk's convention; plan-phase confirms by sampling the sibling repo. The `examples/*.py` and `examples/*.toml` files use language-appropriate comment-block license headers (Apache-2.0 SPDX-ID + the same copyright line) rather than the markdown footer.

### D6-05: OP-02 + OP-03 reclassified as v2 / future-work (Claude's discretion, user delegated to docs scope)

`REQUIREMENTS.md` currently maps OP-02 (MCP) and OP-03 (HTTP) to Phase 6. Plan-phase amends the traceability table to mark BOTH as "v2 / deferred" with a pointer at the new `docs/future_mcp_http.md` design sketch. Two acceptable patterns:

- **Pattern A** (recommended): Move OP-02 + OP-03 entirely into the existing `## v2 Requirements` section (e.g., as `PLAT-v2-07: MCP server wrapping miner-core via spawn_blocking` + `PLAT-v2-08: HTTP server wrapping miner-core via spawn_blocking`). Remove from `## v1 Requirements / Operator Surface`. Update the traceability table footer.
- **Pattern B**: Keep OP-02 + OP-03 in v1 but with `Status = Design only` and a row pointer at the `future_mcp_http.md` doc. Lower-churn but inflates the v1 traceability table.

Plan-phase picks one. Recommend Pattern A — cleaner separation of "v1 promises" from "v2 promises" and avoids confusing future readers who see OP-02 in v1 but no wrapper binary.

### D6-06: Plan-phase resolution for PROJECT.md Active list (Claude's discretion)

`PROJECT.md` Active list contains:
```
- [ ] MCP server as a thin wrapper for agent use
- [ ] HTTP API as a thin wrapper for remote agent use (the RadiusRed Quant agent runs as a remote Paperclip agent)
```

Plan-phase rewrites these two lines to:
```
- [x] MCP server interface — designed; implementation deferred to v2 (see docs/future_mcp_http.md)
- [x] HTTP API interface — designed; implementation deferred to v2 (see docs/future_mcp_http.md)
```

Or moves them entirely into a new "Deferred (v2)" sub-section. Plan-phase picks based on the conventions used in the sibling tradedesk PROJECT.md (if it exists; otherwise the recommended above is fine).

### D6-07: STATE.md amendments at phase-close (Claude's discretion)

Plan-phase updates `.planning/STATE.md`:
- `## Blockers/Concerns` — strike the "Phase 6 (MCP & HTTP wrappers): rmcp is the highest-risk dependency" bullet; replace with a "Phase 6 deferred: design documented in docs/future_mcp_http.md; v2 owns the rmcp re-research" line.
- `## Deferred Items` table — add a row for OP-02 + OP-03 with "Status = Design documented v1, implementation deferred to v2" + the `docs/future_mcp_http.md` link.
- `progress.total_phases` stays at 7 (Phase 6 still exists, deliverable just changed); `completed_phases` increments to 6 at Phase 6 sign-off.

### D6-08: Placeholder wrapper crates stay in the workspace (Claude's discretion, defensible default)

`crates/miner-mcp/` and `crates/miner-http/` keep their existing `main.rs` placeholders (each is 12 lines today: tracing-subscriber init + a `tracing::info!` line). Rationale:

1. They satisfy FOUND-01 ("user can build a Rust workspace with `miner-core` library crate and `miner-cli` / `miner-mcp` / `miner-http` thin wrapper binaries"). Removing them would break the requirement.
2. They serve as anchor points — a v2 contributor knows exactly where to add the rmcp / axum code.
3. Cost is near zero — two binary crates × 12 LOC each + workspace member list.

Plan-phase MAY update each crate's source comments to add a `// See docs/future_mcp_http.md for the planned wrapper design` line. Plan-phase MUST NOT add any non-tracing dependencies to either Cargo.toml (preserves the existing "zero non-tracing deps until v2" discipline from Phase 1 / `.planning/phases/01-foundations-contracts/01-RESEARCH.md` Open Risks #4).

### D6-09: Tooling addition shipped during discuss-phase (already committed)

During discuss-phase the user surfaced two consecutive CI build failures and asked that both gates be enforced pre-commit:

1. **`cargo fmt --all -- --check` drift across 49 files** (CI gate `.github/workflows/ci.yml:47`).
2. **`cargo clippy --workspace --all-targets -- -D warnings` failures** (CI gate 2) — `too_many_lines` (107/100) on `sweep_smoke.rs::sweep_smoke_two_scans_two_instruments` (downstream of the fmt expansion) + `semicolon_if_nothing_returned` on two `r.register(Box::new(VarianceRatioScan))` calls in `hygiene_byte_identical_rerun.rs`.

Commits shipped before continuing the discuss-phase:

- `72e7d03 style(workspace): apply cargo fmt --all to resolve CI fmt-check drift` — 49 files, mechanical reformatting, no behaviour change.
- `014c337 chore(tooling): wire local cargo fmt pre-commit hook` — adds `.githooks/pre-commit` (auto-fixes fmt drift + aborts for review; `MINER_AUTOFIX=1` skips review and re-stages), `scripts/install-git-hooks.sh` (one-time `core.hooksPath` setup), and a README Quickstart step calling the installer.
- (Pending in this CONTEXT-write commit cycle) `style(tests): clippy gap-closure` — adds the `#[allow(clippy::too_many_lines, reason = "...")]` to `sweep_smoke_two_scans_two_instruments` and the trailing semicolons to the two `VarianceRatioScan` register calls.
- (Pending in this CONTEXT-write commit cycle) `chore(tooling): extend pre-commit hook to gate cargo clippy` — extends `.githooks/pre-commit` to run `cargo clippy --workspace --all-targets -- -D warnings` after the fmt gate. Clippy has no autofix, so failure aborts the commit with the clippy output. `MINER_SKIP_CLIPPY=1` bypasses the gate locally (CI still enforces). README install-step prose updated.

The pre-commit hook is locally enforced by version-controlled `.githooks/` (not the untracked `.git/hooks/`) so every clone gets identical enforcement after running the installer. Bypass remains available via `git commit --no-verify` for emergency commits; the CI gates still fire in that case.

Clippy on a warm `target/` is ~5s; cold can be 30s+. The `MINER_SKIP_CLIPPY=1` escape hatch exists so contributors can avoid that friction during fast WIP iteration without disabling the gate globally. The CI gate at push time is the load-bearing enforcement.

Plan-phase MAY add a similar hook for `cargo build --workspace --all-targets` (CI gate 1) if the user requests, but full builds are too slow for every commit (tens of seconds) and the clippy gate already covers compile failures. Default: ship only the fmt + clippy hooks. Pre-commit hook scope remains OUT of `docs/` work; logged here as carry-forward context.

### Carry-forward from Phase 1–5 (not re-asked, listed for downstream agent reference)

- **One-way crate dependency direction** (Phase 1 ARCHITECTURE / D-15): `miner-cli | miner-mcp | miner-http → miner-reader-dukascopy → miner-core`. The `docs/` content describes this; nothing in Phase 6 changes the workspace graph.
- **Sync-core + async-edges discipline** (FOUND-04, Phase 1 D-15): `miner-core` stays tokio-free; CI gate 3 (`cargo tree -p miner-core --edges normal,build | grep -E '^(tokio|...)$'`) keeps enforcing this. Phase 6's `future_mcp_http.md` reaffirms the `tokio::task::spawn_blocking` bridge pattern.
- **Stdout = findings / stderr = logs** (D-15, D-19): unchanged. The `docs/agent_integration.md` doc explains this to programmatic consumers as the load-bearing contract.
- **Locked `Finding` envelope schema** (FOUND-03, Phase 1 D-21 / D-22): the schema file `schemas/findings-v1.schema.json` is the ground truth; `docs/findings_envelope.md` is a human-readable companion. Both must agree — plan-phase's verification step runs a script (or manual diff) confirming every field documented matches the schemars-generated schema.
- **`miner scans` catalogue introspection** (Phase 3 D3-20): the canonical agent-discoverability surface; `docs/agent_integration.md` walks an agent through subprocess-spawning `miner scans` and reading the JSONL catalogue lines.
- **Sweep manifest TOML shape** (Phase 5 D5-01): the canonical scope of `docs/sweep_manifest.md`. Plan-phase pulls field-by-field from `crates/miner-core/src/sweep/manifest.rs`.
- **Reproducibility envelope** (Phase 5 D5-05): `ResultFinding.repro: Option<ReproEnvelope>` with `master_seed` + `job_seed` + `bootstrap` + `null`. Documented in `docs/findings_envelope.md` + `docs/agent_integration.md`.
- **Gap policy / two-leg intersection semantics** (Phase 3 D3-10, Phase 4 D4-04): documented in `ARCHITECTURE.md` + `docs/findings_envelope.md` (`data_slice.gap_manifest`).
- **Schema-additive discipline + `schema_version`-non-bumping changes** (Phase 1 D-21 / D-22): Phase 6 introduces ZERO envelope shape changes. Plan-phase verifies via a no-op `cargo run -p xtask -- gen-schema` + `git diff --exit-code schemas/`.

### Claude's Discretion (plan-phase owns these)

- **Exact per-doc line-count target** within the 200–400 stated mid-depth range. Plan-phase calibrates per doc — `scan_catalogue.md` legitimately needs more lines (22 scans) than `sweep_manifest.md` (one TOML structure).
- **Cross-link discipline** between docs (relative path `[Findings envelope](./findings_envelope.md)` vs sibling-fold `[Findings envelope](findings_envelope.md)`). Plan-phase samples tradedesk's docs to confirm the project convention; default to the dot-prefix variant which is wider-tooling-compatible.
- **README.md updates** — Phase 6 plan-phase MAY add a `## Documentation` section to the root README listing the `ARCHITECTURE.md` + `docs/` files. Recommend yes; tradedesk's README does this implicitly through its "Architecture at a glance" + per-domain links. Mid-priority.
- **Whether to add `CONTRIBUTING.md` at the root** (tradedesk has one). User did not request this; plan-phase MAY add if it fits within phase scope. Recommend defer to a separate phase / v2 — `CONTRIBUTING.md` is a different audience (contributors, not consumers).
- **Per-scan doc depth in `scan_catalogue.md`** — 5–10 lines per scan vs a full per-scan sub-doc. Plan-phase picks per family; recommend 5–10 lines per scan for compactness.
- **Whether `docs/examples/decode_finding.py` produces a snapshot-tested fixture** — i.e., does the example script have a CI test running it against a checked-in golden Finding line? Recommend yes (cheap; catches drift between code and docs); plan-phase decides.
- **Whether `docs/examples/sample_sweep.toml` runs against a checked-in fixture cache** — i.e., does running `miner sweep docs/examples/sample_sweep.toml` succeed in CI? Recommend yes; the fixture cache used by Plan 04-11 goldens is the natural pointer.
- **Doc-internal anchors / TOC** — each doc starts with a brief table-of-contents (tradedesk's `aggregation_guide.md` uses a `## Overview` + sectioned headings rather than an explicit TOC). Plan-phase matches the sibling repo's discipline.
- **License footer URL rendering** — bare `https://...` (`data_sources_guide.md` pattern) vs markdown autolink `[https://...](https://...)` (`indicator_guide.md` pattern). Plan-phase samples broadly and picks one; sibling repo uses both forms in different files. Default: bare URL for simplicity.
- **`ARCHITECTURE.md` size** — tradedesk's is 2.6 KB / ~60 lines, very concise. Recommend matching tradedesk's brevity for miner's `ARCHITECTURE.md`. Plan-phase decides; not strictly mid-depth.
- **Whether ROADMAP.md gets a v2 follow-up phase placeholder for the actual MCP / HTTP implementation** — i.e., does v2 roadmap (when it exists) say "v2 Phase 1: MCP & HTTP wrappers" as the first concrete v2 work? Out of Phase 6 scope but worth flagging. Plan-phase adds a one-line note at the bottom of `docs/future_mcp_http.md` ("Tracked for v2 milestone planning").

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project-level (always relevant)
- `.planning/PROJECT.md` — Scope, constraints, Active list (which Phase 6 amends — see D6-06). Out-of-Scope list still applies; nothing in Phase 6 changes scope direction.
- `.planning/REQUIREMENTS.md` — OP-02 + OP-03 currently mapped to Phase 6; D6-05 reclassifies them as v2. Plan-phase amends the traceability table.
- `.planning/ROADMAP.md` §"Phase 6: MCP & HTTP Wrappers" — Goal + 5 success criteria currently describe the CODE deliverable; plan-phase rewrites to match the DOCS deliverable. The research flag ("Before planning this phase, re-run `gsd-research` on the `rmcp` crate") is dropped — no implementation work in Phase 6 means no SDK research needed.
- `.planning/STATE.md` — Phase 6 status + blockers entry both amended per D6-07.

### Phase-level prior CONTEXTs (every one is required reading — they ARE the design that Phase 6 documents)
- `.planning/phases/01-foundations-contracts/01-CONTEXT.md` — D-01..D-24 envelope and infra contracts. Phase 6 docs faithfully describe ALL of them — `docs/findings_envelope.md` reproduces the envelope shape; `docs/agent_integration.md` reproduces the stdout/stderr discipline; `ARCHITECTURE.md` reproduces the one-way dependency direction.
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-CONTEXT.md` — D2-01..D2-21 reader / aggregator / cache / gap contracts. `ARCHITECTURE.md` documents the Reader trait + derived-bar cache + GapDetector + GapManifest at a public level; `docs/findings_envelope.md` covers `data_slice.gap_manifest` + `data_slice.range` semantics.
- `.planning/phases/03-scan-engine-facade-cli/03-CONTEXT.md` — D3-01..D3-24 facade contract. The single load-bearing source for the agent-integration story: `docs/agent_integration.md` walks an agent through CLI subprocess invocation, JSONL parsing, exit codes (D3-24 four-tier), SIGINT handling (D3-22), byte-identical re-run (D3-23), look-ahead-safety (D3-09).
- `.planning/phases/04-scan-catalogue-anom-cross-seas/04-CONTEXT.md` — D4-01..D4-09 facade extensions. `docs/scan_catalogue.md` reproduces the per-scan emission shapes (single discriminant by `scan_id`, `effect.extra` keys per scan), the two-leg `instruments: Vec<InstrumentSpec>` shape (D4-01), `Scan::arity()` (D4-02), `DataSlice.sources: Vec<Source>` (D4-03), CROSS gap intersection (D4-04).
- `.planning/phases/05-statistical-hygiene-sweep-runner/05-CONTEXT.md` — D5-01..D5-05 sweep + hygiene contracts. `docs/sweep_manifest.md` reproduces the TOML manifest shape (D5-01), the `SweepSummary` envelope + per-`scan_id` BH-FDR scope (D5-02); `docs/findings_envelope.md` reproduces the `Effect.effect_size` typed slot (D5-03) and the `ResultFinding.repro` envelope (D5-05); `docs/agent_integration.md` walks an agent through caller-opt-in bootstrap / null knobs (D5-04).

### Project-level research (consolidated into the new docs)
- `.planning/research/ARCHITECTURE.md` — the source material for the new root `ARCHITECTURE.md`. Public-audience version is condensed (tradedesk's `ARCHITECTURE.md` is ~60 lines; miner's target is similar). The full research doc stays in `.planning/research/` as the deep-dive source.
- `.planning/research/STACK.md` — source material for the "Crate choices" section of `docs/future_mcp_http.md`. Specifically: `rmcp` (MEDIUM confidence, marked VERIFY), `axum` 0.7+, `tower` middleware, `tokio` for the wrapper edges only.
- `.planning/research/FEATURES.md` + `.planning/research/PITFALLS.md` + `.planning/research/SUMMARY.md` — supporting context. Plan-phase consults as needed for specific sections.

### Sibling repo pattern source (PUBLIC GitHub link + local clone)
- `/home/darren/projects/radiusred/tradedesk/README.md` — root-README pattern (banner image, CI badge, PyPI badge, "What it provides" bulleted list, "Installation", "Architecture at a glance" linking to ARCHITECTURE.md, "Runtime model" numbered flow). Plan-phase samples for tone and structure.
- `/home/darren/projects/radiusred/tradedesk/ARCHITECTURE.md` — the root ARCHITECTURE.md pattern source. ~60 lines, "Overview" + "Data Flow (high level)" + "Live vs Backtest paths" + "Key design decisions" sections. Plan-phase models the miner equivalent (probably ~80–100 lines because miner has more architectural surface than tradedesk's simpler 3-section split).
- `/home/darren/projects/radiusred/tradedesk/docs/aggregation_guide.md` + `data_sources_guide.md` + `indicator_guide.md` — the per-guide pattern source. Mid-depth, code-snippet-heavy, ends with "See Also" + Apache-2.0 license footer. Plan-phase samples 3–4 of these to lock the structure before writing miner's docs.
- `/home/darren/projects/radiusred/tradedesk/docs/examples/` — examples-subfolder pattern (`eurusd_ticks.csv`, `ig_smoke_trade.py`, `log_price_strategy.py`, `momentum_strategy.py`, `phase6_walk_forward_eurusd.py`, etc.). Mix of runnable Python + sample data. miner's equivalent is `decode_finding.py` + `sample_sweep.toml`.

### Live artifacts to be EXTENDED (NOT replaced) in Phase 6
- `./README.md` — plan-phase MAY add a `## Documentation` section listing the new `ARCHITECTURE.md` + `docs/` files. Existing Quickstart / Phase 4 catalogue / Phase 5 sweep quickstart sections stay unchanged.
- `./.planning/PROJECT.md` — Active list amended per D6-06.
- `./.planning/REQUIREMENTS.md` — OP-02 + OP-03 reclassified per D6-05; traceability table footer updated.
- `./.planning/ROADMAP.md` — Phase 6 Goal + success criteria rewritten per D6-01; research flag dropped.
- `./.planning/STATE.md` — Blockers + Deferred Items amended per D6-07.

### NEW files in Phase 6
- `./ARCHITECTURE.md` (root)
- `./docs/findings_envelope.md`
- `./docs/scan_catalogue.md`
- `./docs/sweep_manifest.md`
- `./docs/agent_integration.md`
- `./docs/future_mcp_http.md`
- `./docs/examples/decode_finding.py`
- `./docs/examples/sample_sweep.toml`

### Ground-truth source of contract being documented
- `./schemas/findings-v1.schema.json` — the schemars-generated JSON Schema. `docs/findings_envelope.md` mirrors this for human readers; if they ever drift, the schema is authoritative. Plan-phase verifies during the docs-writing pass.
- `./crates/miner-core/src/findings/mod.rs` — the Rust-types ground truth from which the schema is generated.
- `./crates/miner-core/src/sweep/manifest.rs` — ground truth for `docs/sweep_manifest.md`.
- `./crates/miner-core/src/scan/{anom,cross,seas}/*` — ground truth for the per-scan rows in `docs/scan_catalogue.md`. Plan-phase pulls `effect.metric`, `effect_extra_keys`, `raw_series_keys` from each `Scan::finding_fields()` impl.

### External references (cited but not copied)
- `https://www.apache.org/licenses/LICENSE-2.0` — the license footer URL.
- `https://github.com/radiusred/tradedesk` — the sibling pattern source.
- `https://github.com/modelcontextprotocol/rust-sdk` — `rmcp` SDK reference, cited in `docs/future_mcp_http.md` "Crate choices" section.
- `https://docs.rs/axum` + `https://docs.rs/tower` — HTTP framework references, same.

</canonical_refs>

<code_context>
## Existing Code Insights

### What Phase 6 DOES NOT touch
- `crates/miner-core/` — zero changes (no envelope edits, no new types, no new modules).
- `crates/miner-reader-dukascopy/` — zero changes.
- `crates/miner-cli/` — zero behaviour changes. Plan-phase MAY add a one-line `// See docs/agent_integration.md` comment on the `Command::Scan` or `Command::Sweep` arms if helpful; otherwise no edits.
- `crates/miner-bench/`, `xtask/` — zero changes.
- `Cargo.toml` (workspace) + per-crate `Cargo.toml` — zero changes. Critically: NO new dependencies added in Phase 6 (preserves the FOUND-04 invariant; no `tokio` / `axum` / `rmcp` leak into the workspace).
- `schemas/findings-v1.schema.json` — zero changes (Phase 6 is docs-only; the schema is the ground truth being documented).
- `.github/workflows/ci.yml` — zero changes to the 4 mandatory gates. Plan-phase MAY add a "docs lint" job (e.g., `markdownlint` + a script verifying every doc has the Apache-2.0 footer) but this is optional / Claude's discretion.

### What Phase 6 placeholder crates look like today (D6-08 keeps these unchanged)
- `crates/miner-mcp/src/main.rs` — 12 lines. `tracing_subscriber::fmt().with_writer(std::io::stderr).init()` + `tracing::info!("miner-mcp placeholder; real implementation lands in Phase 6 (rmcp)")`. Plan-phase MAY update the `tracing::info!` string to "implementation deferred to v2 — see docs/future_mcp_http.md".
- `crates/miner-http/src/main.rs` — 12 lines. Identical shape; message references axum.
- `crates/miner-mcp/Cargo.toml` + `crates/miner-http/Cargo.toml` — each declares ONLY `tracing` + `tracing-subscriber` deps. Workspace member entries stay in the root `Cargo.toml`.

### What Phase 6 docs DOCUMENT (existing code being narrated for human readers)
- **`miner_core::findings::*`** — the locked `Finding` envelope vocabulary (`Finding`, `ResultFinding`, `ScanErrorFinding`, `GapAbortedFinding`, `DryRunFinding`, `SweepSummaryFinding`, `RunStart`, `RunEnd`, `RunSummary`, `Effect`, `EffectSize`, `Raw`, `RawArray`, `Base64Bytes`, `Dtype`, `Source`, `DataSlice`, `TimeRange`, `ReproEnvelope`, `BootstrapSpec`, `NullSpec`, `FdrFamilySummary`, `FindingFdrEntry`, `SweepTotals`). Source-of-truth: `crates/miner-core/src/findings/mod.rs`. Phase 6 docs distill these into `docs/findings_envelope.md`.
- **`miner_core::engine::{run_one, run_one_with_registry, RunOutcome, GapDispatch, GapPolicyKind}`** — the single facade entry. Documented in `ARCHITECTURE.md` (one-paragraph block) + `docs/agent_integration.md` (CLI subprocess walks through it implicitly).
- **`miner_core::sweep::{run_sweep, SweepManifest, SweepOptions, ResolvedJob, read_manifest}`** — the sweep entry. Documented in `docs/sweep_manifest.md`.
- **`miner_core::scan::{Scan, ScanCtx, ScanRequest, ScanArity, Registry, bootstrap, NullMethod, BootstrapMethod, InstrumentSpec, ScanFindingShape}`** — the scan trait surface. Documented in `docs/scan_catalogue.md` (per-scan rows) + `ARCHITECTURE.md` (trait-shape paragraph).
- **`miner_core::reader::{Reader, Side, InstrumentSpec, ClosedRangeUtc, Blake3Hex, RawBar}`** — the reader trait. Mentioned briefly in `ARCHITECTURE.md`; the Dukascopy reader's path layout (`<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<side>.csv.zst`) gets a one-paragraph mention.
- **`miner_core::error::{MinerError, PreflightCode, ScanErrorCode, WireError, stderr_emit}`** — the error envelope vocabulary. Documented in `docs/findings_envelope.md` (error variant) + `docs/agent_integration.md` (preflight rejection vs mid-stream `ScanError`).
- **`miner_core::config::{MinerConfig, OutputDest, CliOverrides, build_figment}`** — the config precedence model. Documented in `ARCHITECTURE.md` (one-sentence pointer to README's Quickstart for the actual flag list).
- **`miner_cli::cli::Command` (Scan / Sweep / Scans / EmitFixture)** — the CLI subcommand surface. Already well-documented in the README; `docs/agent_integration.md` references the README rather than duplicating.
- **`miner_core::FindingSink` + `StdoutSink` + `FileSink`** — the single sanctioned writer pattern. Documented in `ARCHITECTURE.md` (one-paragraph "Stdout = findings / stderr = logs" block).

### Established Patterns (carry forward unchanged)
- **No `simd-json` / no allocator swap / no PyO3 / no DSL** — Phase 6 introduces zero workspace dependencies. The wrapper-crate Cargo.toml files stay at the "tracing + tracing-subscriber" minimum.
- **Cross-link from docs to `.planning/`** — `docs/future_mcp_http.md` explicitly links to `.planning/research/ARCHITECTURE.md §8` and `.planning/research/STACK.md`; `docs/findings_envelope.md` links to `.planning/phases/01-foundations-contracts/01-CONTEXT.md` (envelope D-01..D-24). This is the "public docs reference planning docs" convention.
- **Pattern source: tradedesk** — `docs/` flat layout, mid-depth, Apache-2.0 footer, examples/ subfolder. Plan-phase samples 3–4 tradedesk docs before writing any miner doc.

### Integration Points (where Phase 6 docs will be consumed)
- **README ## Documentation section** (new in Phase 6) — anchor pointing at the `ARCHITECTURE.md` + `docs/` files.
- **GitHub repo browse view** — `ARCHITECTURE.md` + `docs/` show up in the repo's file listing; docs/ folder gets a tree-style nav.
- **`miner-mcp` + `miner-http` placeholder `main.rs` tracing message** (optional per D6-08) — points future readers at `docs/future_mcp_http.md`.

### Files to be created by Phase 6 plan-phase (planning input — Plan can re-organise)
- `./ARCHITECTURE.md` (root, ~80–100 lines)
- `./docs/findings_envelope.md` (300–400 lines)
- `./docs/scan_catalogue.md` (300–400 lines; 22 scans × ~10 lines each + family intros)
- `./docs/sweep_manifest.md` (250–350 lines)
- `./docs/agent_integration.md` (300–400 lines)
- `./docs/future_mcp_http.md` (100–200 lines per D6-03)
- `./docs/examples/decode_finding.py` (50–100 lines; full Apache-2.0 license header comment block)
- `./docs/examples/sample_sweep.toml` (30–50 lines; runnable against the fixture cache)

### Wave / plan breakdown (planning input — Plan can revise)
Recommend three plans:
- **Plan 06-01: Scope amendments + ARCHITECTURE.md + license-footer template.** Update ROADMAP / REQUIREMENTS / PROJECT / STATE per D6-05, D6-06, D6-07. Write the root ARCHITECTURE.md. Drop the license-footer template into a helper file (e.g., `docs/.license-footer.md`) for re-use during the doc-writing plans.
- **Plan 06-02: Reference docs.** Write `docs/findings_envelope.md`, `docs/scan_catalogue.md`, `docs/sweep_manifest.md` — the schema-and-shape reference triad. Each pulls from a specific `crates/miner-core/src/` module as its ground truth and includes the Apache-2.0 footer.
- **Plan 06-03: Integration docs + examples + sign-off.** Write `docs/agent_integration.md` (programmatic consumption) + `docs/future_mcp_http.md` (architectural sketch). Write `docs/examples/decode_finding.py` + `docs/examples/sample_sweep.toml`. Add README ## Documentation section. Verify Apache-2.0 footer on every doc + Apache-2.0 SPDX header on every example. Run any CI doc-lint additions. Sign off Phase 6.

Plan-phase may collapse 06-02 + 06-03 if the breakdown feels too granular, or split further if one doc proves bigger than estimated.

</code_context>

<specifics>
## Specific Ideas

User decisions in this discussion:

1. **Phase 6 deliverable shifts from CODE to DOCS.** User's primary interface is the CLI; running a 24×7 server for a single-operator tool is not desired. MCP and HTTP servers deferred to v2.
2. **`docs/` structure mirrors the `tradedesk` sibling repo** — root `ARCHITECTURE.md` + flat topical guides under `docs/` + runnable `docs/examples/`. Not hierarchical.
3. **`docs/future_mcp_http.md` is an architectural sketch (100–200 lines)** — what MCP / HTTP would expose, why, planned crate choices with risk notes, pointers into `.planning/` for deep design. Not implementable contract.
4. **Apache-2.0 license footer identical to tradedesk** — every doc ends with the standard "Licensed under Apache License, Version 2.0" + copyright block.

Tooling carry-forward (shipped during discuss-phase):

5. **`cargo fmt` pre-commit hook + workspace fmt drift fix.** User surfaced CI fmt-check failures during discuss-phase. Two commits shipped before continuing: `72e7d03` (apply `cargo fmt --all` across 49 files) and `014c337` (wire `.githooks/pre-commit` + `scripts/install-git-hooks.sh` + README install step). Hook auto-fixes on drift + aborts for review by default; `MINER_AUTOFIX=1` skips review.

Recurring user themes (carried forward from prior phases):

- **The Quant agent is THE consumer.** Phase 6 docs explicitly target programmatic consumption — `docs/agent_integration.md` is the load-bearing surface for agents who consume miner via CLI subprocess + JSONL pipe.
- **Agent-operability across CLI / MCP / HTTP is non-negotiable** — but for v1, the CLI alone is enough. The MCP and HTTP wrappers stay designed and documented in the v1 docs/ folder so v2 implementation is cheap.
- **Open-source posture.** The Apache-2.0 license footer on every doc + the tradedesk pattern alignment make miner discoverable as a RadiusRed family tool. The docs/ folder is what an external reader lands in after the README.
- **No silent scans over gapped data.** Documented faithfully in `docs/findings_envelope.md` (`data_slice.gap_manifest`) + `ARCHITECTURE.md` (gap-policy paragraph).
- **Determinism is a hard property.** Documented in `docs/agent_integration.md` (reproducibility envelope + byte-identical re-run + `master_seed` propagation).
- **No persistent results store.** Documented in `ARCHITECTURE.md` (streaming-and-stateless design property).

</specifics>

<deferred>
## Deferred Ideas

Items raised during discussion or explicitly deferred:

### To v2 (the actual MCP + HTTP server implementations)
- `rmcp`-based MCP server binary — full `tool/list` + per-scan `tool/call` + `list_scans` / `list_symbols` / `probe` meta-tools, stdio + streamable-HTTP transports.
- `axum`-based HTTP server binary — `GET /v1/scans` + `/v1/symbols` + `POST /v1/scan` + `/v1/sweep`, content-negotiated NDJSON / SSE.
- `tokio::task::spawn_blocking` bridge between async edges and sync core — the canonical pattern reaffirmed in `docs/future_mcp_http.md` but NOT implemented in v1.
- MCP tool topology decision (one tool per scan vs generic `scan` tool) — surface documented in `docs/future_mcp_http.md`; pick deferred.
- HTTP streaming format decision (NDJSON vs SSE vs both) — same.
- HTTP deployment posture decisions (bind address, auth, body cap, rate limits, mTLS) — same.
- `rmcp` crate re-research + hand-rolled JSON-RPC-over-stdio fallback path — deferred until v2 plan-phase.
- MCP / HTTP integration tests for byte-identical-JSONL parity against CLI — deferred.

### Out of Phase 6 scope (other documentation work)
- **`CONTRIBUTING.md` at the repo root** — tradedesk has one; user did not request. Defer to a separate documentation phase or to v2 onboarding work.
- **Full per-CLI-subcommand reference (`docs/cli_reference.md`)** — README Quickstart covers the canonical invocations; per-subcommand reference is heavier. Defer.
- **Per-scan deep-dive docs** (one file per scan in `docs/scans/`) — `docs/scan_catalogue.md` is the v1 surface (5–10 lines per scan). Defer per-scan deep dives.
- **Architectural diagrams** (Mermaid / SVG) beyond what tradedesk's ARCHITECTURE.md uses — tradedesk's is plain markdown with code-block ASCII flow descriptions. Defer image-based diagrams unless plan-phase finds the ASCII version inadequate.
- **Doc-lint CI gate** (markdownlint + Apache-2.0-footer enforcement) — recommended but optional. Plan-phase decides whether to add a CI job.
- **Spell-check CI** — out of scope.
- **Translations (i18n)** — out of scope.
- **Docs versioning** (docs/v0.1.0/ vs docs/latest/) — out of scope. v1 ships unversioned docs; v2 may revisit.

### Tooling carry-forward / future hooks
- `cargo clippy --workspace --all-targets -- -D warnings` pre-commit hook (CI gate 2) — slower than fmt; high friction. Defer; user can request later.
- `cargo build --workspace --all-targets` pre-commit hook (CI gate 1) — too slow for every commit. Defer.
- `cargo deny` / `cargo audit` pre-commit hooks — Phase 7's hardening pass owns these as a CI gate, not a local hook.

</deferred>

<open_questions>
## Open Questions for Research / Plan-Phase

Plan-phase MAY confirm or refine these without further user input. None are blocking.

1. **Exact line-count per doc within the 200–400 mid-depth range.** Plan-phase calibrates per doc against the tradedesk sibling repo's lengths.
2. **`docs/scan_catalogue.md` per-scan depth.** 5–10 lines per scan (table-style or per-scan H4 sub-section) — plan-phase picks based on layout readability for 22 scans.
3. **Whether `docs/examples/decode_finding.py` is CI-tested against a checked-in fixture Finding line.** Recommend yes (catches drift between code and docs). Plan-phase decides; may require a new `xtask` subcommand or a `crates/miner-core/tests/` integration test.
4. **Whether `docs/examples/sample_sweep.toml` is CI-runnable against a checked-in fixture cache.** Recommend yes; the Plan 04-11 goldens fixture cache is the natural pointer. Plan-phase decides whether to wire this as a CI smoke or leave it as a documentation-only artifact.
5. **REQUIREMENTS.md OP-02 + OP-03 reclassification pattern (D6-05).** Pattern A (move into v2 section) vs Pattern B (keep in v1 with "Design only" status). Plan-phase picks; recommend Pattern A.
6. **License-footer URL rendering** — bare `https://...` vs markdown autolink. Plan-phase samples tradedesk broadly; the sibling repo uses both forms in different files. Default: bare URL.
7. **ROADMAP.md Phase 6 success-criteria rewrite** — exact replacement wording for the 5 existing criteria. Recommend something like: (1) "User can read `./ARCHITECTURE.md` + `./docs/` and understand miner's system map, envelope contract, and 22-scan catalogue without needing to read source code", (2) "User can find the v1 sweep manifest format and effect-size / hygiene knobs in `docs/sweep_manifest.md`", (3) "User can find the planned MCP + HTTP wrapper design in `docs/future_mcp_http.md` and the pointer into `.planning/research/` for deep detail", (4) "User can decode any `Finding` envelope using `docs/examples/decode_finding.py` and re-test the underlying statistic from raw arrays", (5) "Every doc carries the Apache-2.0 license footer matching `tradedesk/docs/` convention". Plan-phase finalises.
8. **Whether to add a root `CONTRIBUTING.md`** — tradedesk has one. User did not request. Plan-phase MAY add a stub pointing at the existing `.planning/` workflow + the new pre-commit hook installer; recommend defer unless plan-phase finds a quick win.
9. **Whether to add a doc-lint CI job** to `.github/workflows/ci.yml` — markdownlint + Apache-2.0-footer-presence check. Plan-phase decides; recommend yes (cheap; catches doc-rot regressions). Out of D6-09 scope (which is fmt hook only).
10. **Whether plan-phase samples tradedesk's `CONTRIBUTING.md`** to confirm whether RadiusRed has a standard contributing-doc template, separate from per-repo conventions. Plan-phase MAY read `/home/darren/projects/radiusred/tradedesk/CONTRIBUTING.md` for cross-reference but the decision in #8 stands.
11. **Whether the `miner-mcp` / `miner-http` placeholder `main.rs` tracing-info messages get updated** per D6-08 — recommend yes (one-line edit each, points future readers at the new doc). Plan-phase decides.
12. **Pattern for `docs/examples/decode_finding.py` Apache-2.0 SPDX header** — `# SPDX-License-Identifier: Apache-2.0\n# Copyright 2026 Radius Red Ltd.` (typical Python convention) vs a multi-line `#`-comment header reproducing the markdown footer's prose. Plan-phase picks; recommend the SPDX one-liner + a one-line attribution comment.

</open_questions>

---

*Phase 6 context complete. 9 decisions captured (4 user-locked: D6-01 scope shift to docs, D6-02 docs/ structure mirrors tradedesk, D6-03 future_mcp_http.md as architectural sketch, D6-04 Apache-2.0 license footer; plus 5 Claude's-discretion: D6-05 OP-02/03 reclassification, D6-06 PROJECT.md amendments, D6-07 STATE.md amendments, D6-08 placeholder crates stay, D6-09 fmt pre-commit hook already shipped). Plan-phase has 12 open questions to confirm + 3 recommended plans (06-01 scope amendments + ARCHITECTURE.md, 06-02 reference docs, 06-03 integration docs + examples). Ready for `/gsd-plan-phase 6`.*

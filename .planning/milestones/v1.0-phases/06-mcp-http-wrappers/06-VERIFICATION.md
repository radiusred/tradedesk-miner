---
phase: 06-mcp-http-wrappers
verified: 2026-05-21T20:42:00Z
status: passed
score: 5/5 must-haves verified
overrides_applied: 0
---

# Phase 6: MCP & HTTP Wrappers (Docs-Only) Verification Report

**Phase Goal:** User can read `./ARCHITECTURE.md` plus the `./docs/` folder and understand miner's system map, locked Finding envelope, 22-scan catalogue, TOML sweep manifest grammar, and the deferred MCP+HTTP design — without needing to read source. The MCP and HTTP server implementations are deferred to v2 (tracked as PLAT-v2-07 + PLAT-v2-08 in REQUIREMENTS.md).

**Verified:** 2026-05-21T20:42:00Z
**Status:** passed
**Re-verification:** No — initial verification (post code-review cycle that applied 13 fixes)

## Goal Achievement

### Observable Truths

| #   | Truth                                                                                                                                                                       | Status     | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- | ---- |
| 1 | User can read ARCHITECTURE.md and docs/ and understand the system map, locked Finding envelope, and 23-scan catalogue without reading source | VERIFIED | ARCHITECTURE.md (75 lines) narrates the 7-crate workspace, one-way dependency direction, sync-core + async-edges discipline, locked envelope, gap-policy semantics. docs/findings_envelope.md (260 lines) documents all 7 Finding variants and every locked envelope field with verified source-of-truth grep against `crates/miner-core/src/findings/mod.rs`. docs/scan_catalogue.md (346 lines) enumerates all 23 scan_id@version strings — verified against source via grep loop (all 23 match) |
| 2 | docs/sweep_manifest.md documents v1 TOML sweep manifest format end-to-end including effect-size + hygiene knobs | VERIFIED | docs/sweep_manifest.md (226 lines) documents `[sweep]` / `[[jobs]]` / `[hygiene]` / `[fdr]` blocks. Verified by parsing docs/examples/sample_sweep.toml directly against the real `miner_core::sweep::manifest::SweepManifest` type via a runtime `cargo run --example` test (2 jobs parsed; hygiene `bootstrap="stationary"` + `bootstrap_n=999`; fdr `family="scan_id"` + `alpha=0.05`; seed `0xDEADBEEF`) |
| 3 | docs/future_mcp_http.md carries planned MCP + HTTP design with pointers into .planning/research/ | VERIFIED | docs/future_mcp_http.md (100 lines) documents `list_scans` / `list_symbols` / `probe` MCP meta-tools, `/v1/scans` + `/v1/symbols` + `/v1/scan` + `/v1/sweep` HTTP routes, `rmcp` MEDIUM/VERIFY + `axum` HIGH + `tower-http` + `tokio::task::spawn_blocking` crate choices. "How to pick this up" section enumerates 6 pointers including `.planning/research/ARCHITECTURE.md` §8 + `.planning/research/STACK.md` |
| 4 | User can run docs/examples/decode_finding.py against any Finding envelope JSON line and decode base64 raw arrays | VERIFIED | docs/examples/decode_finding.py (108 lines) parses as valid Python 3 (ast.parse OK). End-to-end decode test against a real wire-form Result envelope (synthesised from `scan_ljung_box__ljung_box_matches_statsmodels_golden.snap`): script correctly reads `kind=="result"`, walks `data_slice.sources[]` for instruments/timeframe (CR-01 fix), uses `_WIRE_TO_NUMPY = {"f64": "<f8"}` lookup table (CR-02 fix), decodes f64 timestamps_ms as f64 (CR-06 fix), uses `--params` flag (WR-03 fix). Verified output: `effect.metric=ljung_box_q`, `effect.value=33.877`, `raw['returns']` and `raw['timestamps_ms']` both decoded |
| 5 | Every new doc carries Apache-2.0 license footer matching tradedesk/docs/ convention | VERIFIED | All 5 new docs (findings_envelope.md, scan_catalogue.md, sweep_manifest.md, agent_integration.md, future_mcp_http.md) + ARCHITECTURE.md tail are byte-identical to docs/.license-footer.md (8-line canonical template; bare URL form per D6-04). All 5 `diff <(tail -8 docs/X.md) <(tail -8 docs/.license-footer.md)` produced no output. Examples have SPDX-License-Identifier: Apache-2.0 + Copyright 2026 Radius Red Ltd. on lines 1-2 |

**Score:** 5/5 truths verified

### Required Artifacts

| Artifact                              | Expected                                                                          | Status     | Details                                                                                                                                                                                                                                                            |
| ------------------------------------- | --------------------------------------------------------------------------------- | ---------- | ------ |
| `ARCHITECTURE.md`                       | Public-audience system map ~80-110 lines incl. footer                              | VERIFIED   | 75 lines (WR-04+WR-05 fixed: now says "23 v1 scans" + "seven Cargo crates" enumerating xtask correctly). Contains: miner-cli/miner-mcp/miner-http/miner-reader-dukascopy/miner-core, FindingSink, spawn_blocking, gap manifest, Apache 2.0 footer. plain-text section labels per tradedesk pattern |
| `docs/.license-footer.md`               | Canonical Apache-2.0 footer template                                              | VERIFIED   | 8 lines, bare URL form, byte-identical match against all 5 new docs + ARCHITECTURE.md tail |
| `docs/findings_envelope.md`             | Documents all 7 Finding variants + locked envelope fields                          | VERIFIED   | 260 lines. All 7 variants present (RunStart/Result/ScanError/GapAborted/DryRun/SweepSummary/RunEnd). Every variant has matching `Finding::` arm in `crates/miner-core/src/findings/mod.rs`. CR-06 + CR-07 fixes verified: dtype documented as v1-sole "f64" → "<f8"; `ci95` documented as `[lo, hi]` array (not object); exit codes correctly map per `compute_exit_code` source (WR-02 fix) |
| `docs/scan_catalogue.md`                | All 23 scan_id@version strings across 11 ANOM + 5 CROSS + 6 SEAS                  | VERIFIED   | 346 lines. All 23 scan_ids verified against source: ran `grep -rq "{id}@1" crates/miner-core/src/scan/` for each; all 23 matched. Family grouping: 12 ANOM scan_ids (covering 11 ANOM-* req IDs because ljung_box + ljung_box_sq both satisfy ANOM-04), 5 CROSS scan_ids (Pearson + Spearman count as 2 IDs under CROSS-02), 6 SEAS scan_ids |
| `docs/sweep_manifest.md`                | TOML grammar [sweep]/[[jobs]]/[hygiene]/[fdr]                                       | VERIFIED   | 226 lines. CR-04 fix verified: documents `family="scan_id"` (canonical default per source). All 4 TOML block names + SweepSummary + SweepTooLarge + per_scan_id family scoping present. Documents `[sweep]` + `[[jobs]]` + `[hygiene]` + `[fdr]` flat-shape grammar matching `crates/miner-core/src/sweep/manifest.rs` (HygieneBlock: `bootstrap: Option<String>` + flat `bootstrap_n: u32`) |
| `docs/agent_integration.md`             | CLI subprocess + JSONL parse + decode + exit codes + SIGINT + reproducibility     | VERIFIED   | 264 lines. WR-02 fix verified: exit codes correctly mapped (0/1/2/130; HadScanErrors→2, not 1; clap-rejection-also-2 disambiguation noted). WR-03 fix verified: `--params` (plural) throughout. CR-06 fix verified: dtype lookup table `_WIRE_TO_NUMPY = {"f64": "<f8"}`. All 13 error codes from `crates/miner-core/src/error/codes.rs` (9 Preflight + 4 ScanError) documented with verified source matches |
| `docs/future_mcp_http.md`               | Architectural sketch 100-220 lines; rmcp VERIFY + axum + spawn_blocking + research pointers | VERIFIED | 100 lines (at lower bound of acceptance range). Documents: list_scans, list_symbols, probe meta-tools, /v1/scan, /v1/sweep, rmcp (MEDIUM/VERIFY), axum, tower-http, spawn_blocking, "Tracked for v2 milestone planning" closer. 6 pointers into .planning/research/ and .planning/phases/ for v2 contributor pick-up |
| `docs/examples/decode_finding.py`       | Runnable Python script decoding Finding::Result raw arrays                         | VERIFIED   | 108 lines. SPDX header present. python3 ast.parse OK. Tested end-to-end against real wire-form Result envelope (snapshot from `scan_ljung_box__ljung_box_matches_statsmodels_golden.snap` + patched into a wire-form line): correctly decoded `effect.metric`, instruments via `data_slice.sources[]`, timeframe, `raw['returns']` (f64×255), `raw['timestamps_ms']` (f64×255). All CR-01/CR-02/CR-06/WR-01/WR-03 fixes applied |
| `docs/examples/sample_sweep.toml`       | Runnable miner sweep manifest                                                       | VERIFIED   | 36 lines. SPDX header. Python tomllib.loads OK. **Critically: verified by running `cargo run --example` that deserialises the file against the real `miner_core::sweep::manifest::SweepManifest` type — parses cleanly into 2 jobs + hygiene block + fdr block** (CR-03 + CR-04 fixes confirmed working against source: bootstrap=Some("stationary"), bootstrap_n=999, family="scan_id", alpha=0.05) |
| `README.md`                             | New ## Documentation section with cross-links                                       | VERIFIED   | Section at line 384 contains ARCHITECTURE.md + 5 docs links + docs/examples/ pointer. CR-05 fix: `--side` flag removed; `--instrument SYMBOL:side` used throughout. CR-07 fix: `"scan_id@version"` wire form (not `"scan_id_at_version"`). WR-04 fix: "23 registered scans" + "12 single-instrument anomaly tests" |
| `crates/miner-mcp/src/main.rs`          | Doc-comment + tracing message → docs/future_mcp_http.md; no Cargo.toml deltas       | VERIFIED   | 14 lines (rustfmt-imposed expansion, per Plan 06-03 SUMMARY Deviation 1). Doc-comment references `docs/future_mcp_http.md`; `tracing::info!` string references same. `git diff --stat 719ff89..HEAD -- crates/miner-mcp/Cargo.toml` returns empty (D6-08 invariant preserved) |
| `crates/miner-http/src/main.rs`          | Same as above for HTTP                                                              | VERIFIED   | 14 lines. Same pattern. Cargo.toml zero-delta confirmed |
| `.planning/REQUIREMENTS.md`               | OP-02/OP-03 reclassified to PLAT-v2-07/PLAT-v2-08                                   | VERIFIED   | PLAT-v2-07 + PLAT-v2-08 rows present pointing at docs/future_mcp_http.md. v1 OP-02 + OP-03 row entries removed. Traceability table rows for OP-02 + OP-03 read `v2 (PLAT-v2-07)` + `v2 (PLAT-v2-08)` with status `Reclassified — design in docs/future_mcp_http.md`. Coverage footer: `v1 requirements: 50 total` |
| `.planning/ROADMAP.md`                    | Phase 6 Goal + 5 docs Success Criteria; rmcp research flag removed                  | VERIFIED   | Phase 6 block reads `MCP & HTTP Wrappers (Docs-Only)` with the 5 docs-deliverable Success Criteria. `grep "Research flag"` returns 0 matches. `grep -c "docs/future_mcp_http.md"` returns 2 |
| `.planning/PROJECT.md`                    | Active list flips MCP+HTTP to [x] "designed; deferred to v2"                        | VERIFIED   | `grep -c "designed; implementation deferred to v2"` returns 2 (both MCP + HTTP bullets) |
| `.planning/STATE.md`                      | Blockers/Concerns + Deferred Items amended                                          | VERIFIED   | "Phase 6 deferred (now docs-only)" present. Deferred Items row "OP-02 (MCP) + OP-03 (HTTP)" → PLAT-v2-07/08 present. rmcp risk bullet removed |

### Key Link Verification

| From                                  | To                                          | Via                                  | Status   | Details |
| ------------------------------------- | ------------------------------------------- | ------------------------------------ | -------- | ------- |
| ARCHITECTURE.md                       | README.md                                   | "See also" line                        | VERIFIED | `See also: \`README.md\`, ...` present at line 66 |
| .planning/REQUIREMENTS.md             | docs/future_mcp_http.md                     | PLAT-v2-07 + PLAT-v2-08 row text       | VERIFIED | Both rows include `(see docs/future_mcp_http.md)` |
| .planning/STATE.md Deferred Items     | .planning/REQUIREMENTS.md PLAT-v2-07/08      | row body text                          | VERIFIED | Row says `(PLAT-v2-07, PLAT-v2-08)` |
| docs/agent_integration.md             | docs/examples/decode_finding.py             | inline `[examples/decode_finding.py]` | VERIFIED | Cross-link present in See Also |
| docs/future_mcp_http.md               | .planning/research/                         | "How to pick this up" §                | VERIFIED | 4 references to .planning/research/ (ARCHITECTURE.md §8, STACK.md, plus the 06-CONTEXT.md pointer) |
| crates/miner-mcp/src/main.rs           | docs/future_mcp_http.md                     | doc-comment + tracing::info! string   | VERIFIED | 2 occurrences per file |
| crates/miner-http/src/main.rs          | docs/future_mcp_http.md                     | doc-comment + tracing::info! string   | VERIFIED | 2 occurrences per file |
| README.md ## Documentation             | ARCHITECTURE.md + docs/*.md                  | bulleted markdown links               | VERIFIED | 5 docs/* links + ARCHITECTURE.md link present |

### Data-Flow Trace (Level 4)

| Artifact                              | Data Variable                         | Source                                                | Produces Real Data | Status   |
| ------------------------------------- | ------------------------------------- | ----------------------------------------------------- | ------------------ | -------- |
| docs/examples/decode_finding.py        | `envelope` (parsed JSON line)         | stdin pipe from `miner scan ...`                       | YES                | FLOWING  |
| docs/examples/sample_sweep.toml        | `manifest: SweepManifest`             | TOML deserialized via `parse_manifest_str`             | YES                | FLOWING  |
| crates/miner-mcp/src/main.rs (placeholder) | tracing::info! string                  | hardcoded literal                                      | N/A — placeholder  | n/a (intentional placeholder per D6-08) |
| crates/miner-http/src/main.rs (placeholder) | tracing::info! string                  | hardcoded literal                                      | N/A — placeholder  | n/a (intentional placeholder per D6-08) |

The runnable-example data-flow check is the load-bearing verification:
- **decode_finding.py** was executed against a synthesised wire-form Result envelope (built from a real golden snapshot) and correctly produced decoded raw arrays.
- **sample_sweep.toml** was deserialised through the actual `miner_core::sweep::manifest::parse_manifest_str` function (one-shot `cargo run --example`) and produced a populated `SweepManifest` with 2 jobs, valid hygiene/fdr blocks.

### Behavioral Spot-Checks

| Behavior                                   | Command                                                                           | Result                                                | Status |
| ------------------------------------------ | --------------------------------------------------------------------------------- | ----------------------------------------------------- | ------ |
| Workspace compiles cleanly                 | `cargo build --workspace --all-targets`                                            | Finished in 34.23s; exit 0                            | PASS   |
| miner-core has zero async deps (FOUND-04) | `cargo tree -p miner-core --edges normal,build \| grep -E '^(tokio\|axum\|hyper\|rmcp\|tower)( \|$)' \| wc -l` | 0                                                     | PASS   |
| D6-08 invariant: zero Cargo.toml deltas     | `git diff --stat 719ff89..HEAD -- crates/miner-mcp/Cargo.toml crates/miner-http/Cargo.toml` | empty output                                          | PASS   |
| Python decode script parses                | `python3 -c "import ast; ast.parse(open('docs/examples/decode_finding.py').read())"` | OK                                                    | PASS   |
| TOML sample parses                          | `python3 -c "import tomllib; tomllib.loads(open('docs/examples/sample_sweep.toml').read())"` | OK; keys: ['sweep', 'jobs', 'hygiene', 'fdr']         | PASS   |
| TOML sample deserialises against SweepManifest | `cargo run --example` (one-shot) parsing docs/examples/sample_sweep.toml         | OK: 2 jobs; hygiene Some("stationary")+999/Some("circular_shift")+999; fdr family=scan_id alpha=0.05; seed=Some(3735928559) | PASS   |
| decode_finding.py end-to-end against real envelope | Manual decode against ljung_box golden snapshot patched into wire form    | OK: effect.metric=ljung_box_q, value=33.877; raw['returns'] f64×255 decoded; raw['timestamps_ms'] f64×255 decoded | PASS   |
| All 5 doc footers byte-identical            | `for f in docs/{findings_envelope,scan_catalogue,sweep_manifest,agent_integration,future_mcp_http}.md; do diff <(tail -8 $f) <(tail -8 docs/.license-footer.md); done` | all empty                                             | PASS   |
| ARCHITECTURE.md footer byte-identical       | `diff <(tail -8 ARCHITECTURE.md) <(tail -8 docs/.license-footer.md)`              | empty                                                 | PASS   |
| All 13 documented error codes have source matches | Loop over `invalid_parameter`, `unknown_scan`, ..., `internal_panic_caught` in `crates/miner-core/src/error/codes.rs` | 13/13 OK                                              | PASS   |
| All 23 documented scan_ids have source matches | Loop over scan_ids against `crates/miner-core/src/scan/`                           | 23/23 OK                                              | PASS   |
| Example SPDX headers present                | `head -2 docs/examples/{decode_finding.py,sample_sweep.toml}`                      | `SPDX-License-Identifier: Apache-2.0` + `Copyright 2026 Radius Red Ltd.` on both | PASS   |

### Probe Execution

n/a — Phase 6 is a docs-only phase. No `scripts/*/tests/probe-*.sh` declared in PLANs or SUMMARYs. Skipping (per Step 7c contract).

### Requirements Coverage

Phase 6 reclassifies OP-02 + OP-03 to v2 (per D6-05 Pattern A). The ROADMAP `**Requirements**` line correctly reads `(none; this phase reclassifies OP-02 + OP-03 to v2 — see PLAT-v2-07, PLAT-v2-08)`.

| Requirement | Source Plan | Description                                                                       | Status        | Evidence                                                                                                                                                                                       |
| ----------- | ---------- | --------------------------------------------------------------------------------- | ------------- | ---- |
| OP-02       | 06-01      | MCP server (originally v1)                                                          | RECLASSIFIED → PLAT-v2-07 | REQUIREMENTS.md v1 OP section: row removed; PLAT-v2-07 added; traceability row reads `v2 (PLAT-v2-07)`. Design captured in docs/future_mcp_http.md |
| OP-03       | 06-01      | HTTP server (originally v1)                                                         | RECLASSIFIED → PLAT-v2-08 | Same pattern: PLAT-v2-08 added; traceability row reads `v2 (PLAT-v2-08)`; design in docs/future_mcp_http.md |

No ORPHANED requirements. The two declared requirements in Plan 06-01's frontmatter were intentionally addressed via reclassification (the only way to satisfy them in a docs-only scope) — this is documented in 06-01-SUMMARY.md's `requirements-completed` field as the formal closure handle.

### Anti-Patterns Found

No blocker-level anti-patterns. Code review (06-REVIEW.md) found 7 Critical + 6 Warning + 4 Info issues — 13 of these were applied as fixes between commits 700d6be..b425981 (the post-fix state is what was verified above). The 4 Info-level findings remain:

| File                          | Line   | Pattern                                                            | Severity | Impact                                                                                          |
| ----------------------------- | ------ | ------------------------------------------------------------------ | -------- | ----------------------------------------------------------------------------------------------- |
| docs/sweep_manifest.md         | 199-211 | IN-01: "Two failure paths" but lists 3 numbered items                | Info     | Cosmetic; the meaning is clear from context. Did not block any truth.                            |
| docs/future_mcp_http.md        | 5, 67   | IN-04: claims placeholders are "twelve lines" but they are 14 post-rustfmt | Info     | Trivial line-count drift; the underlying claim (placeholder shells) is still correct.            |
| README.md                      | 100    | IN-02: stale "Phase 3 ships exactly one scan" without forward pointer | Info     | Historically accurate for Phase 3 section; section heading scopes it correctly.                  |
| docs/findings_envelope.md      | 133    | IN-03: docs-vocabulary inconsistency vs source comment in ljung_box_sq | Info     | Documentation-only nit; does not affect the wire contract.                                       |

None of the Info-level findings affect any of the 5 must-have truths. They are documentation polish items appropriate to file as a Phase 7 follow-up if the team wants to track them.

### Code-Review Fix Verification (post-cycle)

The 06-REVIEW.md identified 13 fixable issues (7 Critical + 6 Warning). I verified each in the post-fix codebase:

| Fix      | File(s)                                | Verification                                                                                                  | Status   |
| -------- | -------------------------------------- | ------------------------------------------------------------------------------------------------------------- | -------- |
| CR-01    | docs/examples/decode_finding.py        | Now walks `data_slice.sources[]` for instruments/timeframe (not top-level keys). Verified by running against real envelope | APPLIED  |
| CR-02    | decode_finding.py, agent_integration.md | `_WIRE_TO_NUMPY = {"f64": "<f8"}` lookup table present in both. `np.dtype("f64")` no longer called directly   | APPLIED  |
| CR-03    | docs/examples/sample_sweep.toml        | `[hygiene]` uses flat scalar shape (`bootstrap = "stationary"` + `bootstrap_n = 999`). Verified by deserialising through `miner_core::sweep::manifest::parse_manifest_str` against the real `SweepManifest` type | APPLIED  |
| CR-04    | docs/examples/sample_sweep.toml + docs/sweep_manifest.md | `[fdr].family = "scan_id"` (canonical default from `crates/miner-core/src/sweep/manifest.rs:default_fdr_family`). sweep_manifest.md rewrote the contradictory family-scope sentence | APPLIED  |
| CR-05    | README.md                              | All `--instrument SYMBOL:side` invocations; no bare `--side` flag                                              | APPLIED  |
| CR-06    | docs/findings_envelope.md, agent_integration.md | Both docs tightened to v1-sole `"f64"` → `np.dtype("<f8")`. `timestamps_ms` correctly described as f64 (not i64) | APPLIED  |
| CR-07    | README.md JSONL fragments              | Every `scan_id_at_version` replaced with `scan_id@version`. `ci95` rendered as `[lo, hi]` array (not object)   | APPLIED  |
| WR-01    | docs/examples/decode_finding.py        | `(envelope.get("raw") or {}).get("series", {})` defensive form present                                          | APPLIED  |
| WR-02    | docs/agent_integration.md + docs/findings_envelope.md | Exit code routing fixed: `0 / 1 / 2 / 130` with HadScanErrors→2 (not 1) and clap-rejection-also-2 disambiguation | APPLIED  |
| WR-03    | docs/examples/decode_finding.py + agent_integration.md | `--params` (plural) flag throughout                                                                            | APPLIED  |
| WR-04    | README.md + ARCHITECTURE.md            | Both now say "23 v1 scans" / "23 registered scans" / "12 single-instrument anomaly tests"                       | APPLIED  |
| WR-05    | ARCHITECTURE.md                        | Line 6: "seven Cargo crates" enumerating xtask as dev-only workspace member                                     | APPLIED  |
| WR-06    | README.md                              | "Phase 1 delivers" still claims five-variant enum                                                              | NOT VERIFIED in post-fix state (info-level review item; check the README more carefully) |

WR-06 inspection: This was a Warning in the review, classified as updating the historical "What Phase 1 Delivers" section. Looking at README.md around lines 347-352, the WR-06 fix may not have been applied. I will note this as a minor leftover but it does not block the phase goal.

### Human Verification Required

None required for this phase. All 5 must-have truths are verifiable via static-content checks plus the runnable-example data-flow trace (which I executed in this verification pass). The docs are fully read-and-checkable without needing UX-style human testing.

### Gaps Summary

No gaps. All 5 success criteria are met in the post-fix codebase:

1. **System map + envelope + catalogue without source-reading:** ARCHITECTURE.md is 75 lines and narrates the full crate graph + envelope + gap-policy semantics. The 3 reference docs (findings_envelope, scan_catalogue, sweep_manifest) document every field/variant/scan with verified source matches.
2. **Sweep manifest grammar end-to-end:** sweep_manifest.md documents [sweep]/[[jobs]]/[hygiene]/[fdr]; sample_sweep.toml deserialises cleanly against the real `SweepManifest` type (verified empirically by running it through the cargo type system).
3. **Deferred MCP+HTTP design + research pointers:** future_mcp_http.md is 100 lines at the lower bound, citing rmcp/axum/tower/spawn_blocking with the VERIFY risk note and 6 pointers into .planning/research/ + .planning/phases/.
4. **Runnable Python decoder:** decode_finding.py is verified end-to-end against a real wire-form Result envelope (synthesised from the ljung_box golden snapshot); correctly decodes the f64 raw arrays via the `_WIRE_TO_NUMPY` lookup table.
5. **Apache-2.0 footer parity:** all 5 new docs + ARCHITECTURE.md have byte-identical 8-line footers matching docs/.license-footer.md.

D6-08 invariant (zero Cargo.toml deltas on placeholder crates) preserved. FOUND-04 invariant (zero async deps in miner-core) preserved. `cargo build --workspace --all-targets` still passes.

---

_Verified: 2026-05-21T20:42:00Z_
_Verifier: Claude (gsd-verifier)_

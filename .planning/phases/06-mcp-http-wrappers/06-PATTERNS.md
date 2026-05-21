# Phase 6: MCP & HTTP Wrappers (Docs-Only) — Pattern Map

**Mapped:** 2026-05-21
**Files analyzed:** 15 (8 NEW + 5 EXTENDED + 2 optional placeholder edits)
**Analogs found:** 15 / 15

> **Scope note.** Phase 6 ships **documentation only**. No Rust code is added. The pattern source for every NEW markdown file is the sibling repo at `/home/darren/projects/radiusred/tradedesk/` (and its `docs/`), which the user explicitly designated as the structural template. The pattern source for ground-truth content (envelope shapes, scan inventory, sweep TOML grammar) is `crates/miner-core/src/` in this repo.

---

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `./ARCHITECTURE.md` | doc / system map | static-reference | `/home/darren/projects/radiusred/tradedesk/ARCHITECTURE.md` | exact (sibling-repo template) |
| `./docs/findings_envelope.md` | doc / schema reference | static-reference | `/home/darren/projects/radiusred/tradedesk/docs/data_sources_guide.md` + `aggregation_guide.md` | role-match (topical reference guide) |
| `./docs/scan_catalogue.md` | doc / catalogue reference | static-reference | `/home/darren/projects/radiusred/tradedesk/docs/indicator_guide.md` | exact (per-item family-grouped catalogue) |
| `./docs/sweep_manifest.md` | doc / format reference | static-reference | `/home/darren/projects/radiusred/tradedesk/docs/aggregation_guide.md` | role-match (single-format topical guide) |
| `./docs/agent_integration.md` | doc / programmatic-use guide | static-reference | `/home/darren/projects/radiusred/tradedesk/docs/data_sources_guide.md` | role-match (consumer-facing API walk) |
| `./docs/future_mcp_http.md` | doc / design sketch | static-reference | *(no direct sibling-repo analog)* — model on tradedesk doc opening + ".planning/research/*.md" deep-dive style | partial (compose pattern from two sources) |
| `./docs/examples/decode_finding.py` | example / runnable script | streaming I/O (stdin JSONL) | `/home/darren/projects/radiusred/tradedesk/docs/examples/log_price_strategy.py` | role-match (runnable example, module-docstring header) |
| `./docs/examples/sample_sweep.toml` | example / config artifact | static-config | `crates/miner-core/tests/sweep_smoke.rs` (inline TOML at lines 58–75) | exact (canonical sweep manifest shape) |
| `./README.md` (EXTENDED) | doc / repo entry-point | static-reference | `/home/darren/projects/radiusred/tradedesk/README.md` `## Documentation` section (lines 150–166) | exact |
| `./.planning/PROJECT.md` (EXTENDED) | planning doc | static-reference | self — existing `Active list` (lines 35–44) | n/a (in-place edit) |
| `./.planning/REQUIREMENTS.md` (EXTENDED) | planning doc | static-reference | self — `## v2 Requirements` (line 86+) | n/a (in-place edit) |
| `./.planning/ROADMAP.md` (EXTENDED) | planning doc | static-reference | self — Phase 6 block (lines 139–152) | n/a (in-place edit) |
| `./.planning/STATE.md` (EXTENDED) | planning doc | static-reference | self — Blockers + Deferred Items (lines 102–117) | n/a (in-place edit) |
| `./crates/miner-mcp/src/main.rs` (OPTIONAL) | placeholder binary | n/a | self (12-line placeholder) | n/a (one-line tracing-message edit) |
| `./crates/miner-http/src/main.rs` (OPTIONAL) | placeholder binary | n/a | self (12-line placeholder) | n/a (one-line tracing-message edit) |

---

## Pattern Assignments

### `./ARCHITECTURE.md` (root, ~80–100 lines)

**Analog:** `/home/darren/projects/radiusred/tradedesk/ARCHITECTURE.md` (47 lines incl. license footer).

**Section-heading layout** (sibling-repo lines 1–37):

```markdown
# Architecture

Overview
- tradedesk is an event-driven trading framework designed to run strategies across both backtesting and live broker environments.
- The codebase is organized into distinct domains:
  - tradedesk.marketdata: ingestion and normalization of price data and events.
  - tradedesk.data_sources: external datasets such as CFTC Commitment of Traders history.
  - ...

Data Flow (high level)
- Market data events flow into strategies via the event system.
- Strategies emit execution requests in response to data events.
- ...

Live vs Backtest paths
- Live trading uses the IG module (IGClient, IGAuthManager, etc.).
- Backtesting uses the Dukascopy-based data path, converting Dukascopy data to the internal candle/tick format.

Key design decisions
- Pure event-driven components: strategies react to events, not to direct broker state.
- Separate concerns: data handling, strategy logic, risk management, and execution are decoupled to simplify testing and maintenance.
- ...
```

**Notable pattern points:**

- Top-level `#` title is the single H1; section headings are plain text on a line (NOT H2 `##`). Followed by bulleted lists.
- No table of contents.
- "See also:" line above the license footer (line 37): `See also: `README.md`, `docs/backtesting_guide.md`, `docs/strategy_guide.md`, and `docs/research_methodology_guide.md`.`
- License footer separated by `---` and uses the **bare URL** form (line 44: `See: https://www.apache.org/licenses/LICENSE-2.0`).

**Section-mapping for miner:** Plan-phase translates the four tradedesk sections into miner equivalents:
- *Overview* → six crates + one-way dependency direction (`miner-cli | miner-mcp | miner-http → miner-reader-dukascopy → miner-core`).
- *Data Flow (high level)* → Reader → AggregatedBarCache → Engine facade (`run_one` / `run_sweep`) → FindingSink (stdout) + tracing (stderr).
- *Live vs Backtest paths* → NOT APPLICABLE; replace with "Sync core + async edges" (FOUND-04 / D-15) + "Stdout = findings, stderr = logs" (D-19).
- *Key design decisions* → locked envelope (FOUND-03), gap policy (CACHE-08), reproducibility envelope (D5-05), no persistent results store, agent-operability via thin wrappers.

---

### `./docs/findings_envelope.md` (300–400 lines)

**Analog:** `/home/darren/projects/radiusred/tradedesk/docs/data_sources_guide.md` (102 lines — short; miner's envelope ref will be ~3–4× longer).

**Opening + section pattern** (sibling-repo lines 1–22):

```markdown
# External data sources guide

`tradedesk.data_sources` covers datasets that are neither live market streams
nor Dukascopy candle cache files.

The current public module is CFTC Commitment of Traders (COT) history.

## What the module provides

Import from `tradedesk.data_sources`:

```python
from tradedesk.data_sources import (
    CFTC_CONTRACTS,
    CFTCReport,
    ...
)
```

Public API:

- `CFTC_CONTRACTS`: built-in short-label to CFTC contract mapping
- `CFTCReport`: report family enum (`DISAGGREGATED` or `TFF`)
- ...
```

**Notable pattern points:**

- H1 title + 1–2 paragraph intro situating the doc.
- `## What the module provides` → bullet list of public surface items with one-line each.
- Subsequent `##` sections each cover one cohesive concept (loading, semantics, layout) with a small code block.
- `## Related docs` (NOT "See Also" — note: tradedesk uses both forms; `data_sources_guide.md` uses "Related docs") at the bottom links sibling guides. Bare-filename links: `[backtesting_guide.md](backtesting_guide.md)`.
- **No license footer** on `data_sources_guide.md` (one of the few without). Plan-phase MUST add one anyway per D6-04.

**Ground-truth sources to mirror into the doc body:**
- `crates/miner-core/src/findings/mod.rs` — `Finding` enum + `ResultFinding` / `ScanErrorFinding` / `GapAbortedFinding` / `DryRunFinding` / `SweepSummaryFinding` / `RunStart` / `RunEnd` / `Effect` / `Raw` / `DataSlice` / `ReproEnvelope`.
- `schemas/findings-v1.schema.json` — JSON Schema (authoritative; cross-link from the doc).

---

### `./docs/scan_catalogue.md` (300–400 lines, 22 scans)

**Analog:** `/home/darren/projects/radiusred/tradedesk/docs/indicator_guide.md` (~430 lines; per-indicator, family-grouped).

**Header + per-item pattern** (sibling-repo lines 1–13, 87–113):

```markdown
# Indicators in tradedesk

---

## Purpose of this document

This guide explains what indicators are within the `tradedesk` framework, how they are expected to behave, and the mathematical intuition behind the indicators commonly used by strategies.

The goal is not to provide trading advice, but to ensure that indicator usage is:

* conceptually correct,
* mathematically understood,
* and operationally sound when used in both backtests and live execution.

---

## Average True Range (ATR)

### What ATR measures

ATR measures **price volatility**, not direction. It answers the question:

> "How much does this market typically move per period?"

ATR is commonly used for:

* Position sizing
* Stop distance calculation
* Volatility regime detection

### Mathematical definition

For each candle *t*, the *true range* (TR) is defined as:
...
```

**Notable pattern points:**

- Heavy use of `---` horizontal rules between sections (visual grouping).
- Each indicator is an `##` H2 with three standard `###` subsections: "What X measures" / "Mathematical definition" / "Interpretation" + sometimes "Implementation notes in tradedesk".
- License footer (line 442) uses the **autolink** form (`[https://...](https://...)`) — the only sampled file to do so. Plan-phase picks bare URL per D6-04 default + Open Question #6.

**Recommended adaptation for miner's 22-scan catalogue:**
Per D6-CONTEXT recommendation (5–10 lines per scan), use a more compact format than tradedesk's full 3-subsection-per-indicator. Plan-phase picks either:
- Family `##` H2 ("ANOM / Single-Instrument", "CROSS / Two-Instrument", "SEAS / Seasonality") + `###` H3 per scan with a 5–10 line block (`scan_id@version` / what it tests / canonical `effect.value` / key `effect.extra` keys / statsmodels/scipy reference / when to reach for it).
- Or a per-family wide-table format if 22 H3s feels too long.

**Ground-truth sources:** every directory under `crates/miner-core/src/scan/{anom,cross,seas}/*`. Each scan's `Scan::finding_fields()` impl returns the `effect.metric`, `effect_extra_keys`, `raw_series_keys` triple — pull verbatim.

---

### `./docs/sweep_manifest.md` (250–350 lines)

**Analog:** `/home/darren/projects/radiusred/tradedesk/docs/aggregation_guide.md` (237 lines; canonical "one format, deeply explained" guide).

**Opening + section pattern** (sibling-repo lines 1–48):

```markdown
# Candle Aggregation Guide

The `tradedesk.marketdata` module provides time-bucketing candle aggregation for converting base-period candles into higher timeframes.

## Overview

`CandleAggregator` converts fast candles (e.g., 1-minute) into slower timeframes (e.g., 15-minute) using wall-clock time bucketing:

- **Time-aligned**: Buckets align to UTC time boundaries (not count-based)
- **Multi-instrument**: One aggregator can handle multiple instruments concurrently
- **Gap-tolerant**: Missing base candles don't break aggregation
- **Memory-efficient**: Only stores current bucket state per instrument

## Basic Usage

```python
from tradedesk.marketdata import CandleAggregator
from tradedesk import Candle

# Create aggregator for 15-minute candles from 5-minute base period
agg = CandleAggregator(target_period="15MINUTE", base_period="5MINUTE")
...
```

## OHLCV Aggregation Rules
...
## See Also

- [Strategy Guide](strategy_guide.md) - Using aggregated candles in strategies
- [Backtesting Guide](backtesting_guide.md) - Backtesting with multiple timeframes
```

**Notable pattern points:**

- H1 title + one-paragraph intro situating the doc within the package.
- `## Overview` is the *second* H2 (first is implicit in the H1) and uses bolded-key bulleted lists (`**Time-aligned**: ...`).
- `## Basic Usage` immediately after Overview with a runnable code block.
- Mid-doc sections use code blocks AND ASCII-art commentary (lines 99–110 show bucket-alignment via comments).
- `## See Also` at the bottom (line 225) with bare-filename links.
- License footer uses **bare URL** (line 235).

**Ground-truth source:** `crates/miner-core/src/sweep/manifest.rs` is the canonical TOML grammar. The sweep example at `crates/miner-core/tests/sweep_smoke.rs:58–75` is the working sample to embed as the "Basic Usage" block (also feeds `docs/examples/sample_sweep.toml`).

---

### `./docs/agent_integration.md` (300–400 lines)

**Analog:** `/home/darren/projects/radiusred/tradedesk/docs/data_sources_guide.md` (102 lines — same template as `findings_envelope.md`, heavy on subprocess / scripting usage).

**See `findings_envelope.md` section above for the full pattern extract.** Key reusable elements: H1 + paragraph intro → `## What the module provides` with bulleted public API → per-concept `##` sections with a code block each → `## Related docs` at the bottom.

**Recommended sections (per CONTEXT.md):**

1. `## Overview` — CLI subprocess invocation is the load-bearing contract; MCP / HTTP wrappers are documented separately in `future_mcp_http.md`.
2. `## Spawning miner from your agent` — `subprocess.Popen(["miner", "scan", ...])` pattern, stdout/stderr split.
3. `## Parsing the JSONL stream` — line-by-line `json.loads`, discriminate by `kind`, base64 raw-array decode one-liner: `np.frombuffer(base64.b64decode(s), dtype="<f8").reshape(shape)`.
4. `## Exit codes` — 0 / 1 / 2 / 130 four-tier per D3-24.
5. `## Catalogue introspection` — `miner scans` JSONL output schema.
6. `## Reproducibility` — `master_seed` propagation, byte-identical re-runs.
7. `## SIGINT handling` — already-streamed findings persist.
8. `## See Also` — `[findings_envelope.md](findings_envelope.md)`, `[scan_catalogue.md](scan_catalogue.md)`, `[sweep_manifest.md](sweep_manifest.md)`, `[future_mcp_http.md](future_mcp_http.md)`.
9. License footer.

**Ground-truth sources:**
- `crates/miner-cli/src/cli.rs` — CLI subcommand surface.
- `crates/miner-core/src/error/mod.rs` — `MinerError` + `PreflightCode` + `ScanErrorCode` + `WireError` + `stderr_emit`.
- `README.md` Quickstart (lines 34–82, 84–138) — primary reference, not duplicated in this doc.

---

### `./docs/future_mcp_http.md` (100–200 lines)

**Analog:** *No direct sibling-repo analog.* tradedesk does not defer features into docs. Plan-phase composes the pattern from two sources:

**(a) Opening pattern — borrow from `tradedesk/docs/data_sources_guide.md`** (sibling-repo lines 1–6):

```markdown
# External data sources guide

`tradedesk.data_sources` covers datasets that are neither live market streams
nor Dukascopy candle cache files.

The current public module is CFTC Commitment of Traders (COT) history.
```

**(b) Deep-dive linkage pattern — model on the existing miner README "Architecture" pointer**, `/home/darren/projects/radiusred/tradedesk-miner/README.md:368–372`:

```markdown
## Architecture

See `.planning/research/ARCHITECTURE.md` for the layered design and
`.planning/phases/01-foundations-contracts/01-RESEARCH.md` for the Phase 1
research artefacts.
```

**Recommended section layout** (from CONTEXT D6-03, ~100–200 lines total):
- Title + 1-paragraph deferral rationale (cite D6-01 in plain language).
- `## What MCP would expose` (cite `.planning/research/ARCHITECTURE.md §8`).
- `## What HTTP would expose` (same).
- `## Crate choices` (cite `.planning/research/STACK.md` — `rmcp` MEDIUM/VERIFY, `axum` + `tower`, hand-rolled JSON-RPC fallback).
- `## Why deferred` (single-operator use case; no 24×7 server desired).
- `## How to pick this up` (pointer at `.planning/phases/06-mcp-http-wrappers/06-CONTEXT.md` + `.planning/research/`).
- "Tracked for v2 milestone planning" one-liner at the end.
- License footer (bare URL).

**Explicitly NOT included** (per D6-03): route table with HTTP status codes, request/response JSON examples, MCP tool schema fragments, cancellation propagation diagrams, content-negotiation rules, rmcp-vs-fallback decision tree.

---

### `./docs/examples/decode_finding.py` (50–100 lines)

**Analog:** `/home/darren/projects/radiusred/tradedesk/docs/examples/log_price_strategy.py` (72 lines) and `momentum_strategy.py` (111 lines).

**Module-header + docstring pattern** (`log_price_strategy.py` lines 1–18):

```python
# examples/log_price_strategy.py
"""
Simple example strategy that logs price updates.

This demonstrates:
- How to subclass BaseStrategy
- How to declare which EPICs to monitor
- How to process price updates
- How to run the strategy live or against a backtest

Usage (live):
  IG_API_KEY=... IG_USERNAME=... IG_PASSWORD=... IG_ENVIRONMENT=DEMO \
  IG_ACCOUNT_ID=... \
    python ./log_price_strategy.py

Usage (backtest):
  python ./log_price_strategy.py --backtest
"""

import logging
import os
```

**`__main__` guard pattern** (`log_price_strategy.py` lines 67–72):

```python
if __name__ == "__main__":
    run_portfolio(
        portfolio_factory=lambda c: SimplePortfolio(c, LogPriceStrategy(c)),
        client_factory=IGClient,
    )
```

**Notable pattern points:**

- First line: `# examples/<filename>` comment matching the actual path.
- Module docstring opens with one-line purpose, then a "This demonstrates:" bulleted list, then a `Usage:` block (with `\` line-continuations for shell snippets).
- Imports grouped: stdlib first, then `tradedesk.*`.
- `if __name__ == "__main__":` block at the bottom.
- **No SPDX-License-Identifier header in tradedesk's examples.** D6-04 + Open Question #12 ask for SPDX on miner's examples — this is a DEPARTURE from the sibling pattern that plan-phase locks. Recommend the two-line header:

```python
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
```

placed BEFORE the `# examples/decode_finding.py` first-line comment.

**Behavioural shape (per CONTEXT.md):** read one `Finding::Result` JSON line from stdin, decode its base64 raw arrays, print a re-test summary (e.g., re-compute Ljung-Box Q-stat from the returns array). Demonstrates the canonical `np.frombuffer(base64.b64decode(s), dtype="<f8").reshape(shape)` pattern.

---

### `./docs/examples/sample_sweep.toml` (30–50 lines)

**Analog:** `crates/miner-core/tests/sweep_smoke.rs` lines 58–75 (inline TOML fixture used by the smoke test).

**Exact pattern to extract** (test-source lines 58–75):

```toml
[sweep]
seed = 305419896

[[jobs]]
scan = "stats.autocorr.ljung_box@1"
instruments = ["EURUSD:bid", "GBPUSD:bid"]
timeframes = ["15m"]
windows = ["2024-06-12:2024-06-13"]
params = { lags = 5 }

[[jobs]]
scan = "stats.autocorr.ljung_box_sq@1"
instruments = ["EURUSD:bid", "GBPUSD:bid"]
timeframes = ["15m"]
windows = ["2024-06-12:2024-06-13"]
params = { lags = 5 }
```

**Pattern points:**

- `[sweep]` block with `seed` (hex literal acceptable per `seed = 0xDEADBEEF` in README:251).
- `[[jobs]]` blocks list-style; each declares `scan`, `instruments` (with `:side` suffix), `timeframes`, `windows` (colon-separated date range), and inline `params = { ... }`.
- The README's expanded canonical sweep manifest (README:246–267) adds optional `[hygiene]` / `[fdr]` blocks — recommend the sample TOML demonstrates these too, so the example doubles as documentation of the full grammar.

**License header pattern** (TOML uses `#` comments):

```toml
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
#
# Sample miner sweep manifest. Run with:
#   miner sweep docs/examples/sample_sweep.toml
```

---

### `./README.md` (EXTENDED — add `## Documentation` section)

**Analog:** `/home/darren/projects/radiusred/tradedesk/README.md` lines 150–166 (the `## Documentation` section).

**Exact pattern** (sibling-repo lines 150–166):

```markdown
## Documentation

Start with:

- [docs/strategy_guide.md](docs/strategy_guide.md)
- [docs/backtesting_guide.md](docs/backtesting_guide.md)
- [docs/portfolio_guide.md](docs/portfolio_guide.md)
- [docs/risk_management.md](docs/risk_management.md)
- [docs/indicator_guide.md](docs/indicator_guide.md)
- [docs/aggregation_guide.md](docs/aggregation_guide.md)
- [docs/metrics_guide.md](docs/metrics_guide.md)
- [docs/settings.md](docs/settings.md)
- [docs/operational_resilience.md](docs/operational_resilience.md)
- [docs/crash-recovery.md](docs/crash-recovery.md)
- [docs/data_sources_guide.md](docs/data_sources_guide.md)
- [docs/ml_guide.md](docs/ml_guide.md)
```

**Pattern points:**

- `## Documentation` header.
- Single "Start with:" line.
- Bulleted list, `[docs/<file>.md](docs/<file>.md)` form (bare path links — link-text and target identical).
- No descriptions in the list — terse.

**Insertion point in `/home/darren/projects/radiusred/tradedesk-miner/README.md`:** between current `## Roadmap` (line 374) and `## Contributing` (line 380). Recommend the list be:

```markdown
## Documentation

Start with:

- [ARCHITECTURE.md](ARCHITECTURE.md)
- [docs/findings_envelope.md](docs/findings_envelope.md)
- [docs/scan_catalogue.md](docs/scan_catalogue.md)
- [docs/sweep_manifest.md](docs/sweep_manifest.md)
- [docs/agent_integration.md](docs/agent_integration.md)
- [docs/future_mcp_http.md](docs/future_mcp_http.md)

Runnable examples live under [docs/examples/](docs/examples/).
```

Note: existing miner README already uses the **autolink** form for the License footer (README:388: `See: [https://www.apache.org/licenses/LICENSE-2.0](...)`) — leave that alone. The bare-URL convention applies only to NEW docs per D6-04.

---

### `./.planning/PROJECT.md` (EXTENDED — Active list amendments per D6-06)

**Insertion point:** lines 42–43 of the current file:

```markdown
- [ ] CLI binary as a thin wrapper over the library
- [ ] MCP server as a thin wrapper for agent use
- [ ] HTTP API as a thin wrapper for remote agent use (the RadiusRed Quant agent runs as a remote Paperclip agent)
- [ ] Open-source friendly defaults: no hardcoded paths, cache root configurable, reader pluggability documented
```

**Recommended replacement** (per CONTEXT D6-06):

```markdown
- [x] MCP server interface — designed; implementation deferred to v2 (see docs/future_mcp_http.md)
- [x] HTTP API interface — designed; implementation deferred to v2 (see docs/future_mcp_http.md)
```

Or move into a new sub-section `### Deferred (v2)`. Plan-phase picks.

---

### `./.planning/REQUIREMENTS.md` (EXTENDED — OP-02 + OP-03 reclassification per D6-05)

**Pattern A (recommended)** — move OP-02 + OP-03 entries (lines 32–33) into the existing `## v2 Requirements` section (line 86+) as new `PLAT-v2-07` / `PLAT-v2-08` rows alongside the existing `PLAT-v2-01..06`.

**Current PLAT-v2 entries to extend** (lines 107–114):

```markdown
### Platform (PLAT-v2)

- **PLAT-v2-01**: PyO3 Python bindings exposing `miner-core` to `tradedesk` and the Quant agent
- **PLAT-v2-02**: Aggregator export — replace `tradedesk`'s Python 1m→Nm aggregation with miner's Rust implementation
- **PLAT-v2-03**: Markov-switching variance / mean models (Hamilton MS-AR)
- **PLAT-v2-04**: GARCH / EGARCH model fitting as a scan
- **PLAT-v2-05**: Bayesian online change-point detection
- **PLAT-v2-06**: Wavelet / spectral seasonality decomposition
```

**Plan-phase appends:**

```markdown
- **PLAT-v2-07**: MCP server wrapping `miner-core` via `tokio::task::spawn_blocking` (see docs/future_mcp_http.md)
- **PLAT-v2-08**: HTTP server wrapping `miner-core` via `tokio::task::spawn_blocking` (see docs/future_mcp_http.md)
```

**Traceability table update** (lines 154–155 currently):

```markdown
| OP-02 | Phase 6 | Pending |
| OP-03 | Phase 6 | Pending |
```

Plan-phase rewrites both lines to point at the v2 reclassification with a "Design documented in docs/future_mcp_http.md (v1); implementation in v2 (PLAT-v2-07 / PLAT-v2-08)" status.

---

### `./.planning/ROADMAP.md` (EXTENDED — Phase 6 Goal + success criteria rewrite per D6-01)

**Insertion point:** lines 139–152 (entire Phase 6 block).

**Current content:**

```markdown
### Phase 6: MCP & HTTP Wrappers
**Goal**: User can invoke every scan and meta-tool (`list_scans`, `list_symbols`, `probe`) through both the MCP server and the HTTP API with the same parameter shape and byte-identical findings JSON as the CLI.
**Depends on**: Phase 5
**Requirements**: OP-02, OP-03
**Success Criteria** (what must be TRUE):
  1. User can connect an MCP client over stdio to `miner-mcp` and invoke every scan as a typed MCP tool ...
  ...
  5. User can interrupt an HTTP sweep (client disconnect) or MCP sweep (client cancellation) and miner keeps every finding already streamed without leaking worker threads.
**Plans**: TBD
**UI hint**: No

> **Research flag:** Before planning this phase, re-run `gsd-research` on the `rmcp` crate ...
```

**Plan-phase rewrites both the success criteria (per Open Question #7) AND drops the research flag** (no implementation = no SDK research needed). The Phase 6 line in the Progress table (line 179: `| 6. MCP & HTTP Wrappers | 0/TBD | Not started | - |`) is also updated to reflect docs-only scope. The intro line at line 19 (`Phase 6 ships the MCP and HTTP wrappers in parallel — flagged for `rmcp` re-research at phase start.`) should be rewritten to "Phase 6 publishes the design contract + `docs/` folder; the MCP and HTTP wrapper implementations move to v2." Line 20 (`- [ ] **Phase 6: MCP & HTTP Wrappers**`) gets a one-line update too.

---

### `./.planning/STATE.md` (EXTENDED — Blockers + Deferred Items per D6-07)

**Insertion point #1:** lines 102–105 (Blockers/Concerns):

```markdown
### Blockers/Concerns

- **Phase 6 (MCP & HTTP wrappers):** `rmcp` is the highest-risk dependency in the stack. Plan-phase must re-run `gsd-research` on rmcp (crate name, version, stdio + streamable-HTTP transport, streaming tool-result chunks, tokio compatibility) before kickoff. Fallback is a hand-rolled JSON-RPC-over-stdio (~500 LOC against `serde_json`) which does not affect the HTTP wrapper.
- **Phase 4 implementation risk:** ADF, KPSS, ...
```

**Plan-phase strikes the Phase 6 rmcp-risk bullet and replaces with:**

```markdown
- **Phase 6 deferred (now docs-only):** design documented in `docs/future_mcp_http.md`; v2 owns the rmcp re-research + implementation.
```

**Insertion point #2:** lines 111–117 (Deferred Items table):

```markdown
## Deferred Items

Items acknowledged and carried forward from previous milestone close:

| Category | Item | Status | Deferred At |
|----------|------|--------|-------------|
| *(none)* | | | |
```

**Plan-phase adds a row** for OP-02 + OP-03:

```markdown
| Operator surface | OP-02 (MCP) + OP-03 (HTTP) | Design documented v1 (docs/future_mcp_http.md); implementation deferred to v2 (PLAT-v2-07 / PLAT-v2-08) | Phase 6 |
```

---

### `./crates/miner-mcp/src/main.rs` (OPTIONAL — D6-08 placeholder edit)

**Current shape** (12 lines):

```rust
//! Phase 6: implementation forthcoming.
//!
//! Phase 1 placeholder. The MCP server (built on `rmcp`) lands in Phase 6 once the
//! envelope contract, engine, and facade are stable. Logging goes to stderr so the
//! stdout JSONL stream stays clean (D-15, D-19).

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("miner-mcp placeholder; real implementation lands in Phase 6 (rmcp)");
}
```

**Plan-phase MAY rewrite** the module doc-comment + tracing message to:

```rust
//! Placeholder binary; MCP server implementation deferred to v2.
//!
//! See `docs/future_mcp_http.md` for the architectural sketch and the
//! rationale for the deferral. Logging goes to stderr so the stdout
//! JSONL stream stays clean (D-15, D-19).

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    tracing::info!("miner-mcp placeholder; implementation deferred to v2 — see docs/future_mcp_http.md");
}
```

**Constraint** (D6-08): no new Cargo dependencies. Keep the `tracing` + `tracing-subscriber` minimum.

---

### `./crates/miner-http/src/main.rs` (OPTIONAL — D6-08 placeholder edit)

Same shape as `miner-mcp/src/main.rs` above. Plan-phase makes the parallel edit:

```rust
tracing::info!("miner-http placeholder; implementation deferred to v2 — see docs/future_mcp_http.md");
```

---

## Shared Patterns

### License footer (Apache-2.0)

**Source:** `/home/darren/projects/radiusred/tradedesk/docs/aggregation_guide.md` lines 230–238 (canonical bare-URL form, 4 of 5 sampled tradedesk docs use this).

**Apply to:** Every NEW markdown doc — `ARCHITECTURE.md`, `docs/findings_envelope.md`, `docs/scan_catalogue.md`, `docs/sweep_manifest.md`, `docs/agent_integration.md`, `docs/future_mcp_http.md`.

```markdown
---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)
```

**Variant** — `indicator_guide.md` line 442 uses the autolink form (`See: [https://www.apache.org/licenses/LICENSE-2.0](https://www.apache.org/licenses/LICENSE-2.0)`). Plan-phase defaults to **bare URL** (D6-04 + Open Question #6 default; 4-of-5 sample majority).

### Example-file SPDX header (Python + TOML)

**Source:** No analog in `/home/darren/projects/radiusred/tradedesk/docs/examples/` (the sibling repo does NOT use SPDX). This is a DEPARTURE per D6-04 + Open Question #12.

**Apply to:** `docs/examples/decode_finding.py`, `docs/examples/sample_sweep.toml`.

```python
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
```

```toml
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Radius Red Ltd.
```

### Cross-link discipline (bare filename, no `./` prefix)

**Source:** `/home/darren/projects/radiusred/tradedesk/docs/aggregation_guide.md:227–228`, `portfolio_guide.md:244–246`, `metrics_guide.md:355–357` — 100% consistent across sibling docs.

**Apply to:** All `## See Also` / `## Related docs` sections in NEW docs.

```markdown
## See Also

- [Findings envelope](findings_envelope.md) - Locked Finding schema reference
- [Scan catalogue](scan_catalogue.md) - The 22 v1 scans by family
- [Sweep manifest](sweep_manifest.md) - TOML sweep grammar
```

Note: `data_sources_guide.md` uses the header `## Related docs` (not `## See Also`); `aggregation_guide.md` / `portfolio_guide.md` / `metrics_guide.md` use `## See Also`. Plan-phase picks one and uses it consistently across miner's docs — recommend `## See Also` (3-of-4 sibling majority).

### Heading discipline — `##` for major sections, no TOC

**Source:** All sampled tradedesk docs. None use a top-of-doc TOC; each H1 is followed directly by an intro paragraph and then `##` sections.

**Apply to:** All NEW docs.

---

## No Analog Found

| File | Role | Reason |
|------|------|--------|
| `./docs/future_mcp_http.md` | doc / deferred-design sketch | tradedesk does not defer features into docs — the pattern is composed from a tradedesk topical-guide opening + miner's existing planning-doc deep-dive linkage style. |

---

## Metadata

**Analog search scope:**
- `/home/darren/projects/radiusred/tradedesk/` (root + `docs/` + `docs/examples/`) — 5 docs sampled in full, 4 sampled by tail for license-footer variant detection.
- `/home/darren/projects/radiusred/tradedesk-miner/.planning/` (ROADMAP / REQUIREMENTS / PROJECT / STATE) — current shape for insertion points.
- `/home/darren/projects/radiusred/tradedesk-miner/crates/miner-core/` (findings, sweep, scan directory tree) — ground-truth source identification.
- `/home/darren/projects/radiusred/tradedesk-miner/crates/miner-mcp/src/main.rs` + `miner-http/src/main.rs` — placeholder shape for optional edits.
- `/home/darren/projects/radiusred/tradedesk-miner/README.md` — existing structure for `## Documentation` insertion point.

**Files scanned:** 19 source files (5 tradedesk markdown docs, 2 tradedesk Python examples, 1 tradedesk ARCHITECTURE.md, 1 tradedesk README.md, 4 miner planning docs, 2 miner placeholder mains, 1 miner README, 1 miner sweep test, 2 miner findings/sweep ground-truth pointers).

**Pattern extraction date:** 2026-05-21.

# Phase 4: Scan Catalogue (ANOM, CROSS, SEAS) - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-19
**Phase:** 04-scan-catalogue-anom-cross-seas
**Areas discussed:** Multi-instrument request shape (CROSS)

---

## Gray Areas Initially Presented

| Area | Description | Selected for discussion |
|------|-------------|-------------------------|
| Multi-instrument request shape (CROSS) | How CROSS scans receive their second instrument; extends the Phase 3 single-instrument facade contract Phase 6 must mirror | ✓ |
| Envelope shape for multi-output scans | Rolling / summary-stats / bucketed scans don't have a single headline scalar; emission shape decision | |
| Golden-test discipline for 22 scans | Statsmodels/scipy pinning + tolerance policy for the whole catalogue | |
| ANOM-01 returns primitive surface | Standalone callable scan vs kernel-only utility | |

**User's choice:** Multi-instrument request shape (CROSS) only. The other three deferred to plan-phase + Claude's-discretion.

---

## Multi-instrument request shape (CROSS)

### Q1: Where does the second instrument live in the request shape?

| Option | Description | Selected |
|--------|-------------|----------|
| Generalize ScanRequest.instrument to Vec<InstrumentSpec> | Replace `instrument + side` with `instruments: Vec<InstrumentSpec { symbol, side }>`. Single-instrument scans get length 1; CROSS gets length 2. One contract for both. Breaking change to ScanRequest. | ✓ |
| Add a sibling `peer_instrument` field on ScanRequest | Keep ScanRequest single-leg + add `peer: Option<InstrumentSpec>`. Less invasive but two-way validation matrix and awkward for v2 basket scans. | |
| Peer lives inside `params` (`params.peer = "GBPUSD"`) | ScanRequest stays as-is. Cost: peer not structurally visible; data_slice represents only one leg; harder MCP/HTTP tool typing. | |

**User's choice:** Generalize to Vec<InstrumentSpec>.
**Notes:** Phase 4 is the right time to break the facade — Phase 6 hasn't committed yet. Vec is uniform across ANOM/SEAS (length 1) and CROSS (length 2), and extends cleanly to v2 basket scans. → **D4-01 locked.**

### Q2: How should CLI express the instrument list, and how does the engine validate per-scan arity?

| Option | Description | Selected |
|--------|-------------|----------|
| Repeatable --instrument flag + new Scan::arity() trait method | CLI: `--instrument EURUSD:bid --instrument GBPUSD:bid`. Engine: new `Scan::arity() -> ScanArity` validated at preflight via new `PreflightCode::WrongInstrumentArity`. Declarative, surfaces in `miner scans`, MCP/HTTP can render typed parameters. | ✓ |
| Single --instruments flag with comma list | `--instruments EURUSD:bid,GBPUSD:bid` parsed into Vec. Same trait-method arity. More compact but harder to escape commas. | |
| Two named flags: --instrument-a / --instrument-b | `--instrument-a SYM --side-a SIDE --instrument-b SYM --side-b SIDE` for CROSS. Awkward symmetry with internal Vec. | |
| Runtime arity check inside each Scan::run | No trait method; each scan checks at runtime. Cost: not declarative; `miner scans` can't surface it; preflight can't reject as structured error. | |

**User's choice:** Repeatable --instrument flag + new Scan::arity() trait method.
**Notes:** Declarative arity surfaces in `miner scans` catalogue for MCP/HTTP Phase 6 to render typed parameters. → **D4-02 locked.**

### Q3: How should DataSlice and Raw represent both legs of a CROSS scan?

| Option | Description | Selected |
|--------|-------------|----------|
| DataSlice.sources: Vec<Source> + Raw.series keys carry leg labels | Replace `source: Source` with `sources: Vec<Source>` parallel to instruments. Raw.series uses `returns_a` / `returns_b` / `timestamps_ms_aligned`. Self-describing; no privileged-primary leg. Schema-additive concerns flagged for plan-phase research. | ✓ |
| Keep DataSlice.source (primary) + add peer_source: Option<Source> | Less invasive but asymmetric — "primary" leg becomes privileged; schema has singular/plural ambiguity. | |
| Per-leg findings (CROSS emits 1 finding per leg + 1 join finding) | 3× findings per CROSS invocation; harder for Quant agent to assemble; breaks "one finding per scan call" simplicity. | |

**User's choice:** DataSlice.sources: Vec<Source> + leg-labelled Raw.series keys.
**Notes:** Schema-additive guarantee is a plan-phase research checkpoint (D4-03-ALT fallback documented). → **D4-03 locked.**

### Q4: How should CROSS scans handle time-alignment and gap policy across two legs?

| Option | Description | Selected |
|--------|-------------|----------|
| Inner-join only + intersection gap policy | v1 ships inner-join only; CROSS-01 emits joined `(timestamps_ms, a_idx, b_idx)` view. Gap policy operates on intersection of both legs' manifests. Alternative join modes deferred to v2. | ✓ |
| Inner-join default + params.alignment switch | Same intersection gap policy + `--params alignment=inner\|forward_fill\|strict_equal`. Cost: 3× testing surface; forward-fill needs look-ahead-safety review. | |
| Inner-join only + union gap manifest | Same compute as A but data_slice.gap_manifest carries BOTH legs' manifests under per-leg keys. Richer provenance. | |
| Forward-fill default | Carry-last-known. Cost: subtle look-ahead landmines; generally NOT recommended for statistical mining. | |

**User's choice:** Inner-join only + intersection gap policy.
**Notes:** Forward-fill explicitly rejected for v1 due to look-ahead landmines on rolling stats. Alternative join modes (forward-fill, strict_equal) deferred to v2. → **D4-04 locked.**

### Q5: More questions on this area, or ready for context?

| Option | Description | Selected |
|--------|-------------|----------|
| More questions on this area | Probe: per-leg side independence; effect.value canonical pick for vector-output CROSS scans; hedge-ratio sign convention for Engle-Granger. | |
| Ready for context — capture what we have | Lock D4-01..D4-04 in CONTEXT.md. Remaining sub-decisions become plan-phase research checkpoints. | ✓ |

**User's choice:** Ready for context.
**Notes:** Per-leg side independence, effect.value canonical pick, hedge-ratio sign convention all delegated to plan-phase as Claude's-discretion items (D4-07, D4-08, D4-09).

---

## Claude's Discretion

Items the user explicitly delegated or did not select for discussion:

- **Envelope shape for multi-output scans** (rolling vol, summary stats, bucketed scans) — defaults proposed in CONTEXT.md `<deferred>` section; plan-phase locks per-scan.
- **Golden-test discipline** (statsmodels/scipy pinning + tolerance policy) — default suggestion: single pinned version (`statsmodels==0.14.6` + `scipy==1.x.y`) in `tests/REFERENCE-VERSIONS.md`; plan-phase confirms or splits per-scan.
- **ANOM-01 returns primitive surface** — default suggestion: standalone callable scan + reusable kernel utility (D4-05); plan-phase picks exact name and one-scan vs four-scans-per-variant.
- **Per-leg side independence for CROSS scans (D4-07)** — default: yes, independent sides allowed; typical use is same-side; plan-phase confirms no scan rejects mixed sides.
- **`effect.value` canonical pick for vector-output CROSS scans (D4-08)** — default: `effect.value = last_window_value`; full vector in `effect.extra.values` + `effect.extra.window_starts_ms`. Plan-phase locks per-scan against statsmodels reporting convention.
- **Hedge-ratio sign convention for Engle-Granger CROSS-05 (D4-09)** — default: leg-order determines regressand (`a`) vs regressor (`b`); plan-phase research confirms against statsmodels `coint()` ordering.
- **Plan/wave breakdown of the 22 scans into Phase 4 plans** — organisational decision for `/gsd:plan-phase 4`.
- **Look-ahead-safety API shape on `ScanCtx::bars_up_to(ts)`** — plan picks the simplest passing shape.
- **Hand-rolled JSON Schema vs typed `#[derive(JsonSchema)]` per-scan param_schema** — plan-phase per-scan.
- **Session-boundary defaults for SEAS-03 (Asia / London / NY UTC ranges)** — plan picks defensible source.
- **Trading-day-of-month cutoff for SEAS-04 EOM/SOM bucketing** — plan picks (likely last/first 3 trading days; configurable).
- **Default outlier z-threshold for ANOM-10** — plan picks (likely 3.0; z + modified-z both reported).
- **AIC lag selection details for ANOM-05 ADF, ARCH-LM default lag for ANOM-08** — plan pins against statsmodels defaults.

## Deferred Ideas

Ideas mentioned during discussion that were noted for future phases:

- **v2 alternative time-alignment join modes** (forward-fill, strict_equal) — REQUIREMENTS.md v2 if any concrete consumer ever asks. Forward-fill explicitly rejected for v1 due to look-ahead landmines for rolling stats.
- **v2 basket scan family** (Johansen cointegration, 3+ instruments) — already in REQUIREMENTS.md `SCAN-v2-01`. The `Vec<InstrumentSpec>` design from D4-01 + the `ScanArity` enum from D4-02 extend cleanly when v2 lands (`ScanArity::Many(min, max)` is the natural future variant).
- **D4-03-ALT fallback** (`peer_sources: Vec<Source>` additive sibling instead of replacing `source: Source` with `sources: Vec<Source>`) — invoked only if plan-phase research finds schemars 1.x emits non-additive output for the D4-03 change.
- **Statistical hygiene layer** (Cohen's d / Hedges' g / Cliff's delta / block bootstrap / phase-scramble nulls / BH-FDR / DSR) — Phase 5 / HYG-*.
- **TOML sweep manifest fanout** — Phase 5 / OP-04.
- **MCP / HTTP wrapper parity validation** — Phase 6 / OP-02 / OP-03.
- **Bench harness, flamegraph, golden regression scale-out** — Phase 7.

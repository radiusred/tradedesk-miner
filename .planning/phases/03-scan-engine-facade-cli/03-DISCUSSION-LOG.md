# Phase 3: Scan Engine, Facade & CLI - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in 03-CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-18
**Phase:** 03-scan-engine-facade-cli
**Areas discussed:** Demo scan choice (4 sub-decisions). All other gray areas (windowing semantics, gap-manifest emission, multi-input scope, plus 5 not-yet-surfaced categories: scan trait shape, registry construction, dry-run output, SIGINT mechanism, param_hash canonicalization) delegated to Claude's discretion.

---

## Gray-area selection

**Question asked:** Which gray areas do you want to discuss for Phase 3 (Scan Engine, Facade & CLI)?

| Option | Description | Selected |
|--------|-------------|----------|
| Demo scan choice | Phase 3 ships ONE fully-implemented scan to prove the facade. Trivial bar-count baseline, summary-stats (ANOM-02), or Ljung-Box (ANOM-04 — has scipy goldens)? | ✓ |
| Windowing semantics | What does `--window 2024-01-01:2024-12-31` mean operationally? | |
| Gap-manifest emission | Under `continuous_only`, how is the manifest delivered? | |
| Multi-input scope | Does Phase 3's CLI accept multiple instruments/timeframes in one invocation, or strict single-shot? | |

**User's choice:** Demo scan choice only.
**Notes:** First AskUserQuestion attempt returned empty; retry yielded the selection above. User chose to deep-dive demo-scan and delegate the rest.

---

## Demo Scan — selection

**Question asked:** Which scan should be the Phase 3 fully-implemented end-to-end demo?

| Option | Description | Selected |
|--------|-------------|----------|
| Ljung-Box on returns | ANOM-04. Exercises every envelope slot (value, p_value, n, extra.{lags,q_stats,p_values,acf}, raw.series.{returns,timestamps_ms}). Has statsmodels golden (`acorr_ljungbox`). | ✓ |
| Summary statistics | ANOM-02. Welford mean/std/skew/kurt/IQR/min/max. No p_value, no extra arrays. | |
| Bar-count baseline | Trivial 'how many bars in the slice'. Proves plumbing but no statistical content. | |
| Rolling volatility | ANOM-03. Array-heavy but spec variants (sample vs population std, log vs simple returns) still open. | |

**User's choice:** Ljung-Box on returns.
**Notes:** Matches the workflow's recommended option. Phase 3 demo scan locked as `stats.autocorr.ljung_box@1`. Drives D3-01 and the Phase 3 golden fixture against statsmodels.

---

## Demo Scan — returns computation location

**Question asked:** Where does the returns computation live for Ljung-Box?

| Option | Description | Selected |
|--------|-------------|----------|
| Inline inside the scan | Phase 3 Ljung-Box scan computes its own `log(close[t]/close[t-1])`. Avoids scope-creeping ANOM-01 into Phase 3. Refactor to shared primitive when ANOM-01 lands in Phase 4. | ✓ |
| Co-implement ANOM-01 in Phase 3 | Pull ANOM-01 forward — ship a reusable `returns(BarFrame, kind)` helper. Larger scope; eliminates the Phase-4 refactor. | |
| Inline + ship in raw.series only | Inline log returns AND ship them in `raw.series.returns` per D-04. No shared primitive — each Phase 4 scan reinvents it until ANOM-01 lands. | |

**User's choice:** Inline inside the scan (Claude's recommendation).
**Notes:** Locks D3-02. Phase 3 scope stays narrow; one extra Phase-4 refactor pass acceptable. Returns ARE still shipped in `raw.series.returns` per the D-04 input/output split (this is independent of where the computation lives — D3-04 captures it).

---

## Demo Scan — effect.extra contents

**Question asked:** What ships in the Ljung-Box finding's `effect.extra` block?

| Option | Description | Selected |
|--------|-------------|----------|
| Per-lag stats + ACF | `effect.value`/`p_value`/`n` headline; `effect.extra.{lags, q_stats, p_values, acf}` as RawArrays. Matches statsmodels output one-for-one. | ✓ |
| Joint stat only | Just `effect.value` (Q@max-lag), `p_value`, `n`. No per-lag arrays, no ACF. Smallest envelope; loses per-lag diagnostic. | |
| Per-lag stats only, no ACF | Per-lag `q_stats`+`p_values`+`lags`, skip `acf`. Forces agent to recompute ACF from `raw.series.returns`. | |

**User's choice:** Per-lag stats + ACF.
**Notes:** Locks D3-04. Quant agent gets the full Box-Jenkins diagnostic without re-running the scan. Bytes-per-finding cost (~4 × lags × 8 = ~32 × max_lag bytes base64-encoded) is acceptable.

---

## Demo Scan — lag default

**Question asked:** Lag-count default + parameter shape for Ljung-Box?

| Option | Description | Selected |
|--------|-------------|----------|
| Box-Jenkins default + override | `lags = min(10, n/5)` (statsmodels default). `--params lags=<int>` to override. Rejected at boundary if `lags<1` or `lags>=n` (PreflightCode::InvalidParameter). | ✓ |
| Fixed default 20 | Hard default 20; overridable. Simple to document; over-fits short windows. | |
| Required parameter, no default | User must pass `--params lags=N`. More explicit but more CLI typing. | |

**User's choice:** Box-Jenkins default + override.
**Notes:** Locks D3-03. Matches statsmodels golden exactly. Resolved value is echoed into every finding's `params` block (Phase 1 D-04 / D-11 / OP-08).

---

## Wrap-up choice

**Question asked:** Demo-scan area is locked. Which of these next?

| Option | Description | Selected |
|--------|-------------|----------|
| Windowing semantics | What `--window` means + what fills `data_slice.range`. | |
| Gap-manifest emission | Inline vs hash-referenced. | |
| Multi-input scope | Single-shot vs comma-list fanout. | |
| Wrap up — Claude discretion on the rest | Stop discussion. Defaults: windowing = output range with optional warm-up + data_slice = consumed range; gap-manifest = inline in data_slice; multi-input = strict single-shot. Plus the not-yet-surfaced decisions (scan trait shape, registry, dry-run, SIGINT, param_hash) all to Claude's discretion. All flagged for plan-phase research. | ✓ |

**User's choice:** Wrap up — Claude discretion on the rest.
**Notes:** All 20 non-user-locked decisions in 03-CONTEXT.md are tagged `(discretion)` and surfaced as plan-phase research items in `<open_questions>`. The plan-phase research agent must confirm or override each before execution begins.

---

## Claude's Discretion

Twenty decisions land here (D3-06 through D3-24 plus the gap-policy emission triplet D3-10..D3-12), grouped:

### Windowing semantics (D3-06..D3-09)
- `--window START:END` is the OUTPUT window, half-open `[START, END)`.
- CLI accepts ISO 8601 date or datetime, UTC-only.
- `data_slice.range` = the actual consumed range (post gap-partitioning).
- Shuffled-future regression test pins look-ahead safety for the demo scan.

### Gap-policy emission (D3-10..D3-12)
- `continuous_only` inlines the full `GapManifest` into every finding's `data_slice` via an additive optional field (`DataSlice.gap_manifest: Option<GapManifest>`).
- `strict` emits one `Finding::GapAborted` per invocation (no `Result` findings).
- Zero-gap fast path: empty manifest still ships under `continuous_only` for structural consistency.

### param_hash (D3-13)
- `blake3(serde_json::to_vec(&resolved_params))` lowercase hex. BTreeMap discipline already enforces byte-stable JSON; RFC 8785 JCS deferred unless plan-phase research finds a HashMap path.

### Scan engine (D3-14..D3-17)
- `Scan` trait shape with `id() + version() + param_schema() + finding_fields() + run(ctx, req, sink)`.
- `ScanCtx` brokering object owns `BarCache`, `GapDetector`, `RunId`, `code_revision`. Scans never see `Reader` directly.
- Static `Registry` constructed by an explicit `bootstrap()` function — not `inventory` magic.
- scan-id naming: `<family>.<subfamily>.<scan_name>@<integer>` where family ∈ {`stats`,`cross`,`seas`}.

### Facade + CLI (D3-18..D3-21)
- Single-shot multi-input (one instrument × side × timeframe × window per invocation). Phase 5 sweep manifest is the only fanout entry.
- CLI subcommand surface: `miner scan <id@version> --instrument --side --timeframe --window --gap-policy --dry-run --params` and `miner scans`.
- `--side` defaults to `bid`; `--gap-policy` defaults to `continuous_only`.
- `--dry-run` emits ONE additive envelope variant `Finding::DryRun(DryRunFinding)` to stdout.

### SIGINT + determinism + exit codes (D3-22..D3-24)
- `ctrlc` crate registered in `miner-cli` only; sets `Arc<AtomicBool>` flag passed as `CancellationToken` into the facade and into every scan.
- Run-level determinism guarantee: byte-identical output modulo `run_id` ULID + timestamps (which are intentionally non-deterministic).
- Four-tier exit codes: 0 (success), 1 (preflight/catastrophic), 2 (mid-stream scan_error emitted), 130 (SIGINT — overrides 0/2).

## Deferred Ideas

- ANOM-01 returns primitive → Phase 4.
- Multi-input fanout → Phase 5 (TOML sweep manifest).
- Bootstrap CIs, phase-scramble nulls, BH-FDR, DSR → Phase 5 (HYG-* requirements).
- Content-addressed gap-manifest deduplication via `gap_manifest_ref` → Phase 7 hardening.
- Full `Finding::DryRun` planning detail (per-instrument bar counts, per-job estimates) → Phase 5 sweep dry-run.
- `miner scans --schema-version 1` filtering → defer until v2 schema exists.
- `--output-format=cbor` and similar alternative serialisations → defer; JSONL is the v1 contract.
- `inventory`-crate registration pattern → revisit if registry ever splits across crates.
- `simd-json` for output encoding → profile first per CLAUDE.md guidance; flip only if Phase 7 benches show JSON encoding hot.
- `chrono-tz` for per-symbol non-UTC calendars → already deferred in Phase 2 D2-10.
</content>
</invoke>
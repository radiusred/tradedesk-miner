---
phase: 06-mcp-http-wrappers
reviewed: 2026-05-21T00:00:00Z
depth: standard
files_reviewed: 12
files_reviewed_list:
  - ARCHITECTURE.md
  - README.md
  - crates/miner-http/src/main.rs
  - crates/miner-mcp/src/main.rs
  - docs/.license-footer.md
  - docs/agent_integration.md
  - docs/examples/decode_finding.py
  - docs/examples/sample_sweep.toml
  - docs/findings_envelope.md
  - docs/future_mcp_http.md
  - docs/scan_catalogue.md
  - docs/sweep_manifest.md
findings:
  critical: 7
  warning: 6
  info: 4
  total: 17
status: issues_found
---

# Phase 6: Code Review Report

**Reviewed:** 2026-05-21
**Depth:** standard
**Files Reviewed:** 12
**Status:** issues_found

## Summary

Phase 6 is documentation-only: 10 markdown/Python/TOML files plus a 2-line
update to the two placeholder `main.rs` files. The two `main.rs` edits are
correct (tracing-info string only; no runtime behaviour change beyond the
new message).

The doc set, however, contains multiple concrete defects that will cause
runnable examples to fail or mislead agent integrators. The biggest issues:

1. The **runnable Python example `decode_finding.py` is broken end-to-end**
   — it dereferences two top-level envelope fields (`instruments`,
   `timeframe`) that do not exist on `ResultFinding`, and it passes the
   wire-form dtype string `"f64"` directly to `np.dtype()`, which raises
   `TypeError: data type 'f64' not understood`. The script cannot complete
   even one Result envelope.
2. The **sample sweep manifest `sample_sweep.toml` uses an invented
   nested-table form for `[hygiene]`** (`bootstrap = { method, n_iter }`)
   that does not match the actual `HygieneBlock` struct
   (`bootstrap: Option<String>` + flat `bootstrap_n: u32`). The file will
   fail TOML deserialisation against the typed `SweepManifest`. The
   `[fdr].family = "per_scan_id"` value is also not a recognised scope —
   valid values are `"scan_id"` / `"scan_family"` / `"none"`.
3. The **README's Phase 3 example invocations use the removed `--side bid`
   flag** (Plan 04-02 deleted it; side now travels inside
   `--instrument SYMBOL:side`). Two README examples (lines 110-112 and
   124-128) will fail clap parsing as written.
4. The **README's representative JSONL fragments use the Rust field name
   `"scan_id_at_version"`** rather than the actual wire form
   `"scan_id@version"`, and one fragment uses an object-shaped `ci95`
   (`{"low":...,"high":...}`) when the wire form is a two-element array.
5. **Three docs give three different definitions of exit code 2** (README:
   mid-stream ScanError; agent_integration.md: invalid CLI usage from clap;
   findings_envelope.md: fatal mid-run error). The CLI's
   `compute_exit_code` (`crates/miner-cli/src/main.rs` lines 484-493)
   returns 2 for `HadScanErrors` — the README is correct, the other two
   are wrong.
6. **All docs that enumerate `Dtype` values list six variants** (`"f64"`,
   `"f32"`, `"i64"`, ... `"u32"`) but the actual Rust `Dtype` enum
   (`crates/miner-core/src/findings/base64_bytes.rs` lines 73-77) has
   exactly one variant (`F64` → `"f64"`). Every raw payload — including
   `timestamps_ms` — is serialised with `dtype: "f64"`.
7. The **CLI parameter flag is `--params KEY=VAL`** (plural, repeatable,
   `clap::ArgAction::Append`); three doc surfaces use the wrong singular
   form `--param`.

The two placeholder `main.rs` files are otherwise clean — they pin
stderr-only logging and reference `docs/future_mcp_http.md` correctly.

## Critical Issues

### CR-01: `decode_finding.py` accesses non-existent top-level envelope fields

**File:** `docs/examples/decode_finding.py:52-54`
**Issue:** The script reads `envelope['instruments']` and
`envelope['timeframe']` as top-level keys on a `Result` envelope, but
`ResultFinding` (`crates/miner-core/src/findings/mod.rs` lines 331-360)
has neither field at the top level. Instrument/side/timeframe data lives
inside `data_slice.sources[]`. The script will raise `KeyError:
'instruments'` after printing the first two lines.
**Fix:**
```python
# Replace lines 53-54 with:
sources = envelope["data_slice"]["sources"]
instruments = [f"{s['symbol']}:{s['side']}" for s in sources]
timeframe = sources[0]["timeframe"] if sources else None
print(f"instruments   = {instruments}")
print(f"timeframe     = {timeframe}")
```
The same wrong claim appears in `docs/agent_integration.md` lines 51-52
("`instruments` — leg-labelled instrument vector...", "`timeframe` —
the bar-resolution string... Always present alongside `instruments`") —
both bullets must be removed and the surrounding prose corrected to
point at `data_slice.sources[]`.

### CR-02: `decode_finding.py` and `agent_integration.md` use a numpy dtype string that throws TypeError

**File:** `docs/examples/decode_finding.py:36`, `docs/agent_integration.md:92`
**Issue:** Both helpers pass `raw_array["dtype"]` (wire-form string
`"f64"`) directly to `np.dtype()`. NumPy does not recognise `"f64"` as a
dtype literal — it raises `TypeError: data type 'f64' not understood`.
Verified by `python3 -c "import numpy as np; np.dtype('f64')"`. NumPy
expects `"f8"` (digit = byte count) or the explicit endian form `"<f8"`.
The `findings_envelope.md` "f64" ⟷ `np.dtype("<f8")` mapping table on
lines 96-103 documents the correct mapping, but neither helper applies
it.
**Fix:**
```python
# Mapping table at module scope:
_WIRE_TO_NUMPY = {
    "f64": "<f8", "f32": "<f4",
    "i64": "<i8", "i32": "<i4",
    "u64": "<u8", "u32": "<u4",
}

def decode_raw_array(raw_array):
    np_dtype = np.dtype(_WIRE_TO_NUMPY[raw_array["dtype"]])
    shape = tuple(raw_array["shape"])
    payload = base64.b64decode(raw_array["data"])
    return np.frombuffer(payload, dtype=np_dtype).reshape(shape)
```
Update the inline one-liner in `findings_envelope.md` line 92 (in
`agent_integration.md`) the same way. CR-06 (only `f64` exists in v1)
narrows the mapping to a single entry — but the helper should still go
via a lookup table so additive dtype variants in future schema versions
do not silently break consumers.

### CR-03: `sample_sweep.toml` `[hygiene]` block uses an invented nested-table shape that will not deserialise

**File:** `docs/examples/sample_sweep.toml:28-30`
**Issue:** The sample uses
```toml
[hygiene]
bootstrap = { method = "stationary", n_iter = 999 }
null = { method = "circular_shift", n_iter = 999 }
```
but the actual `HygieneBlock`
(`crates/miner-core/src/sweep/manifest.rs` lines 93-101) declares
`bootstrap: Option<String>` (a flat scalar) plus separate `bootstrap_n:
u32` / `null_n: u32` integer fields. TOML deserialisation will reject
the inline-table value `{ method = ..., n_iter = ... }` against an
`Option<String>` shape. The file as committed cannot be run with
`miner sweep`.
**Fix:**
```toml
[hygiene]
bootstrap = "stationary"
bootstrap_n = 999
null = "circular_shift"
null_n = 999
```
The same flat shape is already documented correctly in
`docs/sweep_manifest.md` lines 48-52 and the README example
(lines 252-256) — the sample needs to match.

### CR-04: `sample_sweep.toml` `[fdr].family = "per_scan_id"` is not a recognised scope

**File:** `docs/examples/sample_sweep.toml:33`
**Issue:** The sample sets `family = "per_scan_id"`. The
`scope_family` dispatch (`crates/miner-core/src/sweep/executor.rs`
lines 705-710) accepts only `"scan_id"`, `"scan_family"`, and `"none"`;
every other value silently falls through to the `"scan_id"` defensive
default. The value `"per_scan_id"` therefore acts as `"scan_id"` but
the manifest documents the wrong vocabulary to consumers. The
`sweep_manifest.md` text on line 113 also contains a confused sentence
("v1 also accepts `"scan_family"` per the per_scan_id default of D5-02")
that conflates the two.
**Fix:** Replace with the canonical default value:
```toml
[fdr]
family = "scan_id"
alpha = 0.05
```
And in `docs/sweep_manifest.md` line 113 rewrite the contradictory
sentence to: `Default "scan_id" (one BH family per scan_id@version);
v1 also accepts "scan_family" (one family per scan-family prefix) and
"none" (suppress per-family BH; emit empty fdr_by_family).`

### CR-05: README Phase 3 examples use the removed `--side bid` flag — clap will reject them

**File:** `README.md:110-112,124-128`
**Issue:** The two Phase 3 invocations in the README use
```sh
--instrument EURUSD --side bid --timeframe 15m \
```
but `--side` was removed in Plan 04-02 (`crates/miner-cli/src/scan_args.rs`
lines 183-185 documents the removal; line 21-22 the replacement). The
current `parse_instrument_spec` requires `SYMBOL:side` form and rejects
the bare `EURUSD` value with the error: `--instrument value must be of
the form SYMBOL:side (e.g., EURUSD:bid); got "EURUSD"`. The same defect
applies to the dry-run example on lines 124-128 (`--instrument EURUSD`
with no `:side` suffix).
**Fix:**
```sh
cargo run -p miner-cli -- scan stats.autocorr.ljung_box@1 \
    --instrument EURUSD:bid --timeframe 15m \
    --window 2024-01-01:2024-12-31
```
Apply the same correction to the dry-run example.

### CR-06: All three docs over-enumerate `Dtype` variants — v1 has exactly one

**File:** `docs/agent_integration.md:96-101`,
`docs/findings_envelope.md:133`
**Issue:** Both docs list six dtype values (`"f64"`, `"f32"`, `"i64"`,
`"i32"`, `"u64"`, `"u32"`), but `Dtype`
(`crates/miner-core/src/findings/base64_bytes.rs` lines 73-77) has
exactly one variant — `F64` — which serialises to `"f64"`. Every
`RawArray` in v1 carries `dtype: "f64"`. The over-enumeration cascades
into the Python decode example in `findings_envelope.md` line 155
(`dtype="<i8"` for `timestamps_ms`) which would mis-decode the actual
payload: `timestamps_ms` is constructed via `let timestamps_ms: Vec<f64>
= ...; f64_slice_to_raw_array(&timestamps_ms)`
(`crates/miner-core/src/scan/ljung_box/mod.rs` lines 207, 230-231) so
the wire bytes are little-endian f64, not i64.
**Fix:** Tighten both docs to:
```
v1 dtype values: `"f64"` ⟷ `np.dtype("<f8")` (8-byte IEEE-754 double).
The `Dtype` enum is open-additive — future schema versions may add
`"f32"` / `"i64"` / ... — but v1 emits f64 for every array including
`timestamps_ms` (the timestamps are packed as f64 ms-since-epoch, not
i64).
```
And in `findings_envelope.md` lines 152-157 change the decode example
to use `dtype="<f8"` for `timestamps_ms` too. The threshold cast back
to integer milliseconds is the consumer's responsibility.

### CR-07: README JSONL examples use Rust field names + wrong `ci95` shape

**File:** `README.md:168,191,216,282-287`
**Issue:** Three (now four) representative `Result` lines spell the
field key `"scan_id_at_version":"..."`. The actual wire form is
`"scan_id@version":"..."` per the `#[serde(rename = "scan_id@version")]`
attribute on `ResultFinding` line 334 (and on `ScanErrorFinding`
line 369, `GapAbortedFinding` line 396). The sweep example on line 285
goes further and renders `ci95` as `{"low":...,"high":...}` — but
`Effect.ci95` is `Option<[f64; 2]>`
(`crates/miner-core/src/findings/mod.rs` line 182), so the wire form is
a two-element array (`[lo, hi]`), not an object. A consumer copy-pasting
either pattern would write a parser against keys that never appear.
**Fix:** Replace every `"scan_id_at_version"` with `"scan_id@version"`
and rewrite the sweep example to:
```json
"effect":{"metric":"ljung_box_q_stat","value":...,"p_value":0.043,
          "ci95":[..., ...]}
```

## Warnings

### WR-01: `decode_finding.py` will `AttributeError` when `raw` serialises as `null`

**File:** `docs/examples/decode_finding.py:58`
**Issue:** `envelope.get("raw", {}).get("series", {})` returns `{}` if
the key is absent, but if `raw` is present with the JSON value `null`,
`envelope.get("raw", {})` returns `None`, and the chained
`.get("series", {})` raises `AttributeError: 'NoneType' object has no
attribute 'get'`. `ResultFinding.raw` is `Option<Raw>`
(`findings/mod.rs` line 350) and serialises as JSON `null` when absent.
**Fix:**
```python
raw = (envelope.get("raw") or {}).get("series", {})
```

### WR-02: `agent_integration.md` exit-code routing is wrong for codes 1 and 2

**File:** `docs/agent_integration.md:109-115`
**Issue:** The doc claims exit 1 covers both preflight rejection AND
mid-stream `ScanError`, and exit 2 covers clap CLI-usage rejection. The
actual `compute_exit_code` (`crates/miner-cli/src/main.rs` lines 484-493)
maps `PreflightFailed → 1`, `HadScanErrors → 2`, `Ok → 0`, `Cancelled →
130`. Mid-stream `ScanError` envelopes produce exit 2, not 1 —
contradicting this doc. Clap-rejection-of-bad-argv exits 2 by clap's
default convention, so exit 2 is overloaded between two paths, but the
doc's primary claim (exit 1 ⇒ possibly mid-stream ScanError) is wrong.
The same routing is misdescribed in `findings_envelope.md` lines 199-202
(`2 — fatal mid-run error`). The README's Phase 3 description (lines
116-118) is the only one that is correct.
**Fix:** Rewrite the agent_integration.md table to match
`compute_exit_code` (canonical source) and add a note that exit 2 also
fires on clap-level argv rejection:
```
- `0` — clean run; `RunEnd` closed the stream.
- `1` — preflight rejection. A `WireError` was emitted on stderr; no
  `RunStart` ever reached stdout. Inspect `code`.
- `2` — at least one mid-stream `Finding::ScanError` was emitted (the run
  continued for other jobs but at least one failed). The agent MUST
  inspect each ScanError envelope's `error_code`. Exit 2 is also clap's
  default code when argv parsing fails — in that case stdout is empty
  and stderr carries clap's usage banner.
- `130` — SIGINT.
```
And in `findings_envelope.md` lines 199-202 align the same way.

### WR-03: `--param` (singular) flag does not exist — CLI requires `--params`

**File:** `docs/examples/decode_finding.py:21`,
`docs/agent_integration.md:30,50`
**Issue:** The Python subprocess example in `agent_integration.md` line
30 passes `"--param", "lags=5"`; the bullet on line 50 says "The
`--param` flag may be repeated"; the `decode_finding.py` usage docstring
on line 21 uses `--param lags=5`. The actual flag is
`--params KEY=VAL` (`crates/miner-cli/src/scan_args.rs` line 93:
`#[arg(long = "params", action = clap::ArgAction::Append)]`). All three
examples will fail with clap's "unexpected argument '--param'" error.
`scan_catalogue.md` line 13 correctly uses `--params lags=5`.
**Fix:** Replace every `--param` (singular) with `--params` (plural) in
the affected files.

### WR-04: ARCHITECTURE.md and README disagree with reality on scan count

**File:** `README.md:25,142`, `ARCHITECTURE.md:8`
**Issue:** README line 25-26 says "Phase 4 ships 22 registered scans
across three families — 11 single-instrument anomaly tests (ANOM), 5
two-instrument cross scans (CROSS), and 6 seasonality scans (SEAS)".
ARCHITECTURE.md line 8 says "the 22 v1 scans". Actual count from the
`register_*` call sites (`crates/miner-core/src/scan/anom/mod.rs`
lines 63-73 + `crates/miner-core/src/scan/registry.rs` line 96 +
`crates/miner-core/src/scan/cross/mod.rs` lines 36-40 +
`crates/miner-core/src/scan/seas/mod.rs` lines 50-55) is **23**:
12 ANOM (including the LjungBoxScan registered in `registry.rs` plus
11 in `anom/mod.rs`) + 5 CROSS + 6 SEAS. The `scan_catalogue.md`
("23 distinct scan_id@version pairs") and `agent_integration.md`
("the 23 v1 scan_id@version strings") are correct; the README and
ARCHITECTURE.md are off by one.
**Fix:** README line 25 → "Phase 4 ships 23 registered scans across
three families — 12 single-instrument anomaly tests (ANOM, including
both raw and squared-returns Ljung-Box variants under ANOM-04), 5
two-instrument cross scans (CROSS, with both Pearson and Spearman
rolling-correlation under CROSS-02), and 6 seasonality scans (SEAS)".
ARCHITECTURE.md line 8 → "the 23 v1 scans".

### WR-05: ARCHITECTURE.md says "six crates" then enumerates seven

**File:** `ARCHITECTURE.md:6-15`
**Issue:** Line 6 says "six crates with a strict one-way dependency
direction" but the bullet list enumerates seven names (`miner-core`,
`miner-reader-dukascopy`, `miner-cli`, `miner-mcp`, `miner-http`,
`miner-bench`, `xtask`). The workspace `Cargo.toml` has seven members.
The README handles this correctly on line 342-345 by calling out the
`xtask` as a "seventh" crate.
**Fix:** Rewrite ARCHITECTURE.md line 6 to: "The codebase is organised
into seven Cargo crates with a strict one-way dependency direction:"
and add a sentence noting `xtask` is a dev-only workspace member.

### WR-06: README Phase-1-Delivers section references "five-variant" enum that is now seven

**File:** `README.md:347-352`
**Issue:** "Locked `Finding` envelope — five-variant tagged enum
(`run_start`, `result`, `scan_error`, `gap_aborted`, `run_end`)..."
While the section is labelled "What Phase 1 Delivers" and is therefore
historically accurate for Phase 1, a reader skimming the README in
Phase 6 may take this as the current shape. The current enum has seven
variants (`DryRun` + `SweepSummary` added in Phases 3 and 5
respectively). The bullet sets the wrong baseline for downstream
sections that mention the additional variants.
**Fix:** Add an inline parenthetical: "...five-variant tagged enum
(`run_start`, `result`, `scan_error`, `gap_aborted`, `run_end`; Phases 3
and 5 additively extended the enum with `dry_run` and `sweep_summary` —
see `docs/findings_envelope.md` for the current seven-variant shape)".

## Info

### IN-01: `sweep_manifest.md` numbered-list error renders as 1,2,3 instead of 1,2 with a sub-clause

**File:** `docs/sweep_manifest.md:199-211`
**Issue:** The section "TOML parse and validation errors" announces
"Two failure paths produce a single `WireError`..." but then enumerates
three numbered paths (`1. TOML syntax error`, `2. Schema mismatch`,
`3. Validation failure`). Either the intro should say "Three failure
paths" or items 1 and 2 should be merged (they share the
`InvalidParameter` code).
**Fix:** Rewrite the intro to "Three failure paths produce a single
`WireError`..."

### IN-02: Stale "Phase 3 ships exactly one scan" claim

**File:** `README.md:100`
**Issue:** "Phase 3 ships exactly one scan (`stats.autocorr.ljung_box@1`).
Lines validate against `schemas/scans-catalogue-v1.schema.json`." The
section header is "Running a Scan (Phase 3)", so historically accurate.
But a user running `miner scans` today will see 23 lines, not 1. Worth
a one-line note pointing forward to Phase 4.
**Fix:** Append: "Phase 4 (below) expanded the catalogue to 23 scans;
the `miner scans` command exhaustively enumerates the live registry."

### IN-03: Comment-typo: `Dtype::Json-style` in `ljung_box_sq/mod.rs` is not a doc problem but suggests a docs-vocabulary inconsistency

**File:** `docs/findings_envelope.md:133`
**Issue:** A nearby source comment in
`crates/miner-core/src/scan/anom/ljung_box_sq/mod.rs:161` mentions a
hypothetical `Dtype::Json` variant. Since `findings_envelope.md`
already commits to a fixed list of v1 dtypes, it should explicitly
state the enum is closed at v1 (one variant) but the WIRE form is
open-string for forward-compat — to disambiguate code-comment hints from
the wire contract.
**Fix:** In the dtype paragraph, add: "The `Dtype` enum in
`crates/miner-core/src/findings/base64_bytes.rs` is intentionally a
single-variant enum in v1; the JSON Schema treats `dtype` as an
open-string field so additive variants (`"f32"`, `"i64"`, future
`"json"`) can land without a schema break."

### IN-04: `future_mcp_http.md` references its own deferral source twice without consolidating

**File:** `docs/future_mcp_http.md:5`
**Issue:** Line 5 says "each is currently twelve lines emitting one
`tracing::info!` message pointing at this doc". The actual files are
14 lines (`crates/miner-http/src/main.rs` and `crates/miner-mcp/src/main.rs`
both have 14 lines after Phase 6's update). Trivial line-count drift
but worth fixing so the doc stays accurate against the placeholders it
describes; also referenced in the v2-contributor "pick this up" section
on line 67 ("Each is twelve lines today").
**Fix:** Update both references to "fourteen lines" (or omit the line
count and just say "each is a minimal placeholder shell emitting one
`tracing::info!`").

---

_Reviewed: 2026-05-21_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_

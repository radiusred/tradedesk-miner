# Phase 4: Scan Catalogue (ANOM, CROSS, SEAS) - Pattern Map

**Mapped:** 2026-05-19
**Files analyzed:** 22 new scan modules + 2 primitives + 5 facade-extension touches + 22 integration-test files + 3 fixture goldens + 4 Wave-0 infrastructure files = 58 file targets
**Analogs found:** 58 / 58 (every Phase 4 file maps to a Phase 1-3 analog)

The dominant analog is the **Phase 3 `LjungBoxScan` (`mod.rs` + `kernel.rs` split)** which the research deck (`04-RESEARCH.md` §Section 8) calls the "gold-standard reference pattern." Every Phase 4 scan replicates it verbatim. Cross-cutting facade extensions analog to Phase 3's `engine`, `findings`, `scan/mod.rs`, `error/codes.rs`, and `scan/registry.rs` files which Phase 3 just stabilised under plan 03-07.

## File Classification

### Scan implementations (22 new + 2 primitives)

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/miner-core/src/scan/primitives/mod.rs` | namespace-module | n/a (re-export only) | `crates/miner-core/src/scan/mod.rs` lines 48-53 (pub-mod + pub-use block) | role-match |
| `crates/miner-core/src/scan/primitives/returns.rs` | kernel-only primitive | transform: `&[f64] close -> Vec<f64> returns` | `crates/miner-core/src/scan/ljung_box/kernel.rs::log_returns` (lines 25-28) | exact (D4-06: "move, do not rewrite") |
| `crates/miner-core/src/scan/primitives/time_alignment.rs` | kernel-only primitive | transform: `(&BarFrame, &BarFrame) -> AlignedPair` | `crates/miner-core/src/scan/ljung_box/kernel.rs::biased_acf` (pure-fn shape) + `04-RESEARCH.md` lines 596-628 (inner-join body) | role-match |
| `crates/miner-core/src/scan/anom/mod.rs` | namespace-module | n/a (re-export) | `crates/miner-core/src/scan/mod.rs` lines 48-53 | role-match |
| `crates/miner-core/src/scan/anom/returns.rs` (+ `kernel.rs`) | scan implementation | single-shot: BarFrame.close -> ResultFinding | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/anom/summary.rs` (+ `kernel.rs`) | scan implementation | single-shot: Welford pass over closes | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/anom/vol.rs` (+ `kernel.rs`) | scan implementation | **vector-output:** rolling window stat | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` + `04-RESEARCH.md` Pattern 3 (lines 401-427) | exact |
| `crates/miner-core/src/scan/anom/ljung_box/` (squared-returns variant) | scan implementation | single-shot, second registration | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` (refactor: factor returns out, register `_sq` variant) | exact |
| `crates/miner-core/src/scan/anom/adf.rs` (+ `kernel.rs`) | scan implementation | single-shot with AIC lag selection | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/anom/kpss.rs` (+ `kernel.rs`) | scan implementation | single-shot stationarity test | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/anom/variance_ratio.rs` (+ `kernel.rs`) | scan implementation | single-shot Lo-MacKinlay VR(k) | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/anom/arch_lm.rs` (+ `kernel.rs`) | scan implementation | single-shot ARCH-LM | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/anom/jarque_bera.rs` (+ `kernel.rs`) | scan implementation | single-shot normality | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/anom/outliers.rs` (+ `kernel.rs`) | scan implementation | single-shot z + modified-z | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/anom/drawdown.rs` (+ `kernel.rs`) | scan implementation | single-shot peak-trough | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` | exact |
| `crates/miner-core/src/scan/cross/mod.rs` | namespace-module | n/a (re-export) | `crates/miner-core/src/scan/mod.rs` lines 48-53 | role-match |
| `crates/miner-core/src/scan/cross/corr_rolling.rs` (+ `kernel.rs`) | scan implementation (Pair arity) | **vector-output two-leg:** AlignedPair -> rolling Pearson/Spearman vector | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` + `04-RESEARCH.md` Pattern 2 (lines 379-399) + Pattern 3 | role-match (Pair vs Single arity is the delta) |
| `crates/miner-core/src/scan/cross/ols_rolling.rs` (+ `kernel.rs`) | scan implementation (Pair arity) | **vector-output two-leg:** rolling OLS β/α/R²/σ | same as above + nalgebra SMatrix import | role-match |
| `crates/miner-core/src/scan/cross/lead_lag.rs` (+ `kernel.rs`) | scan implementation (Pair arity) | single-shot two-leg: CCF over ±N lags | same as above | role-match |
| `crates/miner-core/src/scan/cross/engle_granger.rs` (+ `kernel.rs`) | scan implementation (Pair arity) | single-shot two-leg: EG + OU half-life | same as above | role-match |
| `crates/miner-core/src/scan/seas/mod.rs` | namespace-module | n/a (re-export) | `crates/miner-core/src/scan/mod.rs` lines 48-53 | role-match |
| `crates/miner-core/src/scan/seas/hour_of_day.rs` (+ `kernel.rs`) | scan implementation | single-shot bucketed | `crates/miner-core/src/scan/ljung_box/{mod.rs,kernel.rs}` + bucket vectors via Pattern 3 | role-match |
| `crates/miner-core/src/scan/seas/day_of_week.rs` (+ `kernel.rs`) | scan implementation | single-shot bucketed | same | role-match |
| `crates/miner-core/src/scan/seas/session.rs` (+ `kernel.rs`) | scan implementation | single-shot bucketed (uses `Calendar`) | same + `crates/miner-core/src/calendar.rs` consumer pattern | role-match |
| `crates/miner-core/src/scan/seas/eom_som.rs` (+ `kernel.rs`) | scan implementation | single-shot bucketed | same | role-match |
| `crates/miner-core/src/scan/seas/anova_kw.rs` (+ `kernel.rs`) | scan implementation | single-shot multi-test (ANOVA + KW) | same | role-match |
| `crates/miner-core/src/scan/seas/event_window.rs` (+ `kernel.rs`) | scan implementation | single-shot event-aligned | same | role-match |

### Facade-extension touches (Phase 3 surface gets D4-01..D4-04 changes)

| Modified File | Role | Change | Closest Analog | Match Quality |
|---------------|------|--------|----------------|---------------|
| `crates/miner-core/src/scan/mod.rs` | trait + types | D4-01 (`ScanRequest.instrument+side` -> `instruments: Vec<InstrumentSpec>`), D4-02 (`Scan::arity()` trait method + `ScanArity` enum) | existing `ScanRequest` struct (lines 185-232) + existing `Scan` trait body (lines 68-104) | exact (in-place extension of Phase 3 surface) |
| `crates/miner-core/src/findings/mod.rs` | envelope types | D4-03 (`DataSlice.source: Source` -> `sources: Vec<Source>` OR D4-03-ALT `peer_sources: Vec<Source>` sibling) | existing `DataSlice` struct (lines 80-90) + existing `Source` struct (lines 96-102) | exact |
| `crates/miner-core/src/scan/registry.rs` | factory | extend `bootstrap()` with 22 alphabetical `r.register(Box::new(...))` lines | existing `bootstrap()` (lines 82-88) | exact |
| `crates/miner-core/src/error/codes.rs` | enum | add `PreflightCode::WrongInstrumentArity` variant + `as_str` arm | existing `PreflightCode` enum (lines 20-53) | exact |
| `crates/miner-core/src/engine/mod.rs` | facade body | D4-02 arity preflight; D4-04 two-leg cache fetch + cancel poll between fetches; intersect gap manifests for Pair | existing `run_one_with_registry` (lines 211+) | exact (additive branches inside existing 7-step algorithm) |
| `crates/miner-core/src/engine/preflight.rs` | preflight helpers | new `validate_arity(scan, &instruments) -> Result<(), WireError>` | existing `resolve_scan` (preflight.rs:79+) + existing `resolve_scan_id_at_version` (lines 44-73) | exact |
| `crates/miner-core/src/engine/gap_policy.rs` | dispatch | extend `dispatch` for two-leg INTERSECTION (or new `dispatch_pair`) | existing `dispatch` (line 136) + `GapDispatch` enum (line 99) | exact (additive variant or sibling function) |
| `crates/miner-core/src/gap.rs` OR `scan/primitives/time_alignment.rs` | helper fn | new `intersect_gaps(&GapManifest, &GapManifest) -> GapManifest` | existing `GapManifest` (gap.rs:83-101) + `GapSpan` (lines 103-117) | role-match (Plan picks home) |
| `crates/miner-cli/src/scan_args.rs` | CLI parsing | repeatable `--instrument SYMBOL:side` parser | existing `ScanArgs.instrument: String + side: String` (lines 46-58) | exact |
| `schemas/findings-v1.schema.json` | wire schema | regen output (xtask `gen-schema`) | existing committed artifact | exact (regen-only) |
| `schemas/scans-catalogue-v1.schema.json` | catalogue schema | regen with new `arity` field per scan | existing committed artifact | exact (regen-only) |

### Tests (22 integration tests + 1 facade test + 1 arity test + extend shuffled_future)

| New/Modified File | Role | Closest Analog | Match Quality |
|-------------------|------|----------------|---------------|
| `crates/miner-core/tests/scan_<name>.rs` (22 files) | integration test (golden + envelope snapshot) | `crates/miner-core/tests/scan_ljung_box.rs` (lines 1-265) | exact |
| `crates/miner-core/tests/two_leg_facade.rs` | integration test (D4-01..D4-04 end-to-end) | `crates/miner-core/tests/dry_run.rs` + `gap_policy.rs` patterns | role-match |
| `crates/miner-core/tests/arity_preflight.rs` | unit/integration test (WrongInstrumentArity) | `crates/miner-core/tests/gap_policy.rs` (calls preflight helpers directly) | role-match |
| `crates/miner-core/tests/shuffled_future_regression.rs` (EXTEND) | proptest | existing file (lines 1-80+) | exact (additive — new proptest fn per rolling/causal scan; current file documents this as the Phase 4 extension point) |

### Fixtures & goldens (Wave 0 infrastructure)

| New File | Role | Closest Analog | Match Quality |
|----------|------|----------------|---------------|
| `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` | docs (pinned version table) | `04-RESEARCH.md` §1.2 (table); Phase 3 has no analog — net-new Wave 0 file | n/a (new conventions doc) |
| `crates/miner-core/tests/goldens/python-requirements.lock` | pip lock | net-new Wave 0 file | n/a |
| `crates/miner-core/tests/goldens/<scan_id>.jsonl` (≥3: 1 ANOM, 1 CROSS, 1 SEAS minimum; per Wave-0 budget) | golden JSONL fixture | `crates/miner-core/tests/fixtures/ljung_box_golden.json` | role-match (JSONL vs JSON; ANOM-04 golden is single-object JSON because the Phase 3 scan emits one envelope; Phase 4 vector-output scans match the same single-envelope shape so JSONL with one line per golden is the same wire form) |
| `crates/miner-core/tests/goldens/generate_<scan>.py` (one per golden) | golden regen script | `crates/miner-core/tests/fixtures/generate_golden.py` | exact |

## Pattern Assignments

### Pattern A — Single-leg ANOM scan (Single arity, single-shot or vector-output)

Applies to: ANOM-01 returns, ANOM-02 summary, ANOM-03 vol-rolling (vector-output), ANOM-04 squared-returns variant, ANOM-05 ADF, ANOM-06 KPSS, ANOM-07 VR, ANOM-08 ARCH-LM, ANOM-09 JB, ANOM-10 outliers, ANOM-11 drawdown.

**Analog:** `crates/miner-core/src/scan/ljung_box/mod.rs`

**Imports block** (mod.rs lines 41-50) — copy verbatim, swap kernel module name:

```rust
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use chrono::Utc;

use crate::findings::{
    Base64Bytes, DataSlice, Dtype, Effect, Finding, FindingSink, Raw, RawArray, ResultFinding,
    Source,
};
use crate::scan::{Scan, ScanArity, ScanCtx, ScanError, ScanFindingShape, ScanRequest};
//                       ^^^^^^^^^ NEW Phase 4 import — `ScanArity` added by mod.rs extension

pub mod kernel;
```

**Scan-id + version constants** (mod.rs lines 60-67):

```rust
pub struct <Name>Scan;
const SCAN_ID: &str = "<family>.<subfamily>.<scan_name>";   // e.g., "stats.returns.profile"
const SCAN_VERSION: u32 = 1;
const EFFECT_METRIC: &str = "<canonical_metric>";          // e.g., "returns_log_mean"
```

**Trait impl** (mod.rs lines 69-241) — copy verbatim, swap `id`/`version`/`param_schema`/`finding_fields`/`run`. The structural pattern is fixed:

```rust
impl Scan for <Name>Scan {
    fn id(&self) -> &'static str { SCAN_ID }
    fn version(&self) -> u32 { SCAN_VERSION }
    fn arity(&self) -> ScanArity { ScanArity::Single }       // NEW Phase 4 trait method (D4-02)

    fn param_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": { /* hand-rolled per scan */ },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &[/* per 04-RESEARCH.md §2 row */],
            raw_series_keys:   &[/* always includes "timestamps_ms" */],
        }
    }

    fn run(&self, ctx: &ScanCtx<'_>, req: &ScanRequest, sink: &mut dyn FindingSink)
        -> Result<(), ScanError>
    {
        if ctx.cancel.load(Ordering::Relaxed) { return Ok(()); }      // D3-22 cancel-at-entry
        // ... kernel call(s), envelope construction, sink.write_envelope ...
        Ok(())
    }
}
```

**Envelope construction** (mod.rs lines 131-218) — copy verbatim except per-scan `effect.metric`, `effect.value`, `effect.extra` keys, `raw.series` keys. The D4-03 change replaces the `source: Source { ... }` struct literal (lines 209-214) with a `sources: vec![Source { ... }]` Vec literal (length = arity = 1 for Single) — OR D4-03-ALT keeps `source` + adds empty `peer_sources: vec![]`. Plan 04-1 spike decides.

**`f64_slice_to_raw_array` helper** (mod.rs lines 298-312) — Plan-research §3 recommends lifting to `scan::primitives::raw_array::f64_slice_to_raw_array` (one shared copy) instead of replicating per-scan. The helper body is unchanged:

```rust
fn f64_slice_to_raw_array(s: &[f64]) -> RawArray {
    let mut bytes = Vec::with_capacity(s.len() * 8);
    for v in s { bytes.extend_from_slice(&v.to_le_bytes()); }
    RawArray {
        data: Base64Bytes(bytes),
        shape: vec![s.len() as u64],
        dtype: Dtype::F64,
    }
}
```

**Param-resolution helper** (mod.rs lines 253-274) — copy verbatim for any scan with required params. The shape is "read `req.resolved_params.get(KEY)`, parse via `as_i64` / `as_f64` / `as_str`, fall back to default constant, validate ranges, return `ScanError::Kernel(_)` on violation":

```rust
fn resolve_lags(req: &ScanRequest, n: usize) -> Result<usize, ScanError> {
    let raw = req.resolved_params.get("lags");
    let lags_i64: i64 = match raw {
        Some(v) => v.as_i64()
            .ok_or_else(|| ScanError::Kernel(format!("lags must be an integer; got {v}")))?,
        None => default_lags(n),
    };
    if lags_i64 < 1 { return Err(ScanError::Kernel(format!("lags must be >= 1; got {lags_i64}"))); }
    let lags_us = usize::try_from(lags_i64)
        .map_err(|_| ScanError::Kernel(format!("lags out of range for usize: {lags_i64}")))?;
    if lags_us >= n {
        return Err(ScanError::Kernel(format!("lags must be < n; got lags={lags_us}, n={n}")));
    }
    Ok(lags_us)
}
```

**Test block** (mod.rs lines 323-693) — copy verbatim test fixtures (`ar1_bar_frame_seeded`, `sample_request_with_params`, `make_ctx`, `parse_sink_to_findings`) and adapt the 13 named tests:

- `<scan>_id_and_version`
- `<scan>_finding_fields`
- `<scan>_param_schema`
- `<scan>_default_params` (if defaults exist)
- `<scan>_explicit_params`
- `<scan>_invalid_params_low` (range-violation rejection)
- `<scan>_invalid_params_high`
- `<scan>_emits_one_result`
- `<scan>_result_envelope_shape` (the PINNED field-name regression — line 549-577 of LjungBox)
- `<scan>_extras_have_correct_lengths`
- `<scan>_raw_series_lengths`
- `<scan>_raw_new_enforces_timestamps_ms` (D-03 invariant)
- `<scan>_cancellation`

### Pattern B — Kernel split (`kernel.rs`)

**Analog:** `crates/miner-core/src/scan/ljung_box/kernel.rs`

**File header + clippy gate** (kernel.rs lines 1-19) — copy verbatim:

```rust
//! Pure <name> kernel — <list pure fns>.
//!
//! Pattern analog: `aggregator.rs::{align_down, ...}` — private `#[inline]` pure
//! functions on primitive types with a sibling `#[cfg(test)] mod tests` block.
//! No IO, no `serde_json`, no `Reader` calls.

#![cfg_attr(any(test, debug_assertions), allow(clippy::float_cmp))]

use statrs::distribution::ChiSquared;            // or Normal / StudentsT / FisherSnedecor per scan
use statrs::distribution::ContinuousCDF;
```

**Pure-function shape** (kernel.rs lines 26-28 for `log_returns`, 53-73 for `biased_acf`, 100-132 for `ljung_box_q_and_p`):

```rust
#[inline]
pub(super) fn <kernel_fn>(input: &[f64], param: <usize|f64>) -> <KernelOutput> {
    // pure body
}
```

Discipline rules (carry verbatim from Phase 3 — see `04-RESEARCH.md` §Section 8 lines 1357-1366):

1. `#[inline]` on every kernel function (lines 26, 48, 91).
2. `pub(super)` not `pub` (kernels stay internal to the scan module).
3. `statrs` is the ONLY distribution path — no hand-rolled CDF approximations.
4. `debug_assert!` for kernel invariants (kernel.rs lines 105-109): `debug_assert!(max_lag >= 1, "ljung_box_q_and_p: max_lag must be >= 1");`.
5. Sequential / cumsum-style summation order for AIC/Q-stat-style accumulators (lines 115-130) — pin determinism across platforms (Pitfall 4).
6. Constant-input branch: `if denom == 0.0 { out.push(0.0); continue; }` (lines 63-66) — avoids NaN propagation.
7. `#[allow(clippy::cast_precision_loss, reason = "...")]` annotations with the SAME reason text Phase 3 used (lines 49-52, 92-95, 96-99).

**Test block** (kernel.rs lines 138-317) — copy verbatim structure: `const TOL: f64 = 1e-12; fn approx_eq(...) -> bool;` then named tests:

- `<fn>_basic` (smallest non-trivial input)
- `<fn>_empty` (zero-length slice)
- `<fn>_singleton` (single-element slice)
- `<fn>_constant_series` (zero-variance / NaN-trap edge)
- `<fn>_known_input` (HAND-DERIVED reference values pinned within 1e-12)
- `<fn>_length_invariant` (length contract)
- `#[should_panic(expected = "...")] <fn>_invalid_input_panics` (debug_assert pin)

### Pattern C — Two-leg CROSS scan body (Pair arity)

Applies to: CROSS-02, CROSS-03, CROSS-04, CROSS-05.

**Analog:** `crates/miner-core/src/scan/ljung_box/mod.rs` (same envelope construction) + `04-RESEARCH.md` lines 379-399 (two-leg pattern sketch) + `04-RESEARCH.md` lines 596-628 (inner-join body).

**Delta from Pattern A:**

1. `fn arity(&self) -> ScanArity { ScanArity::Pair }` (D4-02).
2. The `run` body reads two BarFrames. The Plan picks the API shape — recommendation from `04-RESEARCH.md` §1.5 is `ctx.bars_pair() -> (&BarFrame, &BarFrame)` OR `ctx.bars_for_leg(i)`. Either way the scan immediately calls `primitives::time_alignment::inner_join(bars_a, bars_b)?`:

```rust
let (bars_a, bars_b) = ctx.bars_pair();
let aligned = primitives::time_alignment::inner_join(bars_a, bars_b)?;
//      aligned.timestamps_ms : Vec<i64>     (the joint timestamp index)
//      aligned.close_a / close_b : Vec<f64> (price series, time-aligned)
let returns_a = primitives::returns::log_returns(&aligned.close_a);
let returns_b = primitives::returns::log_returns(&aligned.close_b);
// ... kernel call, envelope construction ...
```

3. `raw.series` carries leg-labelled keys: `returns_a`, `returns_b`, `timestamps_ms_aligned`.
4. `data_slice.sources: Vec<Source>` has length 2 (D4-03), parallel to `req.instruments` order.
5. `source` struct construction (lines 209-214 of LjungBox) becomes two parallel constructions iterating `req.instruments`.

### Pattern D — Vector-output (rolling / bucketed) finding

Applies to: ANOM-03 rolling-vol; CROSS-02, CROSS-03, CROSS-04; SEAS-01, SEAS-02, SEAS-03, SEAS-04, SEAS-06.

**Analog:** `crates/miner-core/src/scan/ljung_box/mod.rs` lines 131-155 (effect construction) — extended per `04-RESEARCH.md` Pattern 3 (lines 401-427).

**Delta from Pattern A:**

```rust
let effect = Effect {
    metric: "<canonical>".into(),
    value: <headline_scalar>,           // last_window / max_abs_t_stat / argmax_lag per §2 row
    p_value: None,                       // multi-window/multi-bucket -> no headline p
    n: Some(aligned_n as u64),
    ci95: None,                          // Phase 5 hygiene layer fills this
    extra: {
        let mut m = BTreeMap::new();
        m.insert("values".into(),            f64_slice_to_raw_array(&values));
        m.insert("window_starts_ms".into(),  f64_slice_to_raw_array(&window_starts));
        // ... per-scan extras per 04-RESEARCH.md §Section 2 ...
        m
    },
};
```

**Anti-pattern (REJECTED in `04-RESEARCH.md` Pitfall 1):** never emit one `Finding::Result` per window. Always one envelope per invocation; vectors in `effect.extra`.

### Pattern E — Registry bootstrap extension

**Analog:** `crates/miner-core/src/scan/registry.rs` lines 82-88.

Existing body:

```rust
#[must_use]
pub fn bootstrap() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(LjungBoxScan));
    // Phase 4 will add lines here, one per additional scan, alphabetical by id.
    r
}
```

Phase 4 extends with 22 lines, alphabetical by `scan_id` (the `bootstrap_registers_ljung_box_scan` test at line 150-162 will need its assertion count updated, and `registry_iter_lex_order` at line 170-178 starts being a meaningful (non-trivial) test).

### Pattern F — `Scan::arity()` trait method

**Analog:** `crates/miner-core/src/scan/mod.rs` lines 68-104 (Scan trait body) + lines 305-323 (ScanError enum) + line 352-356 (`scan_trait_object_safe` compile gate).

The new method goes in the trait block alongside `id`/`version`/`param_schema`/`finding_fields`/`run`. Recommended position: between `version` and `param_schema` so the doc-comment reading order matches the preflight call order (id-resolve → version-check → arity-check → param-validate → run). The `scan_trait_object_safe` test (line 352-356) MUST continue to pass — `arity(&self) -> ScanArity` is dyn-safe.

`ScanArity` enum lives next to the trait in `scan/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanArity {
    /// Single-leg scan (ANOM / SEAS family). `instruments.len() == 1`.
    Single,
    /// Two-leg scan (CROSS family). `instruments.len() == 2`.
    Pair,
    // v2: Many(min, max) — basket scans (Johansen etc.)
}
```

Analog for the enum shape: `crates/miner-core/src/engine/gap_policy.rs` lines 40-50 (`GapPolicyKind` enum: same derives, same `#[serde(rename_all = "snake_case")]`, sibling `as_str(&self) -> &'static str` method).

### Pattern G — `PreflightCode::WrongInstrumentArity` variant addition

**Analog:** `crates/miner-core/src/error/codes.rs` lines 20-53.

Existing enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreflightCode {
    InvalidParameter,
    UnknownScan,
    UnknownInstrument,
    MissingRequiredConfig,
    InvalidConfig,
    SweepTooLarge,
    InternalError,
}

impl PreflightCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PreflightCode::InvalidParameter => "invalid_parameter",
            // ... 6 more arms ...
            PreflightCode::InternalError => "internal_error",
        }
    }
}
```

Phase 4 adds the 8th variant `WrongInstrumentArity` (alphabetically after `UnknownScan`); the `as_str` arm returns `"wrong_instrument_arity"`. The existing `preflight_code_serialises_snake_case` test (codes.rs:140-161) gets one more `(WrongInstrumentArity, "wrong_instrument_arity")` row in the cases array.

### Pattern H — Preflight arity helper (engine::preflight::validate_arity)

**Analog:** `crates/miner-core/src/engine/preflight.rs` lines 51-73 (`resolve_scan_id_at_version`) + lines 79+ (`resolve_scan`).

Both existing helpers return `Result<_, WireError>` where the `Err` is constructed via `WireError::preflight(PreflightCode::X, "message").with_context("key", json_value)`. New helper:

```rust
pub fn validate_arity(scan: &dyn Scan, instruments: &[InstrumentSpec]) -> Result<(), WireError> {
    let expected = match scan.arity() {
        ScanArity::Single => 1,
        ScanArity::Pair => 2,
    };
    if instruments.len() != expected {
        return Err(WireError::preflight(
            PreflightCode::WrongInstrumentArity,
            format!("{} expects {} instrument(s); got {}", scan.id(), expected, instruments.len()),
        )
        .with_context("scan_id", serde_json::Value::String(scan.id().to_string()))
        .with_context("expected_arity", serde_json::Value::Number(expected.into()))
        .with_context("supplied_arity", serde_json::Value::Number(instruments.len().into())));
    }
    Ok(())
}
```

The engine call site (in `run_one_with_registry` line 239+) maps the `WireError` via `MinerError::Preflight(_)` — same pattern as the existing `preflight::resolve_scan(...).map_err(MinerError::Preflight)?` chain (line 240).

### Pattern I — Two-leg gap-policy intersection

**Analog:** `crates/miner-core/src/engine/gap_policy.rs` lines 99-180 (`GapDispatch` enum + `dispatch` fn) + `crates/miner-core/src/gap.rs` lines 83-117 (`GapManifest` + `GapSpan`).

**Plan picks home for the intersection helper.** Two viable locations:

- **(a)** `crates/miner-core/src/scan/primitives/time_alignment.rs::intersect_gaps(&GapManifest, &GapManifest) -> GapManifest` — co-located with the inner-join primitive (CROSS-01 owns both).
- **(b)** `crates/miner-core/src/gap.rs::GapManifest::intersect(&self, other: &Self) -> GapManifest` — extends the existing `GapManifest` impl.

Either way the engine dispatch grows a `Pair` branch in `run_one_with_registry` that:

1. fetches both BarFrames (with cancel-poll between, per `04-RESEARCH.md` §1.6);
2. computes each leg's manifest via the existing `GapDetector::detect` path;
3. intersects the two manifests (helper above);
4. calls `gap_policy::dispatch` (or a new `dispatch_pair`) with the intersected manifest;
5. emits `Finding::GapAborted` under strict + intersected non-empty, or partitions on the both-continuous sub-ranges under continuous_only.

`Finding::GapAborted` payload (defined in `findings/mod.rs` near `DataSlice`) carries the joint manifest under `data_slice.gap_manifest` — same struct field already exists; the Vec-of-sources extension (D4-03) makes each leg's per-leg manifest accessible as `data_slice.sources[i].symbol` + the joint `data_slice.gap_manifest`.

### Pattern J — Integration test (per-scan golden)

**Analog:** `crates/miner-core/tests/scan_ljung_box.rs` (entire file, 265 lines).

**Structure (verbatim 8-step walk):**

```rust
mod common;                                          // shared BufferSink + helpers

use miner_core::scan::<family>::<scan>::<Name>Scan;
use miner_core::scan::{Scan, ScanCtx, ScanRequest};
use common::BufferSink;

const GOLDEN_JSON: &str = include_str!("goldens/<scan_id>.jsonl");  // or .json for single-envelope
const FLOAT_TOLERANCE: f64 = <per §2 row>;                          // 1e-12 / 1e-10 / 1e-8 / 1e-6

#[test]
fn <scan>_matches_<reference>_golden() {
    // Step 1 — load the precomputed reference + provenance gate.
    let golden: serde_json::Value = serde_json::from_str(GOLDEN_JSON).expect("valid JSON");
    let prov_version = golden["provenance"]["statsmodels_version"].as_str();
    assert_eq!(prov_version, Some("0.14.6"), "regenerate after pinning statsmodels==0.14.6");

    // Step 2 — extract input arrays (close, returns, etc.).
    let close: Vec<f64> = golden["close"].as_array().unwrap()
        .iter().map(|v| v.as_f64().unwrap()).collect();

    // Step 3 — build BarFrame (uses fixture builder from common/synthetic_cache.rs).
    let bars = build_bar_frame_from_closes(&close);

    // Step 4 — construct ScanRequest + ScanCtx (D4-01 uses `instruments: Vec<InstrumentSpec>`).
    let req = ScanRequest { /* fields per Phase 4 facade */ };
    let ctx = ScanCtx { bars: &bars, gap_manifest: None, run_id: RunId::new(),
                        code_revision: "test-rev-abc1234",
                        cancel: Arc::new(AtomicBool::new(false)),
                        sleep_after_first_finding_ms: None };

    // Step 5 — dispatch.
    let mut sink = BufferSink::new();
    <Name>Scan.run(&ctx, &req, &mut sink).expect("ok");

    // Step 6 — parse the captured envelope.
    let findings = common::parse_findings(&sink.0);
    assert_eq!(findings.len(), 1);
    let Finding::Result(ref r) = findings[0] else { panic!("expected Result") };

    // Step 7 — element-by-element comparison against reference within tolerance.
    let golden_<key>: Vec<f64> = golden["<key>"].as_array().unwrap()
        .iter().map(|v| v.as_f64().unwrap()).collect();
    let rust_<key> = decode_f64_raw_array(&r.effect.extra, "<key>");
    assert_close(&rust_<key>, &golden_<key>, FLOAT_TOLERANCE, "<key> vs reference");

    // Step 8 — insta snapshot of the masked envelope.
    let mut masked = serde_json::to_value(&findings[0]).unwrap();
    common::mask_volatile_fields(&mut masked);
    insta::assert_json_snapshot!("<scan>_matches_<reference>_golden", masked);
}
```

The reusable helpers (`build_bar_frame_from_closes` lines 194-222, `decode_f64_raw_array` lines 228-248, `assert_close` lines 250-265) live inside each test file in Phase 3; Plan may lift them to `tests/common/mod.rs` if 22-fold duplication offends.

### Pattern K — CLI repeatable `--instrument SYMBOL:side` flag

**Analog:** `crates/miner-cli/src/scan_args.rs` lines 46-100 (existing single-instrument `ScanArgs`).

**Delta:**

1. Replace `pub instrument: String` (line 53) + `pub side: String` (line 57) with a repeatable, typed Vec:

```rust
/// Repeatable instrument flag — `--instrument SYMBOL:side`. Length must match
/// the scan's declared `arity()`. Preflight rejects mismatches with
/// `PreflightCode::WrongInstrumentArity`.
#[arg(long = "instrument", action = clap::ArgAction::Append, value_parser = parse_instrument_spec)]
pub instruments: Vec<InstrumentSpec>,
```

2. Add a `parse_instrument_spec(s: &str) -> Result<InstrumentSpec, String>` value-parser following the same shape as the existing `parse_window` value-parser (line 64). Splits on `:`, reuses `Side::from_str` (or hand-parses bid/ask), returns `InstrumentSpec { symbol: ..., side: ... }`.

3. `ScanArgs::to_scan_request` (currently in scan_args.rs or main.rs) constructs `ScanRequest { instruments: args.instruments, ... }` instead of `ScanRequest { instrument: args.instrument, side: parse_side(&args.side)?, ... }`.

`InstrumentSpec` lives in `miner-core::reader` (next to existing `Side` enum) or `miner-core::scan` (next to `ScanRequest`). Plan picks; the analog enum `Side` is at `crates/miner-core/src/reader.rs` (the existing single-field pattern).

### Pattern L — Look-ahead-safe `ctx.bars_up_to(ts)` API

**Analog:** `crates/miner-core/src/scan/mod.rs` lines 128-155 (`ScanCtx` struct definition — currently exposes raw `bars: &'a BarFrame`).

**`04-RESEARCH.md` §1.5 recommendation:** add a method (not a new field) on `ScanCtx`:

```rust
impl<'a> ScanCtx<'a> {
    pub fn bars_up_to(&self, ts: DateTime<Utc>) -> BarFrameView<'_> {
        let cutoff = self.bars.ts_open_utc.partition_point(|t| *t <= ts);
        BarFrameView {
            source_id: &self.bars.source_id,
            symbol: &self.bars.symbol,
            side: self.bars.side,
            tf: self.bars.tf,
            ts_open_utc: &self.bars.ts_open_utc[..cutoff],
            close: &self.bars.close[..cutoff],
            // ... open / high / low / tick_volume slices ...
        }
    }
}

pub struct BarFrameView<'a> {
    pub source_id: &'a str,
    pub symbol: &'a str,
    pub side: Side,
    pub tf: Timeframe,
    pub ts_open_utc: &'a [DateTime<Utc>],
    pub close: &'a [f64],
    pub open: &'a [f64],
    pub high: &'a [f64],
    pub low: &'a [f64],
    pub tick_volume: &'a [f64],
}
```

Analog for `BarFrameView` shape: existing `BarFrame` struct in `crates/miner-core/src/aggregator.rs` (referenced by `ScanCtx.bars: &'a BarFrame` already, so the field types are known).

### Pattern M — Shuffled-future regression extension (per rolling scan)

**Analog:** `crates/miner-core/tests/shuffled_future_regression.rs` lines 1-80 (existing N/T/LAGS constants + helpers + the LjungBox proptest body).

The existing file documents itself as the extension point (line 11): "Phase 4 will ADD additional cancellation_tests-style proptests for each new rolling/causal scan it introduces — it does NOT extend this proptest."

Plan 04 ADDs new proptest functions (one per rolling/causal scan: ANOM-03 vol-rolling, CROSS-02 corr-rolling, CROSS-03 ols-rolling, CROSS-04 lead-lag, and arguably the bucketed SEAS scans since their result vectors are timestamp-keyed) into the same file. Each proptest follows the same shape: build closes via `lcg_closes(N, seed)`, run the scan on the prefix `[0..T]`, shuffle `[T..N]`, run again on the full array, compare the prefix-relevant outputs (the per-window vector truncated at `T`) byte-for-byte.

## Shared Patterns

### Cancel polling (D3-22 — apply to every Phase 4 scan's `Scan::run`)

**Source:** `crates/miner-core/src/scan/ljung_box/mod.rs` lines 111-113.

```rust
if ctx.cancel.load(Ordering::Relaxed) {
    return Ok(());
}
```

**Apply to:** Entry of every `Scan::run` impl. Rolling/causal scans add a second poll inside the per-window loop (`04-RESEARCH.md` Pitfall 1 notes the loop bound is hundreds-of-thousands of iterations on a 2-year 15m series so the per-window check is mandatory; cost is one atomic load ~1ns per iteration). CROSS scans get a poll between the two `BarCache::get_or_build` calls (Pattern I + `04-RESEARCH.md` §1.6).

### `BTreeMap` discipline (OUT-03 — apply to every Phase 4 scan's effect.extra and raw.series)

**Source:** `crates/miner-core/src/scan/ljung_box/mod.rs` lines 134-138 (`effect.extra`) + lines 187-192 (`raw.series`).

```rust
let mut extra: BTreeMap<String, RawArray> = BTreeMap::new();
extra.insert("lags".into(), ...);
extra.insert("q_stats".into(), ...);
// ...

let mut series: BTreeMap<String, RawArray> = BTreeMap::new();
series.insert("returns".into(), ...);
series.insert("timestamps_ms".into(), ...);
let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;
```

**Apply to:** Every Phase 4 scan's envelope construction. NEVER `HashMap` (clippy `disallowed_types` gate enforces it). The compile-time check is `crates/miner-core/src/scan/registry.rs` line 140-143 (`registry_uses_btreemap` regression test pattern — replicate for any new `BTreeMap`-typed field).

### `Raw::new` enforces `timestamps_ms` invariant (D-03 — apply to every Phase 4 scan with raw.series)

**Source:** `crates/miner-core/src/scan/ljung_box/mod.rs` line 193 + `crates/miner-core/src/findings/mod.rs` lines 124-138.

```rust
let raw_block = Raw::new(series).map_err(|m| ScanError::Kernel(m.to_string()))?;
```

**Apply to:** Every scan emits `Raw` via `Raw::new(...)` (NEVER `Raw { series }` direct construction). The error case fires only if `timestamps_ms` was forgotten — the kernel test `<scan>_raw_new_enforces_timestamps_ms` (Pattern A's 13th named test) pins this.

### Stdout = findings, stderr = logs (D-15 — clippy disallowed_macros)

**Source:** Workspace `clippy.toml` (already in place from Phase 1). The Phase 4 scan modules inherit the gate automatically — no `println!`/`eprintln!` allowed outside `FindingSink::write_envelope` and the `tracing` adapter.

**Apply to:** Every Phase 4 source file. Verify by `cargo clippy --workspace --all-targets`.

### `param_hash` echoed into every finding (D3-13)

**Source:** `crates/miner-core/src/scan/ljung_box/mod.rs` line 198.

```rust
param_hash: req.param_hash.as_str().to_string(),
```

**Apply to:** Every `ResultFinding` construction. `param_hash` is a blake3 hex of `serde_json::to_vec(&resolved_params)`, computed once at the facade boundary (`engine::param_hash::param_hash` in Phase 3) and copied into every emitted finding for byte-stable provenance.

### Deterministic synthetic fixture (LCG-seeded BarFrame)

**Source:** `crates/miner-core/src/scan/ljung_box/mod.rs` lines 353-392 (`ar1_bar_frame_seeded`) — also at `crates/miner-core/tests/shuffled_future_regression.rs` lines 40-49 (`lcg_closes` standalone helper).

Numerical Recipes LCG constants `a = 1_664_525, c = 1_013_904_223, m = 2^32`; output scaled to `1.0 + (s / 2^32)` so closes land in `(1.0, 2.0)`; timestamps `2024-01-01 + 15m × i`. Deterministic, cross-platform, no `rand` crate dependency, no PRNG drift.

**Apply to:** Every per-scan integration test's BarFrame builder. Plan may lift to `tests/common/synthetic_cache.rs` if the duplication offends; the existing `tests/common/synthetic_cache.rs` already exists for Phase 3 cache tests and is a natural home.

### Volatile-field masking for insta snapshots

**Source:** `crates/miner-core/tests/common/mod.rs` (the `mask_volatile_fields` helper definition is around line 67+; the full body lives in the file).

```rust
pub fn mask_volatile_fields(v: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for key in ["run_id", "started_at_utc", "produced_at_utc", "ended_at_utc"] {
            if map.contains_key(key) { /* overwrite with constant placeholder */ }
        }
        // ... recurse ...
    }
}
```

**Apply to:** Every per-scan integration test's Step 8 `insta::assert_json_snapshot!` call site. The helper already exists; tests just import it via `mod common; use common::mask_volatile_fields;`.

## No Analog Found

Files with no close pattern in the codebase (planner relies on `04-RESEARCH.md` recommendations + external references):

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md` | docs (version pin table) | n/a | Net-new Wave 0 conventions file — Phase 3 had one pinned reference (statsmodels 0.14.6 for LjungBox) but no formal `REFERENCE-VERSIONS.md`. `04-RESEARCH.md` §1.2 dictates content. |
| `crates/miner-core/tests/goldens/python-requirements.lock` | pip lock | n/a | Net-new — Phase 3's `tests/fixtures/generate_golden.py` ran ad-hoc with no committed lock. `04-RESEARCH.md` §1.2 + Open Question 1 dictate the lock-file approach. |
| `crates/miner-core/src/scan/primitives/time_alignment.rs` | kernel | inner-join two BarFrames | The CROSS-01 inner-join body itself is novel (Phase 3 never aligned two BarFrames). The CODE shape of "private `#[inline] pub(super) fn` over `&[f64]` slices returning a typed output" matches `ljung_box/kernel.rs` exactly; the algorithm is new and lives in `04-RESEARCH.md` lines 596-628. |
| `crates/miner-core/src/scan/anom/{adf,kpss,variance_ratio,arch_lm,drawdown}.rs` kernels | hand-rolled statistics | n/a | ADF/KPSS/Lo-MacKinlay/ARCH-LM/drawdown are NOT covered by any comprehensive Rust crate (`04-RESEARCH.md` "Don't Hand-Roll" table line 446 + STATE.md "Phase 4 implementation risk"). Each is hand-derived against scipy/statsmodels/arch references during plan-phase. The kernel-FILE shape (`mod.rs`+`kernel.rs` split, `#[inline] pub(super)` pure fns, `statrs` CDFs only, sequential summation, debug_assert invariants, kernel-tests with 1e-12 tolerance) still matches `ljung_box/kernel.rs` exactly. |

## Metadata

**Analog search scope:**
- `crates/miner-core/src/scan/` — all subdirectories (Phase 3 scan framework + LjungBox)
- `crates/miner-core/src/engine/` — facade, preflight, gap_policy
- `crates/miner-core/src/findings/` — envelope types
- `crates/miner-core/src/error/` — typed code enums
- `crates/miner-core/tests/` — integration test patterns (scan_ljung_box, gap_policy, shuffled_future_regression, dry_run)
- `crates/miner-cli/src/` — CLI arg patterns
- `crates/miner-core/src/{gap,aggregator,cache,calendar,reader}.rs` — supporting types

**Files scanned:** 23 Phase-3 source files + 4 Phase-3 test files = 27.

**Stop criterion hit:** Phase 3's `LjungBoxScan` is the single dominant analog (research deck explicitly calls it "the gold-standard reference pattern"). Found exactly the right number of analogs (one per pattern class A-M); no value in broader search.

**Key insight for planner:** Every Phase 4 file is a `LjungBoxScan` clone or a small additive extension of an existing Phase 3 surface. The work is high in count (22 scans + facade extensions) but low in novelty per file — the planner can use Pattern A as a template body and parameterise across the 22 §Section 2 rows in `04-RESEARCH.md` to author plan task specs.

**Pattern extraction date:** 2026-05-19

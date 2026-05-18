# Phase 3: Scan Engine, Facade & CLI — Pattern Map

**Mapped:** 2026-05-18
**Files analysed:** 23 new/extended source files + 9 new test files
**Analogs found:** 23 / 23 source files (every one has a strong existing analog from Phase 1 or Phase 2)

The Phase 3 codebase already contains canonical patterns for every shape Phase 3 introduces: tagged-enum additivity (`Finding` / `GapReason`), `Send + Sync` traits with associated types (`Reader`, `FindingSink`), `BTreeMap`-everywhere serde discipline (`WireError.context`, `RunSummary.per_scan`), pure-function kernels with `#[cfg(test)] mod tests` blocks (`aggregator.rs`, `gap.rs`), the `error::codes` typed-enum → `as_str` → `WireError::preflight` pipeline, the `BarCache::get_or_build` "single-entry facade returning by value" shape, the `Blake3Hex::from_hex_bytes(hash.to_hex().as_bytes().try_into())` hashing idiom, and the `assert_cmd::Command::cargo_bin("miner") + env_clear() + #[serial_test::serial]` integration-test shape. **Every new Phase 3 file copies an existing analog rather than inventing a new shape.**

---

## File Classification

### Source files

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/miner-core/src/scan/mod.rs` | trait | request-response | `crates/miner-core/src/reader.rs` (`Reader` trait) + `crates/miner-core/src/findings/sink.rs` (`FindingSink` trait) | exact |
| `crates/miner-core/src/scan/registry.rs` | service | catalogue-lookup | `crates/miner-core/src/cache.rs` (`BarCache` constructor + `get_or_build`) + `crates/miner-core/src/findings/mod.rs` (`BTreeMap` discipline) | role-match |
| `crates/miner-core/src/scan/shape.rs` | model | n/a | `crates/miner-core/src/findings/mod.rs` (`PerScanCounts` declarative struct) | role-match |
| `crates/miner-core/src/scan/ljung_box/mod.rs` | service (Scan impl) | transform | `crates/miner-core/src/aggregator.rs` (`aggregate` pure kernel + reader trait usage) | role-match |
| `crates/miner-core/src/scan/ljung_box/kernel.rs` | utility (pure kernel) | transform | `crates/miner-core/src/aggregator.rs::{align_down, emit_bucket}` (private kernels with `#[cfg(test)] mod tests`) | exact |
| `crates/miner-core/src/engine/mod.rs` | service (facade) | request-response | `crates/miner-core/src/cache.rs::BarCache::get_or_build` (single-entry facade returning a value, calling helpers) | role-match |
| `crates/miner-core/src/engine/preflight.rs` | utility (error mapping) | transform | `crates/miner-cli/src/main.rs::classify_figment_error` + `crates/miner-core/src/error/codes.rs::{WireError::preflight, PreflightCode::as_str}` | exact |
| `crates/miner-core/src/engine/gap_policy.rs` | service (dispatch) | transform | `crates/miner-core/src/gap.rs::GapDetector::detect` (stateless unit struct + pure function dispatch) | role-match |
| `crates/miner-core/src/engine/param_hash.rs` | utility (hashing) | transform | `crates/miner-reader-dukascopy/src/reader.rs::fingerprint_day` (the `blake3::hash` → `to_hex` → `Blake3Hex::from_hex_bytes` idiom) | exact |
| `crates/miner-core/src/engine/framing.rs` | utility (builders) | transform | `crates/miner-cli/src/main.rs::emit_fixture` (the existing `RunStart`/`RunEnd` construction pattern with shared `RunId: Copy`) | exact |
| `crates/miner-core/src/findings/mod.rs` (extend) | model | n/a | self (existing 5-variant `Finding` enum + `DataSlice` shape) | exact |
| `crates/miner-cli/src/cli.rs` (extend) | controller (CLI parser) | request-response | self (existing `Command::EmitFixture` + global flags pattern) | exact |
| `crates/miner-cli/src/scan_args.rs` | controller (clap args) | request-response | `crates/miner-cli/src/cli.rs::{Cli, CliOverrides flow}` (clap derive + override-conversion pattern) | exact |
| `crates/miner-cli/src/main.rs` (extend) | controller (entry) | request-response | self (existing tracing init + `Cli::parse` + `classify_figment_error` + `make_sink` + `emit_fixture` flow) | exact |
| `crates/miner-core/src/lib.rs` (extend) | config (re-export surface) | n/a | self (existing `pub use` FROZEN block) | exact |

### Test files

| New File | Role | Closest Analog | Match Quality |
|----------|------|----------------|---------------|
| `crates/miner-core/tests/scan_ljung_box.rs` | snapshot/golden test | `crates/miner-core/tests/gap_manifest_snapshot.rs` (insta JSON snapshot of a synthetic envelope) | exact |
| `crates/miner-core/tests/scan_facade_determinism.rs` | byte-identity test | `crates/miner-cli/tests/cli_streams.rs::emit_fixture_byte_identical_when_volatile_fields_masked` (Test 7) + `crates/miner-core/tests/full_determinism.rs` | exact |
| `crates/miner-core/tests/shuffled_future_regression.rs` | proptest | `crates/miner-core/tests/cache_smoke.rs::arrow_bytes_deterministic_under_shuffled_construction` (proptest harness layout) | exact |
| `crates/miner-core/tests/gap_policy.rs` | behaviour test | `crates/miner-core/tests/cache_smoke.rs` (five named tests against a synthetic substrate, one per VALIDATION row) | exact |
| `crates/miner-core/tests/dry_run.rs` | behaviour test | `crates/miner-core/tests/cache_smoke.rs` (single-scenario integration test using FROZEN surface only) | role-match |
| `crates/miner-cli/tests/scan_subcommand_smoke.rs` | assert_cmd integration | `crates/miner-cli/tests/cli_streams.rs::run_emit_fixture_happy` (assert_cmd + env_clear + `#[serial_test::serial]`) | exact |
| `crates/miner-cli/tests/scans_catalogue.rs` | assert_cmd integration | `crates/miner-cli/tests/cli_streams.rs` Test 1 (parse stdout lines, assert kind) | exact |
| `crates/miner-cli/tests/sigint_preserves_stream.rs` | `#[cfg(unix)]` integration | `crates/miner-cli/tests/cli_streams.rs::run_emit_fixture_happy` + RESEARCH §"Code Examples" `nix::kill` snippet (lines 693-742) | role-match |
| `crates/miner-cli/tests/fixtures/` | test data builder | `crates/miner-core/tests/full_determinism.rs` (inlined `SyntheticCache` builder; uses public reader API) | role-match |

---

## Pattern Assignments

### `crates/miner-core/src/scan/mod.rs` — `Scan` trait + `ScanCtx` + `ScanRequest` + `ScanError` + `ScanFindingShape`

**Primary analog:** `crates/miner-core/src/reader.rs` (the `Reader` trait — `Send + Sync` with an associated `Error` type, `&'static str` source id, dyn-compatible).
**Secondary analog:** `crates/miner-core/src/findings/sink.rs` (`FindingSink: Send` trait + the `Box<dyn FindingSink>` boxed-trait-object pattern at line 35-50).

**Module-doc header pattern** (mirror `reader.rs:1-30` and `sink.rs:1-17`):

```rust
//! `Scan` trait + supporting types — D3-14 / Phase 3.
//!
//! Every scan is a `Send + Sync` polymorphic compute kernel registered in the
//! [`crate::scan::registry::Registry`] and dispatched by [`crate::engine::run_one`].
//! Implementations: [`crate::scan::ljung_box::LjungBoxScan`] (Phase 3 demo);
//! Phase 4 adds 21 more.
```

**Trait signature pattern** (lines `reader.rs:198-201` — `Send + Sync` + associated `Error` + `&'static str` ids):

```rust
// Source: reader.rs:198-244 — the canonical Send+Sync trait shape with type Error + &'static str ids.
pub trait Reader: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    fn source_id(&self) -> &'static str;
    fn trading_calendar(&self) -> Calendar;
    fn read_1m_bars<'a>(&'a self, symbol: &str, side: Side, range: ClosedRangeUtc)
        -> Result<RawBarIter<'a, Self::Error>, Self::Error>;
    // ...
}
```

→ The Phase 3 `Scan` trait MUST follow the same shape (see CONTEXT D3-14 + RESEARCH Pattern 2). One difference: `Scan` does NOT need an associated `Error` type — RESEARCH Pattern 2 ships a `ScanError` enum directly (with `thiserror` per `aggregator.rs:202`), because scans share a single error shape (kernel/io/cancel) whereas readers each have their own (`DukascopyError`, future `PolygonError`, etc.).

**Dyn-compatibility regression gate** (copy `reader.rs:272-274`):

```rust
// Source: reader.rs:272-274 — compile-time gate that the trait stays object-safe.
#[test]
fn scan_trait_object_safe() {
    fn _accept(_s: &dyn crate::scan::Scan) {}
}
```

**`ScanFindingShape` declarative struct** (mirror `findings/mod.rs:200-205` `PerScanCounts`):

```rust
// Source: findings/mod.rs:200-205 — tiny Copy struct with #[derive(Default,…)] for catalogue introspection.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PerScanCounts {
    pub results: u64,
    pub errors: u64,
    pub gap_aborted: u64,
}
```

→ `ScanFindingShape` is the equivalent ANNOUNCEMENT-of-shape struct for `miner scans` introspection. RESEARCH Pattern 2 (lines 334-337) lays out the exact shape:

```rust
pub struct ScanFindingShape {
    pub effect_extra_keys: &'static [&'static str],
    pub raw_series_keys:   &'static [&'static str],
}
```

**`ScanError` enum pattern** (mirror `aggregator.rs:201-219` `AggregateError`):

```rust
// Source: aggregator.rs:201-219 — thiserror-derived enum with #[from] / #[source]; NO Serialize derive.
#[derive(Debug, thiserror::Error)]
pub enum AggregateError<RE>
where
    RE: std::error::Error + 'static,
{
    #[error("reader error: {0}")]
    Reader(#[source] RE),
    #[error("range.start {start} is not aligned to {tf:?} boundary")]
    MisalignedRange { start: DateTime<Utc>, tf: Timeframe },
}
```

→ `ScanError` follows the same idiom: `thiserror::Error`, no `Serialize` derive (kernel errors become `Finding::ScanError` via the engine's `ScanErrorCode::as_str` mapping; serde stays at the engine boundary).

---

### `crates/miner-core/src/scan/registry.rs` — `Registry::new()` / `register()` / `get()` / `iter()` + `bootstrap()`

**Primary analog:** `crates/miner-core/src/cache.rs::BarCache` (constructor + `get_or_build` single-method facade returning a value at line 519-534) + the workspace `BTreeMap` discipline (`findings/mod.rs:29-31` doc-block + every `BTreeMap` field in `RunSummary`, `Effect`, `Raw`).

**Construction pattern** (copy `cache.rs:519-534`):

```rust
// Source: cache.rs:519-534 — fielded struct with #[must_use] constructor taking `impl Into<…>`.
pub struct BarCache {
    pub cache_root: PathBuf,
}
impl BarCache {
    #[must_use]
    pub fn new(cache_root: impl Into<PathBuf>) -> Self {
        Self { cache_root: cache_root.into() }
    }
    // ... single-method facade `get_or_build` ...
}
```

→ `Registry` follows the same shape but the inner field is a `BTreeMap<(String, u32), Box<dyn Scan>>` (per CONTEXT line 204 "the only Phase 3 map type") and the methods are `register(&mut self, Box<dyn Scan>)` / `get(&self, id: &str, version: u32) -> Option<&dyn Scan>` / `iter(&self) -> impl Iterator<Item = &dyn Scan>`.

**`bootstrap()` pattern** (CONTEXT D3-16 lines 97-103 — explicit registration, NO `inventory` crate):

```rust
// Source: CONTEXT D3-16 — explicit bootstrap. Phase 4 extends with one line per scan.
pub fn bootstrap() -> Registry {
    let mut r = Registry::new();
    r.register(Box::new(LjungBoxScan));
    r
}
```

**`BTreeMap` invariant** (mirror `findings/mod.rs:101-103` doc-line and `error/codes.rs:96-98` `WireError.context`):

```rust
// Source: findings/mod.rs:101-103 — the canonical "BTreeMap NEVER HashMap" doc-line.
/// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
pub series: BTreeMap<String, RawArray>,
```

→ `Registry::scans` carries the same doc-line verbatim. The audit test (analog `tests/raw_series_uses_btreemap` at `findings/mod.rs:514-532`) becomes `registry_uses_btreemap`:

```rust
// Source: findings/mod.rs:515-527 — compile-time type assertion via `let _: &BTreeMap<…> = &…;`.
let _: &BTreeMap<(String, u32), Box<dyn Scan>> = &registry.scans;
```

---

### `crates/miner-core/src/scan/shape.rs` — `ScanFindingShape`

**Analog:** `crates/miner-core/src/findings/mod.rs:200-205` `PerScanCounts` (a tiny `Copy` + `#[derive(Default, …, JsonSchema)]` declarative struct used by introspection).

Direct excerpt of the analog already shown above under `scan/mod.rs`. Phase 3 may either inline `ScanFindingShape` in `scan/mod.rs` (since it's tiny) or split into `shape.rs` for symmetry with `findings/mod.rs` exporting its own variants. Both are acceptable per RESEARCH §"Recommended Project Structure" line 224.

---

### `crates/miner-core/src/scan/ljung_box/mod.rs` — `LjungBoxScan: Scan` impl

**Primary analog:** `crates/miner-core/src/aggregator.rs::aggregate` (pure-kernel function calling a `Reader`, returning a `BarFrame`).
**Secondary analog:** `crates/miner-core/src/gap.rs::GapDetector::detect` (stateless unit struct + algorithm dispatch).

**Algorithm-doc header pattern** (mirror `gap.rs:157-187` — explicit numbered algorithm walk in the doc comment):

```rust
// Source: gap.rs:158-186 — numbered algorithm doc with explicit per-step rules.
/// Walk every calendar day in `range`, classify any missing minutes during open
/// hours against the reader's [`Calendar`], and return a sorted [`GapManifest`].
///
/// ## Algorithm
///
/// 1. Enumerate every UTC date in `[range.start, range.end)`.
/// 2. For each date, compute the open-hours minute set ...
/// ...
```

**Kernel-construction pattern** (mirror `aggregator.rs:281-369`):

```rust
// Source: aggregator.rs:281-309 — pure function calling reader, accumulating into a typed output.
pub fn aggregate<R: Reader>(reader: &R, params: AggParams<'_>) -> Result<BarFrame, AggregateError<R::Error>> {
    if !validate_range_alignment(params.range.start, params.tf) {
        return Err(AggregateError::MisalignedRange { /* ... */ });
    }
    let mut frame = BarFrame { /* fielded init */ };
    let iter = reader.read_1m_bars(params.symbol, params.side, params.range)
        .map_err(AggregateError::Reader)?;
    // ... single fold loop accumulating into `frame` ...
    Ok(frame)
}
```

→ `LjungBoxScan::run` is structurally identical but reads `ctx.bars: &BarFrame` (already aggregated by the facade) and writes one `Finding::Result` to `sink`. The full sketch in RESEARCH Pattern 3 lines 366-411 IS the pattern.

**Cancellation polling pattern** (RESEARCH Pattern 4 line 400 — single check at start because Ljung-Box is single-shot):

```rust
// Source: 03-RESEARCH.md Pattern 4 + CONTEXT D3-22. Phase 4 rolling stats poll between rows.
if cancel.load(std::sync::atomic::Ordering::Relaxed) {
    return Ok(());
}
```

---

### `crates/miner-core/src/scan/ljung_box/kernel.rs` — pure `log_returns` / `biased_acf` / `ljung_box_q_and_p`

**Analog:** `crates/miner-core/src/aggregator.rs::{align_down (line 236), validate_range_alignment (line 258), emit_bucket (line 375)}` — private `#[inline]` pure functions on primitive types with `#[cfg(test)] mod tests` blocks.

**Pure-kernel pattern** (copy `aggregator.rs:235-253`):

```rust
// Source: aggregator.rs:235-253 — #[inline] private pure function over primitive types.
#[inline]
fn align_down(ts: DateTime<Utc>, tf: Timeframe) -> DateTime<Utc> {
    let t0 = ts
        .with_second(0)
        .and_then(|t| t.with_nanosecond(0))
        .expect("zeroing sub-minute fields is always valid");
    match tf {
        Timeframe::Tf15m => t0.with_minute((t0.minute() / 15) * 15).expect("..."),
        // ...
    }
}
```

→ `biased_acf`, `ljung_box_q_and_p`, `log_returns` are `#[inline]` pure functions over `&[f64]` / `usize`. The body is already written verbatim in RESEARCH §"Code Examples" lines 644-685 — copy as-is, NOT through `serde_json` and NOT through the sink (these are sub-kernel-level, called by `LjungBoxScan::run`).

**Unit-test discipline** (copy `aggregator.rs:400-700` `#[cfg(test)] mod tests` layout):

```rust
// Source: aggregator.rs::tests — #[cfg(test)] mod tests at the bottom; one `fn` per behavior.
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn raw_bar_strategy(index: i64) -> impl Strategy<Value = RawBar> {
        // ...
    }

    #[test]
    fn aligns_to_15m_boundary() {
        // ...
    }
    // ...
}
```

→ Phase 3 kernels add tests like `acf_matches_statsmodels_at_k1` against a tiny hard-coded vector (CONTEXT D3-05); the kernel is pure so no fixture / no IO / no `serde_json`.

---

### `crates/miner-core/src/engine/mod.rs` — `run_one` facade + `RunOutcome` enum

**Primary analog:** `crates/miner-core/src/cache.rs::BarCache::get_or_build` (lines 569-573 + 533-560 algorithm walk). The single-entry facade that owns the orchestration and returns `Result<…, …Error>` to the caller.
**Secondary analog:** `crates/miner-cli/src/main.rs::main` (lines 34-74) — the existing precedence-then-dispatch flow.

**Single-entry facade signature pattern** (copy `cache.rs:569-573`):

```rust
// Source: cache.rs:569-573 — facade method returning a value with a multi-line algorithm doc.
pub fn get_or_build<R: Reader>(
    &self,
    reader: &R,
    params: AggParams<'_>,
) -> Result<BarFrame, CacheError> {
    // 1. Read existing sidecar ...
    // 2. Compare aggregator_version + arrow_schema_version → full rebuild on mismatch.
    // 3. Enumerate source days; compute current per-day fingerprints.
    // ...
}
```

→ `engine::run_one` follows the same shape:

```rust
// New — but copies the facade pattern verbatim from cache.rs.
pub fn run_one<R: Reader>(
    req: &ScanRequest,
    cfg: &MinerConfig,
    reader: &R,
    sink: &mut dyn FindingSink,
    cancel: Arc<AtomicBool>,
) -> Result<RunOutcome, MinerError> {
    // 1. Cancel-check early (RESEARCH Pattern 4 polling site 1).
    // 2. Preflight: resolve scan from registry, validate params (engine/preflight.rs).
    // 3. Emit RunStart (engine/framing.rs).
    // 4. Build BarFrame via BarCache::get_or_build.
    // 5. Detect gaps via GapDetector::detect; dispatch (engine/gap_policy.rs).
    // 6. For each sub-range: call Scan::run with a fresh ScanRequest::sub_range.
    // 7. Emit RunEnd. Return RunOutcome::{Ok | HadScanErrors | PreflightFailed}.
}
```

**`RunOutcome` enum pattern** (mirror `gap.rs:117-129` `GapReason` — tagged enum, no `f64`, derive `Eq`):

```rust
// Source: gap.rs:117-130 — tagged enum with snake_case wire form; no Serialize needed for RunOutcome (internal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Ok,
    HadScanErrors,
    PreflightFailed,
}
```

→ `RunOutcome` is INTERNAL to the facade contract (CLI maps to exit code), so no `Serialize` / `JsonSchema` needed; the analog still applies for variant naming.

**Tracing discipline** (copy `cache.rs:592-598`):

```rust
// Source: cache.rs:593-598 — every state transition through tracing::*, NEVER println.
tracing::info!(
    symbol = %params.symbol,
    side = ?params.side,
    tf = %params.tf.as_str(),
    old_aggregator = %sc.aggregator_version,
    new_aggregator = AGGREGATOR_VERSION,
    "rebuild triggered: aggregator_version drift",
);
```

→ `engine::run_one` emits `tracing::info!` at each polling site / transition; never `println!` / `eprintln!` (workspace `clippy.toml` bans them).

---

### `crates/miner-core/src/engine/preflight.rs` — `--params` parser + error mapping

**Primary analog:** `crates/miner-cli/src/main.rs::classify_figment_error` (lines 107-130) — typed-figment-error → typed `PreflightCode` mapper.
**Secondary analog:** `crates/miner-core/src/error/codes.rs::{PreflightCode::as_str, WireError::preflight}` (lines 39-53 + 102-110).

**Error-mapping pattern** (copy `main.rs:107-130`):

```rust
// Source: main.rs:107-130 — match on typed kind, fall-through to a default.
fn classify_figment_error(err: &figment::Error) -> PreflightCode {
    use figment::error::Kind;
    let first_kind = err.clone().into_iter().next()
        .map_or(Kind::Message(String::new()), |e| e.kind);
    match first_kind {
        Kind::MissingField(_) => PreflightCode::MissingRequiredConfig,
        Kind::InvalidType(_, _)
        | Kind::InvalidValue(_, _)
        | /* ... */
        | Kind::UnsupportedKey(_, _) => PreflightCode::InvalidConfig,
    }
}
```

→ Phase 3 adds `classify_param_error(err: serde_json::Error) -> PreflightCode` (always `InvalidParameter`) and `classify_scan_lookup(id: &str, version: u32, registry: &Registry) -> Result<&dyn Scan, PreflightCode>` (returns `UnknownScan` on miss). Same dispatch idiom.

**`WireError` construction pattern** (copy `error/codes.rs:101-110`):

```rust
// Source: error/codes.rs:101-110 — typed PreflightCode + message → WireError via ::preflight.
impl WireError {
    #[must_use]
    pub fn preflight(code: PreflightCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: message.into(),
            context: BTreeMap::new(),
        }
    }
}
```

→ Phase 3 preflight always builds errors via `WireError::preflight(PreflightCode::X, "human msg").with_context("key", json_value)` (lines 122-127). Never construct `WireError { code: "invalid_parameter".to_string(), ... }` by hand — go through the typed constructor.

**Emission pattern** (copy `error/stderr_emit.rs:40-58`):

```rust
// Source: error/stderr_emit.rs:40-58 — serialize via serde_json::to_writer, append \n, flush.
pub fn emit_to_stderr(err: &WireError) -> io::Result<()> {
    write_preflight_error(&mut io::stderr(), err)
}
```

→ The CLI's `main()` (not preflight.rs itself) calls `emit_to_stderr` after the facade returns `RunOutcome::PreflightFailed`. `preflight.rs` returns the `WireError` value; the binary's `main.rs` emits.

---

### `crates/miner-core/src/engine/gap_policy.rs` — strict / continuous_only dispatch + partitioning

**Primary analog:** `crates/miner-core/src/gap.rs::GapDetector` (lines 152-260) — stateless unit struct + pure-function dispatch.

**Stateless-unit-struct + algorithm pattern** (copy `gap.rs:152-187`):

```rust
// Source: gap.rs:152-187 — stateless unit struct (no fields), one pure function with a numbered algorithm doc.
#[derive(Debug, Default, Clone, Copy)]
pub struct GapDetector;

impl GapDetector {
    /// Walk every calendar day in `range`, classify any missing minutes during open
    /// hours against the reader's [`Calendar`], and return a sorted [`GapManifest`].
    ///
    /// ## Algorithm
    /// 1. Enumerate every UTC date in `[range.start, range.end)`.
    /// ...
    pub fn detect<R: Reader>(
        reader: &R,
        symbol: &str,
        side: Side,
        range: ClosedRangeUtc,
    ) -> Result<GapManifest, R::Error> {
        // ...
    }
}
```

→ `GapPolicy::dispatch(manifest: &GapManifest, requested: ClosedRangeUtc, policy: GapPolicyKind) -> GapDispatch` is a stateless function; in CONTEXT D3-11/D3-12 the dispatch returns either `GapDispatch::Aborted(GapManifest)` (strict + gaps) or `GapDispatch::SubRanges(Vec<TimeRange>)` (continuous_only). The `GapPolicyKind` enum follows `GapReason`'s tagged-enum shape (`gap.rs:117-130`):

```rust
// Source: gap.rs:117-130 — tagged-enum with rename_all = "snake_case", #[derive(JsonSchema)].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GapReason { /* ... */ }
```

→ `GapPolicyKind { Strict, ContinuousOnly }` is a simple unit-variant enum (mirror `Side::{Bid, Ask}` in `reader.rs:58-90` — `#[serde(rename_all = "snake_case")]`, `as_str(&self) -> &'static str`).

---

### `crates/miner-core/src/engine/param_hash.rs` — `param_hash(resolved: &Value) -> Blake3Hex`

**Primary analog:** `crates/miner-reader-dukascopy/src/reader.rs::fingerprint_day` (lines 123-141) — the `blake3::hash(&bytes) → hash.to_hex() → bytes.try_into() → Blake3Hex::from_hex_bytes` idiom.

**Hashing pattern** (copy `dukascopy/src/reader.rs:128-140`):

```rust
// Source: miner-reader-dukascopy/src/reader.rs:128-140 — the canonical blake3 → Blake3Hex pipeline.
fn fingerprint_day(/* ... */) -> Result<Option<Blake3Hex>, Self::Error> {
    let Some(DayBytes { bytes, .. }) = self.read_day_bytes(symbol, side, date)? else {
        return Ok(None);
    };
    let hash = blake3::hash(&bytes);
    let hex = hash.to_hex();
    let bytes64: [u8; 64] = hex.as_bytes().try_into().map_err(|_| {
        DukascopyError::HexDecode(format!(
            "blake3 hex returned unexpected length: {}",
            hex.len()
        ))
    })?;
    Ok(Some(Blake3Hex::from_hex_bytes(&bytes64)))
}
```

→ `engine::param_hash::param_hash` is structurally identical but its bytes come from `serde_json::to_vec(resolved)?` (CONTEXT D3-13). RESEARCH §"Code Examples" lines 630-639 has the exact 6-line implementation:

```rust
// Source: 03-RESEARCH.md Code Examples §"Computing param_hash (D3-13)".
fn param_hash(resolved: &serde_json::Value) -> Result<Blake3Hex, serde_json::Error> {
    let bytes = serde_json::to_vec(resolved)?;
    let hash  = blake3::hash(&bytes);
    let bytes64: [u8; 64] = hash.to_hex().as_bytes().try_into()
        .expect("blake3 hex is always 64 chars");
    Ok(Blake3Hex::from_hex_bytes(&bytes64))
}
```

**Determinism gate test** (mirror `aggregator_determinism.rs:48-100` `byte_identical_two_runs` style):

```rust
// Source: tests/aggregator_determinism.rs:48-100 — two-runs byte-equality.
#[test]
fn param_hash_is_byte_stable() {
    let v1 = serde_json::json!({"lags": 20});
    let v2 = serde_json::json!({"lags": 20});
    assert_eq!(param_hash(&v1).unwrap().as_str(), param_hash(&v2).unwrap().as_str());
}
```

**PITFALL** (RESEARCH §Pitfall 6 line 589-597): `param_hash`'s input is ONLY the resolved scan params, NEVER the `RunStart.request` value (which contains `run_id` / timestamps).

---

### `crates/miner-core/src/engine/framing.rs` — `RunStart` / `RunEnd` builders

**Analog:** `crates/miner-cli/src/main.rs::emit_fixture` (lines 140-165) — the existing `RunStart` + `RunEnd` construction with shared `RunId: Copy`.

**Framing-builder pattern** (copy `main.rs:140-165`):

```rust
// Source: miner-cli/src/main.rs:140-165 — shared RunId via Copy; clock reads only at framing boundaries.
fn emit_fixture(sink: &mut dyn FindingSink) -> anyhow::Result<()> {
    let run_id = RunId::new();
    let started = chrono::Utc::now();

    let start = Finding::RunStart(RunStart {
        run_id,                              // RunId: Copy
        started_at_utc: started,
        miner_version: env!("CARGO_PKG_VERSION").to_string(),
        code_revision: miner_core::CODE_REVISION.to_string(),
        request: serde_json::json!({ "command": "emit-fixture" }),
    });
    sink.write_envelope(&start)?;

    let ended = chrono::Utc::now();
    let end = Finding::RunEnd(RunEnd {
        run_id,                              // Copy again — only legal because RunId: Copy.
        ended_at_utc: ended,
        wall_clock_ms: ended.signed_duration_since(started).num_milliseconds(),
        summary: RunSummary::default(),
    });
    sink.write_envelope(&end)?;

    sink.flush()?;
    Ok(())
}
```

→ Phase 3 lifts this into `engine/framing.rs` as `build_run_start(req, run_id, started)` / `build_run_end(run_id, started, summary)` pure builders returning `Finding` values. The CLI's `emit_fixture` stays as a smoke test; `run_one` calls the new builders. Crucially: **clock reads (`Utc::now()`) live ONLY in framing builders, NEVER in `Scan::run` / kernels** (CONTEXT D3-23).

**`request: serde_json::Value` shape**: per RESEARCH Pitfall 6, the `request` echo contains run-level metadata (scan_id@version, instrument, side, timeframe, window, gap_policy, resolved_params) but is built SEPARATELY from the canonical params blob fed to `param_hash`.

---

### `crates/miner-core/src/findings/mod.rs` (extend) — `DataSlice.gap_manifest` field + `Finding::DryRun` variant + `DryRunFinding`

**Analog:** self — every existing variant of `Finding` (lines 213-282) and `DataSlice` (lines 68-72).

**Field-additivity pattern** (mirror `DataSlice.gap_manifest_ref` at line 71):

```rust
// Source: findings/mod.rs:68-72 — the existing additive optional field on DataSlice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DataSlice {
    pub range: TimeRange,
    pub gap_manifest_ref: Option<String>,
    // Phase 3 adds:
    #[serde(default)]
    pub gap_manifest: Option<GapManifest>,   // NO skip_serializing_if — see Anti-Pattern below.
}
```

**`gap_manifest` MUST serialise as `null` when absent** (mirror the `dsr` / `fdr_q` rule at `findings/mod.rs:209-211`):

```rust
// Source: findings/mod.rs:209-211, 222-224 — dsr and fdr_q MUST serialise as null, NOT absent.
/// Reserved for Phase 5 (Deflated Sharpe Ratio). Serialises as `null` in v1.
pub dsr: Option<f64>,
```

→ **DO NOT** add `#[serde(skip_serializing_if = "Option::is_none")]` to `gap_manifest`. Use bare `#[serde(default)]` only. RESEARCH §Anti-Pattern at line 497 reaffirms.

**Variant-additivity pattern** (mirror the existing tagged enum at lines 293-301):

```rust
// Source: findings/mod.rs:293-301 — the canonical 5-variant tagged Finding enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    RunStart(RunStart),
    Result(ResultFinding),
    ScanError(ScanErrorFinding),
    GapAborted(GapAbortedFinding),
    RunEnd(RunEnd),
    // Phase 3 adds:
    DryRun(DryRunFinding),                   // serialises as {"kind": "dry_run", ...}
}
```

**`DryRunFinding` payload** (mirror `RunStart` at lines 161-171, NOT `ResultFinding` — dry-run is FRAMING-like, does not carry the seven locked envelope fields):

```rust
// Source: findings/mod.rs:161-171 — RunStart shape: run_id, timestamps, request, NO schema_version/param_hash/data_slice/dsr/fdr_q.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunStart {
    pub run_id: RunId,
    pub started_at_utc: DateTime<Utc>,
    pub miner_version: String,
    pub code_revision: String,
    pub request: serde_json::Value,
}
```

→ Plan-phase MUST decide whether `DryRunFinding` carries the seven locked fields (CONTEXT D3-21 listing `{run_id, request, resolved_params, planned_data_slice, estimated_findings_count}` suggests it does NOT — closer to a framing record). The roundtrip test pattern lives at `findings/mod.rs:536-548`:

```rust
// Source: findings/mod.rs:536-548 — all-variants roundtrip via serde_json.
#[test]
fn all_variants_round_trip() {
    for finding in [/* ... every variant including DryRun ... */] {
        let json = serde_json::to_string(&finding).expect("serialise");
        let parsed: Finding = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(finding, parsed);
    }
}
```

→ Extend this test to include `DryRun` in the iteration list. Add a sibling test asserting `"kind":"dry_run"` discriminator (mirrors `envelope_fields_present` at lines 419-438).

**`RunSummary::results_emitted` invariant** (RESEARCH Pitfall 3 lines 559-567): dry-run does NOT increment `results_emitted`. Add a unit test at the bottom of `findings/mod.rs`:

```rust
// New — pin Pitfall 3.
#[test]
fn dry_run_does_not_increment_results_emitted() {
    // Build a RunSummary; assert results_emitted == 0 when only DryRun envelopes are counted.
}
```

---

### `crates/miner-cli/src/cli.rs` (extend) — `Command::Scan(ScanArgs)` + `Command::Scans`

**Analog:** self — the existing `Command::EmitFixture` variant + the `Cli::overrides()` impl at lines 54-84.

**Subcommand-extension pattern** (mirror `cli.rs:53-59`):

```rust
// Source: cli.rs:53-59 — the existing Command enum shape.
#[derive(Debug, Subcommand)]
pub enum Command {
    EmitFixture,
    // Phase 3 adds:
    Scan(ScanArgs),
    Scans,
}
```

→ `Scan(ScanArgs)` wraps a separate `ScanArgs` struct that lives in `cli/scan_args.rs` (RESEARCH §"Recommended Project Structure" line 241). The clap `Subcommand` derive auto-converts tuple-variant payload into the args struct.

**Global-flag preservation**: the four global flags (`--config`, `--cache-root`, `--bar-cache-root`, `--output`) at `cli.rs:32-47` STAY UNCHANGED. CONTEXT line 199 reaffirms.

---

### `crates/miner-cli/src/scan_args.rs` — `ScanArgs` struct + window parser

**Analog:** `crates/miner-cli/src/cli.rs::{Cli, CliOverrides flow}` — clap derive + `impl Cli { fn overrides(&self) -> CliOverrides }` conversion pattern at lines 28-84.

**Clap-derive args pattern** (mirror `cli.rs:28-51`):

```rust
// Source: cli.rs:28-51 — clap::Parser derive + #[arg] flags.
#[derive(Debug, Parser)]
#[command(name = "miner", version, about)]
pub struct Cli {
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, global = true, env = "MINER_CACHE_ROOT")]
    pub cache_root: Option<PathBuf>,
    /* ... */
    #[command(subcommand)]
    pub command: Command,
}
```

→ `ScanArgs` follows the same shape:

```rust
// New — Phase 3.
#[derive(Debug, clap::Args)]
pub struct ScanArgs {
    pub scan_id_at_version: String,                          // positional: "stats.autocorr.ljung_box@1"
    #[arg(long)]
    pub instrument: String,
    #[arg(long, default_value = "bid")]                      // CONTEXT D3-19 default.
    pub side: String,
    #[arg(long)]
    pub timeframe: String,
    #[arg(long, value_parser = parse_window)]                // window parser per CONTEXT D3-07.
    pub window: ClosedRangeUtc,
    #[arg(long, default_value = "continuous_only")]          // CONTEXT D3-19 default.
    pub gap_policy: String,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long = "params", action = clap::ArgAction::Append)] // repeatable KEY=VAL.
    pub params: Vec<String>,
}
```

**Window parser pattern** (copy RESEARCH §Pattern 5 lines 466-489 — verbatim implementation):

```rust
// Source: 03-RESEARCH.md Pattern 5 — ISO 8601 half-open parser, UTC-only.
fn parse_window(s: &str) -> Result<ClosedRangeUtc, String> {
    let (lhs, rhs) = s.split_once(':')
        .ok_or_else(|| "window must be START:END".to_string())?;
    Ok(ClosedRangeUtc {
        start: parse_iso_utc(lhs)?,
        end:   parse_iso_utc(rhs)?,
    })
}
```

**Conversion-to-typed-request pattern** (mirror `cli.rs::Cli::overrides` at lines 70-83):

```rust
// Source: cli.rs:70-83 — clap struct → typed-domain-struct conversion via #[must_use] method.
#[must_use]
pub fn overrides(&self) -> CliOverrides {
    CliOverrides {
        cache_root: self.cache_root.clone(),
        bar_cache_root: self.bar_cache_root.clone(),
        output: self.output.as_deref().map(|s| { /* ... */ }),
    }
}
```

→ Phase 3 adds `impl ScanArgs { fn to_scan_request(&self, code_revision: &str) -> Result<ScanRequest, WireError> }` — the boundary preflight that parses `--params KEY=VAL` into a `serde_json::Value`, resolves `--side`, validates the timeframe.

---

### `crates/miner-cli/src/main.rs` (extend) — `ctrlc` install + facade plumbing + exit-code routing

**Analog:** self — the existing `main` function at lines 34-74 + `make_sink` at lines 87-96 + `classify_figment_error` at lines 107-130.

**`ctrlc` install pattern** (copy RESEARCH §Pattern 4 lines 422-450 verbatim):

```rust
// Source: 03-RESEARCH.md Pattern 4 — install BEFORE Cli::parse per Pitfall 2.
let cancel = Arc::new(AtomicBool::new(false));
{
    let cancel = Arc::clone(&cancel);
    ctrlc::set_handler(move || {
        cancel.store(true, Ordering::SeqCst);
        tracing::warn!("SIGINT received; shutting down");   // NEVER eprintln!
    }).expect("ctrlc handler install");
}

let parsed = Cli::parse();
```

**Exit-code routing pattern** (copy RESEARCH §Pattern 4 lines 442-449):

```rust
// Source: 03-RESEARCH.md Pattern 4 — four-tier exit code routing per CONTEXT D3-24.
let code = match (cancel.load(Ordering::SeqCst), outcome) {
    (true,  _)                              => 130,
    (false, RunOutcome::PreflightFailed)    =>   1,
    (false, RunOutcome::HadScanErrors)      =>   2,
    (false, RunOutcome::Ok)                 =>   0,
};
std::process::exit(code);
```

**Reader construction at the binary edge** (CONTEXT line 202 "one-way dependency direction" — `miner-cli` constructs `DukascopyReader`, `miner-core` does not):

```rust
// Source: dukascopy/src/reader.rs:59-66 — infallible constructor taking impl Into<PathBuf>.
let reader = DukascopyReader::new(&cfg.cache_root);
let outcome = miner_core::engine::run_one(&req, &cfg, &reader, &mut *sink, Arc::clone(&cancel))?;
```

**`make_sink` reuse** (the existing function at `main.rs:87-96` is UNCHANGED — Phase 3's scan path constructs the sink the same way as `emit-fixture`).

---

### `crates/miner-core/src/lib.rs` (extend) — FROZEN public surface

**Analog:** self — the existing `pub use` block at lines 31-56.

**Re-export pattern** (mirror `lib.rs:31-56`):

```rust
// Source: lib.rs:31-56 — grouped pub use blocks per phase.
pub use findings::{Base64Bytes, DataSlice, /* ... */};
pub use error::{MinerError, PreflightCode, ScanErrorCode, WireError};
pub use config::{CliOverrides, MinerConfig, OutputDest, build_figment};
// Phase 2 extensions:
pub use calendar::Calendar;
pub use reader::{Blake3Hex, ClosedRangeUtc, RawBar, Reader, Side};
// ...
```

→ Phase 3 appends a new section:

```rust
// New — Phase 3 (Plan 03):
pub use scan::{Scan, ScanCtx, ScanRequest, ScanError, ScanFindingShape, Registry, bootstrap};
pub use engine::{run_one, RunOutcome};
pub use findings::DryRunFinding;          // new payload type.
// GapManifest already exposed in Phase 2 — no change needed.
```

→ The `public_surface_audit.rs` test pattern (analog at `tests/public_surface_audit.rs:1-50`) MUST be extended with a Phase-3 surface block.

---

## Test File Pattern Assignments

### `crates/miner-core/tests/scan_ljung_box.rs` — insta golden test

**Analog:** `crates/miner-core/tests/gap_manifest_snapshot.rs` (full file).

**Pattern** (mirror `gap_manifest_snapshot.rs:1-64`):

```rust
// Source: gap_manifest_snapshot.rs — construct a typed value, insta::assert_json_snapshot!.
use chrono::{NaiveDate, TimeZone, Utc};
use miner_core::{/* Scan + LjungBoxScan + ScanCtx + … */};

#[test]
fn ljung_box_matches_statsmodels_golden() {
    // 1. Build a deterministic AR(1) BarFrame (CONTEXT D3-05, 256 samples).
    // 2. Construct ScanCtx + ScanRequest.
    // 3. Run the scan through a VecSink (the existing test-only sink at sink.rs:188-216).
    // 4. Parse the JSONL, mask volatile fields (run_id, timestamps).
    // 5. insta::assert_json_snapshot!(masked_finding).
}
```

**Mask discipline** (copy `cli_streams.rs:323-344` `mask_volatile_fields`):

```rust
// Source: cli_streams.rs:323-344 — recursive volatile-field masking.
fn mask_volatile_fields(v: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for key in ["run_id", "started_at_utc", "ended_at_utc"] { /* ... */ }
        for (_, child) in map.iter_mut() { mask_volatile_fields(child); }
    }
    /* ... */
}
```

→ The `.snap` file lives under `crates/miner-core/tests/snapshots/scan_ljung_box__ljung_box_matches_statsmodels_golden.snap`. CONTEXT D3-05 fixes the byte-equality on the envelope shape; floats inside `RawArray.data` are byte-equal only because the kernel summation order is deterministic per RESEARCH §"Biased ACF" note line 687.

---

### `crates/miner-core/tests/scan_facade_determinism.rs` — twice-run masked byte-equality

**Analog:** `crates/miner-cli/tests/cli_streams.rs::emit_fixture_byte_identical_when_volatile_fields_masked` (Test 7, lines 452-478) + `crates/miner-core/tests/full_determinism.rs` (whole file for the synthetic-cache pattern).

**Pattern** (copy `cli_streams.rs:452-478`):

```rust
// Source: cli_streams.rs:452-478 — Test 7 verbatim shape.
#[test]
#[serial_test::serial]
fn twice_run_byte_identical_when_volatile_fields_masked() {
    let (out1, _, status1) = run_scan_against_synthetic_cache();
    assert_eq!(status1.code(), Some(0));
    let (out2, _, status2) = run_scan_against_synthetic_cache();
    assert_eq!(status2.code(), Some(0));

    let masked1 = mask_volatile_fields_in_jsonl(&out1);
    let masked2 = mask_volatile_fields_in_jsonl(&out2);
    assert_eq!(masked1, masked2, "OUT-03 closure for scan facade");
}
```

→ Difference from Test 7: this test runs `engine::run_one` IN-PROCESS (not via `assert_cmd`) against a `VecSink` (`sink.rs:188-216`). Cheaper and the same byte assertion.

---

### `crates/miner-core/tests/shuffled_future_regression.rs` — proptest

**Analog:** `crates/miner-core/tests/cache_smoke.rs::arrow_bytes_deterministic_under_shuffled_construction` (the proptest harness layout — see `cache_smoke.rs:38-46` for the `use proptest::prelude::*;` block).

**Pattern**:

```rust
// Source: cache_smoke.rs — proptest harness with proptest! { #[test] fn name(args in strategy) { … } }
use proptest::prelude::*;

proptest! {
    #[test]
    fn look_ahead_safe_under_post_t_shuffle(seed in 0u64..1_000) {
        // 1. Build a deterministic BarFrame from `seed` (N bars).
        // 2. Compute Ljung-Box up to cutpoint T = N/2.
        // 3. Shuffle bars at indices [T..N); recompute.
        // 4. Assert the pre-T Q-stat is byte-identical.
    }
}
```

→ CONTEXT D3-09 pins the contract. The shuffle is a deterministic permutation seeded by `seed` so failures are reproducible.

---

### `crates/miner-core/tests/gap_policy.rs` — five gap-policy behaviour tests

**Analog:** `crates/miner-core/tests/cache_smoke.rs` (whole file — five named tests, one per VALIDATION row, against a synthetic substrate).

**Pattern** (copy `cache_smoke.rs:1-50` header + per-test layout):

```rust
// Source: cache_smoke.rs:1-30 — module-doc lists every test with VALIDATION row name verbatim.
//! Five tests, matching the VALIDATION.md row IDs verbatim:
//! - `strict_with_gaps_emits_single_gap_aborted`
//! - `continuous_only_partitions_and_inlines_manifest`
//! - `strict_zero_gaps_emits_result_with_none_manifest`
//! - `continuous_only_zero_gaps_emits_empty_manifest`
//! - `never_silently_emits_on_hole_proptest`
```

→ Each test builds a `GapManifest` value directly (not via `GapDetector::detect`, which is Phase 2 territory), passes it to `engine::gap_policy::dispatch`, asserts the `Vec<TimeRange>` partitioning. The proptest `never_silently_emits_on_hole_proptest` uses the same `proptest!` macro as `cache_smoke.rs`.

---

### `crates/miner-core/tests/dry_run.rs` — `Finding::DryRun` shape

**Analog:** `crates/miner-core/tests/cache_smoke.rs::cache_hit_skips_reader` (single-scenario integration test using only the FROZEN surface).

**Pattern**:

```rust
// Source: cache_smoke.rs — single-scenario test, FROZEN-surface-only use statements.
use miner_core::{/* engine::run_one, Finding, DryRunFinding, etc. */};

#[test]
fn dry_run_emits_dry_run_finding_only() {
    // 1. Construct ScanRequest with dry_run = true.
    // 2. Run through engine::run_one with VecSink.
    // 3. Parse the captured bytes — assert: RunStart, DryRun, RunEnd; NO Result.
    // 4. Assert RunEnd.summary.results_emitted == 0 (Pitfall 3).
}
```

---

### `crates/miner-cli/tests/scan_subcommand_smoke.rs` — assert_cmd happy path

**Analog:** `crates/miner-cli/tests/cli_streams.rs` (whole file).

**Pattern** (copy `cli_streams.rs:57-87` helpers + `cli_streams.rs:94-120` Test 1 shape):

```rust
// Source: cli_streams.rs:57-87 — assert_cmd::Command::cargo_bin + env_clear + #[serial_test::serial].
fn run_scan(scan_id_at_version: &str, extra_args: &[&str]) -> (String, String, ExitStatus) {
    let mut cmd = assert_cmd::Command::cargo_bin("miner").expect("cargo_bin miner");
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("MINER_CACHE_ROOT", "/tmp/cache")
        .env("MINER_BAR_CACHE_ROOT", "/tmp/bar")
        .env("MINER_OUTPUT", "stdout")
        .arg("scan")
        .arg(scan_id_at_version)
        .args(extra_args);
    let out = cmd.output().expect("spawn miner scan");
    /* ... */
}

#[test]
#[serial_test::serial]
fn scan_emits_run_start_result_run_end() {
    let (stdout, _, status) = run_scan("stats.autocorr.ljung_box@1", &[/* … */]);
    assert_eq!(status.code(), Some(0));
    let lines = parse_stdout_lines(&stdout);
    assert_eq!(lines[0]["kind"], "run_start");
    assert_eq!(lines[1]["kind"], "result");
    assert_eq!(lines[2]["kind"], "run_end");
}
```

**Negative cases** (mirror `cli_streams.rs:200-309` — `preflight_*` tests):

```rust
// Source: cli_streams.rs:200-246 Test 5 + 256-309 Test 6 — exit 1, stdout empty, stderr JSON line with expected code.
#[test]
#[serial_test::serial]
fn unknown_scan_emits_wireerror_exit_1() {
    let (stdout, stderr, status) = run_scan("nonexistent.scan@99", &[/* … */]);
    assert_eq!(status.code(), Some(1));
    assert!(stdout.is_empty(), "T-01-03 stdout discipline");
    let wire = find_wireerror_line(&stderr);
    assert_eq!(wire["code"], "unknown_scan");
}
```

---

### `crates/miner-cli/tests/scans_catalogue.rs` — `miner scans` introspection

**Analog:** `crates/miner-cli/tests/cli_streams.rs::emit_fixture_writes_two_jsonl_lines_to_stdout` (Test 1, lines 94-120).

**Pattern**:

```rust
// Source: cli_streams.rs:94-120 — assert_cmd subprocess + parse stdout + assert per-line kind.
#[test]
#[serial_test::serial]
fn scans_emits_one_line_per_registered_scan() {
    let (stdout, _, status) = run_scans_subcommand();
    assert_eq!(status.code(), Some(0));
    let lines = parse_stdout_lines(&stdout);
    assert_eq!(lines.len(), 1);                    // Phase 3 has one scan.
    assert_eq!(lines[0]["scan_id"], "stats.autocorr.ljung_box");
    assert_eq!(lines[0]["version"], 1);
    assert!(lines[0]["finding_fields"]["effect_extra_keys"].is_array());
}
```

→ Per RESEARCH §Pitfall 7 (lines 599-610), `miner scans` lines do NOT match the `findings-v1.schema.json` (they're a different shape) — DO NOT pass them through the same validator. Open Question 8 in RESEARCH §"Open Questions" is whether to add `FindingSink::write_raw_json` or a sibling schema; the plan-phase decides.

---

### `crates/miner-cli/tests/sigint_preserves_stream.rs` — `#[cfg(unix)]` integration

**Analog:** `crates/miner-cli/tests/cli_streams.rs::run_emit_fixture_happy` (assert_cmd shape) + RESEARCH §"Code Examples" lines 693-742 (the `nix::kill` snippet).

**Pattern** (copy RESEARCH lines 693-742 verbatim):

```rust
// Source: 03-RESEARCH.md Code Examples §"assert_cmd + nix::kill SIGINT integration".
#![cfg(unix)]
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::process::{Command, Stdio};

#[test]
#[serial_test::serial]
fn sigint_preserves_already_streamed_findings_and_exits_130() {
    // 1. Spawn `miner scan ...` with a scan that sleeps after emitting Result.
    // 2. Wait for the first Result line on stdout.
    // 3. nix::kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).
    // 4. Wait for exit; assert code 130 AND that the captured stdout already contains the Result + RunEnd.
}
```

**`nix` is dev-dep only** (RESEARCH §Wave 0 line 890): `nix = { version = "0.31", default-features = false, features = ["signal"] }`. Add to `miner-cli/Cargo.toml` `[dev-dependencies]`.

**Mitigate Pitfall 8** (RESEARCH lines 612-622): the test scan MUST sleep between the first finding and `RunEnd` so the SIGINT lands in a measurable window; the plan-phase decides whether to inject a test-only `SleepScan` into `Registry::bootstrap()` under a `#[cfg(test)]` feature.

---

### `crates/miner-cli/tests/fixtures/` — synthetic test cache + Ljung-Box golden

**Analog:** `crates/miner-core/tests/full_determinism.rs` (whole file — inlines a `SyntheticCache` builder using the sibling crate's PUBLIC `day_csv_zst` API, lines 33-50).

**Pattern**:

```rust
// Source: full_determinism.rs:33-50 — inlined synthetic-cache helper using public reader API.
// ===========================================================================
// Synthetic Dukascopy cache builder (inlined from the patterns established by
// `miner-reader-dukascopy::tests::fixtures::SyntheticCache`; inlined here so
// the test does not reach into the sibling crate's #[path]-included test
// fixtures — we use the sibling crate's PUBLIC `day_csv_zst` API instead).
// ===========================================================================
```

→ Phase 3's `fixtures/` directory contains:

- `synthetic_cache.rs` — builds a tiny on-disk cache with N days of bid bars for `EURUSD` at AR(1) prices.
- `ljung_box_golden.snap` (lives under `crates/miner-core/tests/snapshots/`) — the masked JSONL the integration test asserts against.

---

## Shared Patterns (cross-cutting — apply to ALL Phase 3 files)

### `unsafe_code = "forbid"` (workspace-wide)

**Source:** `Cargo.toml:65-66`.
**Apply to:** every new source file in Phase 3.

```toml
# Source: Cargo.toml:65-66 — workspace-level forbid lint.
[workspace.lints.rust]
unsafe_code = "forbid"
```

→ Phase 3 adds ZERO `unsafe` blocks. No `extern "C"`, no `transmute`, no raw pointer dereferences. The CI lint catches violations.

### `clippy::disallowed_macros` (workspace-wide)

**Source:** `clippy.toml` (whole file).
**Apply to:** every new source file in `miner-core` AND `miner-cli`.

```toml
# Source: clippy.toml — disallowed_macros list.
disallowed-macros = [
    { path = "std::println",   reason = "stdout is reserved for findings; use FindingSink or tracing::info!" },
    { path = "std::print",     reason = "stdout is reserved for findings" },
    { path = "std::eprintln",  reason = "use tracing::warn!/error! routed to stderr by the subscriber, or error::stderr_emit::write_preflight_error" },
    { path = "std::eprint",    reason = "use tracing::warn!/error!" },
    { path = "std::dbg",       reason = "do not leave dbg! in production code" },
]
```

→ Findings flow through `FindingSink::write_envelope`; logs through `tracing::{info, debug, warn, error}!`; preflight errors through `error::stderr_emit::emit_to_stderr`. The CLI's SIGINT handler MUST use `tracing::warn!` (NOT `eprintln!`) — RESEARCH §Anti-Patterns line 495 reaffirms.

### `BTreeMap` discipline (every map in a `Serialize` path)

**Source:** `crates/miner-core/src/findings/mod.rs:101-103` (doc-line); `crates/miner-core/src/error/codes.rs:96-98` (`WireError.context`); `crates/miner-core/src/findings/mod.rs:188-195` (`RunSummary`).
**Apply to:** every new struct in Phase 3 that derives `Serialize` AND contains a map.

```rust
// Source: findings/mod.rs:101-103 — the canonical doc-line + type.
/// `BTreeMap` (NEVER `HashMap`) for deterministic ordering — OUT-03.
pub series: BTreeMap<String, RawArray>,
```

→ `Registry::scans: BTreeMap<(String, u32), Box<dyn Scan>>` (CONTEXT line 204). RESEARCH Pitfall 1 (line 539-547) reaffirms: never enable `serde_json/preserve_order`; the workspace `Cargo.toml:36` keeps `serde_json = "1"` with no features list.

### `#[cfg(test)] mod tests` at the bottom of every source file

**Source:** `crates/miner-core/src/findings/mod.rs:307-550`; `crates/miner-core/src/error/codes.rs:130-200`; `crates/miner-core/src/findings/sink.rs:218-420`; `crates/miner-core/src/aggregator.rs::tests` (bottom of file); `crates/miner-core/src/gap.rs::tests` (bottom of file).
**Apply to:** every new source file in Phase 3.

```rust
// Source: findings/mod.rs:307-310 — canonical #[cfg(test)] mod tests header.
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    // ... fixture helpers ...
    // ... numbered #[test] fns with doc-comments naming the test purpose ...
}
```

### `#[serial_test::serial]` on env-touching integration tests

**Source:** `crates/miner-cli/tests/cli_streams.rs` (every test, lines 93, 127, 150, 172, 201, 256, 372, 453).
**Apply to:** every new `miner-cli/tests/*.rs` test that calls `assert_cmd::Command::cargo_bin("miner")` with `env_clear` + custom `MINER_*` vars.

```rust
// Source: cli_streams.rs:93-94 — serial discipline for env-mutating tests.
#[test]
#[serial_test::serial]
fn name_of_test() { /* ... */ }
```

→ Phase 3's `scan_subcommand_smoke.rs`, `scans_catalogue.rs`, `sigint_preserves_stream.rs` ALL apply `#[serial_test::serial]`. Tests in `miner-core/tests/*.rs` that exercise `engine::run_one` in-process via a `VecSink` (no env mutation) MAY omit it.

### Volatile-field masking for byte-identity tests

**Source:** `crates/miner-cli/tests/cli_streams.rs:323-359` (`mask_volatile_fields` + `mask_emit_fixture_stdout`).
**Apply to:** `scan_facade_determinism.rs`, `scan_ljung_box.rs`, any test asserting byte equality across runs.

```rust
// Source: cli_streams.rs:323-344 — recursive masking; the four volatile fields.
fn mask_volatile_fields(v: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = v {
        for key in ["run_id", "started_at_utc", "ended_at_utc"] {
            if map.contains_key(key) {
                map.insert(key.to_string(), serde_json::Value::String(format!("<masked_{key}>")));
            }
        }
        if map.contains_key("wall_clock_ms") {
            map.insert("wall_clock_ms".to_string(), serde_json::Value::from(0i64));
        }
        for (_, child) in map.iter_mut() { mask_volatile_fields(child); }
    }
    /* ... */
}
```

→ Phase 3 may need additional masks: `produced_at_utc` (on `Result` / `ScanError` / `GapAborted` / `DryRun` findings) and `param_hash` if the test runs with `--params` resolution not yet wired. The four masks from `cli_streams.rs` are the minimum; the plan-phase extends as needed.

### Schema-additivity gate

**Source:** `xtask/src/main.rs` (whole file — schema regen pipeline); `crates/miner-core/tests/schema_roundtrip.rs` (round-trip discipline); CONTEXT lines 168-176 + RESEARCH §Pattern 1.
**Apply to:** the two additive changes (`DataSlice.gap_manifest`, `Finding::DryRun`).

→ Plan-phase Task: after the `findings/mod.rs` type changes, run `cargo run -p xtask -- gen-schema` and commit the regenerated `schemas/findings-v1.schema.json` IN THE SAME PR. The diff MUST contain only new additive lines (RESEARCH Pattern 1 lines 277-282). If the diff removes or reorders lines, a non-additive change snuck in.

---

## No Analog Found

| File | Role | Reason |
|------|------|--------|
| (none) | — | Every Phase 3 file has a strong existing analog from Phase 1 or Phase 2. |

The codebase already encodes every shape Phase 3 needs: tagged-enum additivity, `Send + Sync` traits with associated types, pure-kernel functions, the blake3 hashing pipeline, the `WireError::preflight` boundary, the `RunStart`/`RunEnd` framing builders, and the `assert_cmd` + `serial_test::serial` integration-test discipline. **No new patterns are being invented in Phase 3** — every new file copies an established analog.

---

## Metadata

**Analog search scope:**
- `crates/miner-core/src/{findings,error,scan,engine,cache,aggregator,gap,reader,calendar,config}/`
- `crates/miner-cli/{src,tests}/`
- `crates/miner-reader-dukascopy/src/reader.rs`
- `clippy.toml`, root `Cargo.toml`, per-crate `Cargo.toml`
- `.planning/phases/01-foundations-contracts/` and `02-reader-aggregator-derived-bar-cache/` (CONTEXT references; not bundled in this map)

**Files scanned:** 27 source files, 12 test files, 3 config files (Cargo.toml × 3, clippy.toml).
**Search method:** targeted Read calls + Bash grep for `pub fn`, `pub struct`, `impl`, `#[derive(`, `#[test]`, `#[cfg(test)] mod tests`, `tracing::`, `BTreeMap`, `Blake3Hex`, `WireError::preflight`, `assert_cmd`, `serial_test`. No re-reads of ranges already in context.
**Pattern extraction date:** 2026-05-18.

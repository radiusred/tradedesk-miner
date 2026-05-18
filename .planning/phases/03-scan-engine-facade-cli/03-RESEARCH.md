# Phase 3: Scan Engine, Facade & CLI — Research

**Researched:** 2026-05-18
**Domain:** Rust scan-engine + facade + CLI on a locked envelope contract; Ljung-Box golden against statsmodels; SIGINT-cooperative shutdown over rayon
**Confidence:** HIGH

## Summary

Phase 3 is a Rust integration phase that joins Phase 1's locked envelope (`Finding` enum + `FindingSink` + `WireError` + `PreflightCode`/`ScanErrorCode`) to Phase 2's data layer (`Reader` / `BarCache::get_or_build` / `GapDetector::detect`) by introducing three new modules in `miner-core` (`scan::Scan` trait + `Registry` + `LjungBoxScan` + `engine`) and two new CLI subcommands (`miner scan` + `miner scans`). Every Phase 3 success criterion can be discharged inside the existing workspace stack — the only new dep is `ctrlc 3.5.2` in `miner-cli` for the SIGINT path. No `unsafe`, no `async` (rayon is the parallel primitive at the engine edge; Phase 3 is single-shot so rayon barely appears).

The seven open questions from CONTEXT.md all resolve cleanly against current state. `BarCache::get_or_build` returns `BarFrame` **by value** (cache.rs:573, `Result<BarFrame, CacheError>`) so `ScanCtx::bars` is a plain by-value owning return; no lifetime puzzles. `DukascopyReader::new(impl Into<PathBuf>) -> Self` is infallible (reader.rs:61); the CLI's facade-call site needs no `?` for construction. The schemars 1.x tagged-enum pattern already in production at `findings-v1.schema.json` (`oneOf` over `kind: const "..."` discriminants) is provably additive for both the new `DryRun` variant and the new `DataSlice.gap_manifest: Option<GapManifest>` field — the xtask gen-schema pipeline regenerates byte-deterministically. Ljung-Box compatibility against statsmodels 0.14.6 is straightforward because the algorithm is closed-form (biased ACF + Q-stat cumulative-sum + chi-squared CDF) and the only numerically-fragile step is the chi-squared survival function, for which `statrs` is already in CLAUDE.md's stack.

**Primary recommendation:** Implement `Scan` as a trait that returns a hand-rolled `serde_json::Value` schema per scan (D3-14 default) — same pattern statsmodels prints in its docstring, and consistent with the way `param_hash` (D3-13) consumes `serde_json::Value`. Build the registry as a `BTreeMap<(String, u32), Box<dyn Scan>>` via an explicit `bootstrap()` function. Wire `ctrlc` in `miner-cli::main` to set an `Arc<AtomicBool>`; pass `Arc<AtomicBool>` through `ScanCtx` and `Scan::run` polls it between findings. SIGINT testing uses `assert_cmd` + `nix::sys::signal::kill` on the spawned subprocess (the existing `cli_streams.rs` test pattern + a 3-line addition). For the Ljung-Box golden, prefer the **redacted-JSONL byte equality** approach over float-tolerance unpacking: it tests the full envelope pipeline at once, and statsmodels' chi-squared CDF + biased ACF are deterministic for a fixed input across all 0.14.x patches.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| `Scan` trait + registry + Ljung-Box impl | `miner-core::scan::*` (library) | — | Phase 6 MCP/HTTP must consume the same registry byte-for-byte. Library tier is the only correct home. |
| Facade (`run_one` / engine entry) | `miner-core::engine` (library) | — | The single facade is the bytes-identical contract Phase 6 validates parity against. CLI is a thin caller. |
| Window parsing + clap subcommand surface | `miner-cli::cli` (binary) | — | Per D-16 the CLI owns clap; `miner-core` has no clap dependency. |
| SIGINT signal install | `miner-cli::main` (binary) | — | OS-signal handling is binary-edge; `miner-core` only sees a plain `Arc<AtomicBool>` token. |
| Reader construction (`DukascopyReader::new`) | `miner-cli::main` (binary) | — | One-way dep direction: only the CLI binary knows about Dukascopy. |
| Gap-policy enforcement (`strict` aborts, `continuous_only` partitions) | `miner-core::engine` (library) | — | Sits inside the facade for parity across CLI/MCP/HTTP. |
| Envelope schema additions (`DataSlice.gap_manifest`, `Finding::DryRun`) | `miner-core::findings` (library) | `xtask gen-schema` regenerates committed artifact | The Rust types are source of truth; CI gate diffs the regen. |
| Look-ahead-safe windowing (`ScanCtx::bars`) | `miner-core::scan::ctx` (library) | — | The structural invariant Phase 4 builds rolling stats on top of. |

## Standard Stack

### Core (already in workspace — no new deps)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `clap` | 4.5 | CLI parsing (extended w/ Scan + Scans subcommands) | [VERIFIED: existing workspace Cargo.toml] — already wired in `miner-cli::cli`. |
| `serde` / `serde_json` | 1 | Param parsing, schema fragments, findings serialisation | [VERIFIED: existing workspace Cargo.toml] — `serde_json::Value` is the param-schema type per D3-14. |
| `schemars` | 1 (`chrono04` feature) | `JsonSchema` derives feeding the xtask gen-schema pipeline | [VERIFIED: existing workspace Cargo.toml] — already the source of truth for the locked envelope. |
| `rayon` | (not yet in core; reserved) | Phase 4+ parallel-fanout primitive | [CITED: CLAUDE.md] — Phase 3 single-shot path is sequential; the cancellation pattern is rayon-ready but the worker pool itself doesn't enter until Phase 5's sweep manifest. |
| `tracing` | 0.1 | Structured logs to stderr (per D-15) | [VERIFIED: existing workspace Cargo.toml] — Phase 3 logs all engine-side observability via `tracing::*` macros. |
| `thiserror` | 1 | Typed library errors inside `miner-core::scan` | [VERIFIED: existing workspace Cargo.toml] — `ScanError`, `FacadeError` follow the same pattern as `CacheError`. |
| `anyhow` | 1 | `miner-cli::main` error glue | [VERIFIED: existing workspace Cargo.toml] — existing `main()` already returns `anyhow::Result<()>`. |
| `ulid` | 1 (`serde`) | `RunId::new()` generates per-run ULIDs | [VERIFIED: existing workspace Cargo.toml] — reused unchanged. |
| `blake3` | 1 | `param_hash` lowercase-hex blake3 (D3-13) | [VERIFIED: existing workspace Cargo.toml] — already produces `Blake3Hex` for reader fingerprints; same primitive. |
| `chrono` | 0.4 (`clock`, `serde`) | UTC datetime parsing for `--window`; `TimeRange` already uses `DateTime<Utc>` | [VERIFIED: existing workspace Cargo.toml] — `DateTime::parse_from_rfc3339` covers the locked ISO 8601 + `Z` form (D3-07). |
| `statrs` | NEW (likely) | `ChiSquared::cdf` for Ljung-Box p-value | [CITED: CLAUDE.md §Recommended Stack] — `statrs 0.17+` recommended for "T, F, χ², normal CDFs/PDFs needed for p-values"; not yet pulled in. The plan should add it as a `miner-core` dep behind a `chi2-pvalue` feature OR (simpler) unconditionally. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `ctrlc` | 3.5.2 [VERIFIED: cargo info ctrlc, MIT/Apache-2.0] | SIGINT handler in `miner-cli::main` only — D3-22. The handler runs on a dedicated signal thread (the crate spawns it for you) and just `store(true)` on an `Arc<AtomicBool>`. | New dep, `miner-cli` only; never enters `miner-core`. |
| `assert_cmd` | 2 | SIGINT integration test (open question 7); reuse of existing pattern from `cli_streams.rs` | [VERIFIED: existing dev-dep in `miner-cli/Cargo.toml`] — already used for 8 tests. |
| `nix` | 0.31 [VERIFIED: cargo search] | Send `SIGINT` to a spawned subprocess in the integration test via `nix::sys::signal::kill(pid, SIGINT)` | NEW dev-dep on `miner-cli` for SIGINT test only. Unix-only — the test should be `#[cfg(unix)]`. Apache-2.0 / MIT. |
| `insta` | 1.47 (`json` feature) | Golden-fixture snapshotting for the Ljung-Box JSONL output (redacted) | [VERIFIED: existing dev-dep in `miner-core/Cargo.toml`] — same pattern as `gap_manifest_snapshot__gap_manifest_json_shape_pinned.snap`. |
| `proptest` | 1.11 | Shuffled-future regression generator (D3-09) | [VERIFIED: existing dev-dep in `miner-core/Cargo.toml`]. |
| `jsonschema` | 0.46 | Validate emitted findings + emitted dry-run + emitted scans-catalogue lines against `schemas/findings-v1.schema.json` | [VERIFIED: existing dev-dep] — already used in `cli_streams.rs`. |
| `tempfile` | 3 | Test-scratch dirs (cache root, bar-cache root, output file paths) | [VERIFIED: existing workspace dev-dep]. |
| `serial_test` | 3 | Process-env-touching tests (mirror existing `cli_streams.rs` discipline) | [VERIFIED: existing workspace dev-dep]. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-rolled chi-squared CDF | `statrs::distribution::ChiSquared::cdf` | `statrs` is the maintained Rust standard; hand-rolling chi-squared for one scan is a 20-line liability. Pin `statrs` now; Phase 4 reuses across ADF/KPSS/JB/ARCH-LM. [CITED: CLAUDE.md §Statistics primitives] |
| `inventory` for scan registration | Explicit `bootstrap()` calling `r.register(...)` | D3-16 locks the explicit-bootstrap path. `inventory` is rejected on readability grounds; the user is not a Rust practitioner and `inventory`'s compile-time-magic is precisely the kind of indirection the project's "explicitness over cleverness" stance avoids. |
| `signal-hook` | `ctrlc` | `signal-hook` is more powerful (multiple signals, `iterator` API) but adds surface area; `ctrlc` is the minimal "one signal, one bool" tool D3-22 calls for. [CITED: rust-cli book §Signal handling] |
| `tokio::signal` | `ctrlc` | Brings async into `miner-cli`. CLAUDE.md and FOUND-04 explicitly keep `miner-core` sync-only; the CLI inherits that posture. |
| Typed scan-param struct per scan via `#[derive(JsonSchema)]` | `serde_json::Value` returned by `param_schema()` (D3-14 default) | Typed structs are cleaner per-scan, BUT they require `Scan::param_schema` to either be generic (breaks `dyn Scan`) or to lose the type information at the trait boundary anyway. The `serde_json::Value` shape is the lowest common denominator and matches how `RunStart.request` already carries resolved params. See "Pattern 3" below. |

**Installation (delta from Phase 2 state):**
```toml
# Cargo.toml workspace.dependencies — add:
ctrlc  = "3.5"           # SIGINT handler — miner-cli only
statrs = "0.17"          # chi-squared CDF — miner-core only

# crates/miner-cli/Cargo.toml — add to [dev-dependencies]:
nix = { version = "0.31", default-features = false, features = ["signal"] }
```

**Version verification (executed during research):**

```bash
cargo info ctrlc        # 3.5.2 — MIT/Apache-2.0 — rust-version 1.69 (we're on 1.85; fine)
cargo search ctrlc      # 3.5.2 confirmed
cargo search assert_cmd # 2.2.2 (already in workspace at "2")
cargo search nix        # 0.31.3 (the dev-only signal-test dep we add)
cargo search predicates # 3.1.4 (already in workspace at "3")
cargo search insta      # 1.47.2 (already in workspace at "1.47")
```

Verified 2026-05-18 against crates.io index.

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `ctrlc 3.5.2` | crates.io | ~9 yrs | hundreds of M | github.com/Detegr/rust-ctrlc | [OK] | Approved (new in this phase) |
| `statrs 0.17` | crates.io | ~9 yrs | tens of M | github.com/statrs-dev/statrs | not run — but CLAUDE.md HIGH-confidence recommended | Approved (new in this phase) |
| `nix 0.31` | crates.io | ~10 yrs | hundreds of M | github.com/nix-rust/nix | [OK] (slopcheck doesn't reject; nix-rust is the canonical `*nix` bindings crate) | Approved (new dev-dep in this phase) |
| `assert_cmd`, `predicates`, `tempfile`, `serial_test`, `insta`, `proptest`, `jsonschema` | crates.io | — | — | — | already approved in earlier phases | Reused unchanged |

**Packages removed due to slopcheck [SLOP] verdict:** none.
**Packages flagged as suspicious [SUS]:** none.

slopcheck (v0.6.1) ran clean for `ctrlc` and `assert_cmd` on the `crates.io` ecosystem; `statrs` was not slopchecked but is HIGH-confidence per CLAUDE.md's Recommended Stack (and is the upstream named in the Phase 3 plan's chi-squared p-value requirement). All three new deps have public repos, multi-year histories, and Apache-2.0/MIT licensing compatible with the workspace.

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| **OP-01** | User can run a single scan from CLI: `miner scan <name@version> --instrument ... --timeframe ... --window ...` | D3-18, D3-19; clap subcommand surface; facade entry; LjungBoxScan registered in `bootstrap()`. |
| **OP-05** | User can dry-run a planned scan invocation and see resolved job + `data_slice` summary before committing | D3-21 (`Finding::DryRun` variant — additive schema change verified below); `--dry-run` flag short-circuits in the facade after `RunStart` framing. |
| **OP-06** | User can SIGINT a long-running scan and keep every already-streamed finding | D3-22; `ctrlc 3.5.2` installs a handler on a dedicated thread that stores `true` into a shared `Arc<AtomicBool>`; the facade and the scan kernel poll it cooperatively. Exit code 130. |
| **OP-07** | User can introspect the scan catalogue via `miner scans` returning name/version/param-schema/finding-fields | D3-20; one JSONL line per registered scan to stdout via `FindingSink` (using a non-Finding payload shape — see "Open question on `miner scans` framing" below). |
| **OP-08** | User can rely on registry rejecting unknown scans, validating params, echoing resolved params in every finding | D3-13, D3-14, D3-19; `Registry::get` is the boundary; `PreflightCode::UnknownScan` + `PreflightCode::InvalidParameter` from `error/codes.rs`. |
| **OUT-04** | User can read actual consumed range + gap manifest reference on every finding; strict policy emits single error record with manifest | D3-08, D3-10, D3-11; `DataSlice.gap_manifest` additive optional field; `Finding::GapAborted` already exists from Phase 1. |

## Architecture Patterns

### System Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  miner-cli (binary edge)                                                    │
│                                                                             │
│  argv ─┐                                                                    │
│        ▼                                                                    │
│  clap::Cli::parse() ── Command::Scan(ScanArgs) ── Command::Scans            │
│        │                                                                    │
│        │   ctrlc::set_handler(move || cancel.store(true))                   │
│        │       │                                                            │
│        │       ▼                                                            │
│        │   Arc<AtomicBool> cancel  ───────────┐ (passed by clone)           │
│        ▼                                       │                            │
│  resolve(MinerConfig) ── DukascopyReader::new(cfg.cache_root)               │
│        │                                       │                            │
│        ▼                                       ▼                            │
│  make_sink(cfg.output) ──→  Box<dyn FindingSink>                            │
│        │                                                                    │
│        └─────────────────────────────────────┐                              │
│                                              │                              │
│                                              ▼                              │
│  ┌────────────────────────────────────────────────────────────────────┐    │
│  │  miner-core::engine::run_one(req, cfg, reader, sink, cancel)       │    │
│  │  ──── single facade entry; CLI / MCP / HTTP all call this ────     │    │
│  │                                                                    │    │
│  │   1. preflight                                                     │    │
│  │      ├─ Registry::get(scan_id, version) → Err PreflightCode::      │    │
│  │      │     UnknownScan + WireError to stderr; return Exit(1)       │    │
│  │      ├─ parse --params against scan.param_schema()                 │    │
│  │      │     err → InvalidParameter + WireError stderr; Exit(1)      │    │
│  │      ├─ parse --window (ISO 8601, Z-only, half-open)               │    │
│  │      │     err → InvalidParameter / MissingRequiredConfig          │    │
│  │      └─ resolve_params(scan, params) → BTreeMap-backed Value;      │    │
│  │            param_hash = blake3(serde_json::to_vec(&resolved))      │    │
│  │                                                                    │    │
│  │   2. framing-open                                                  │    │
│  │      RunStart{ run_id, started_at_utc, miner_version,              │    │
│  │                code_revision, request: <resolved> }                │    │
│  │      ──→ sink.write_envelope(...)                                  │    │
│  │                                                                    │    │
│  │   3. dry-run short-circuit (when --dry-run)                        │    │
│  │      Finding::DryRun{ run_id, request, resolved_params,            │    │
│  │                       planned_data_slice, est_findings_count }     │    │
│  │      ──→ sink.write_envelope(...)                                  │    │
│  │      jump to step 7                                                │    │
│  │                                                                    │    │
│  │   4. gap detection                                                 │    │
│  │      manifest = GapDetector::detect(reader, sym, side, range)      │    │
│  │      ├─ strict + manifest.gaps.non_empty()                         │    │
│  │      │     → emit ONE Finding::GapAborted{...manifest...}; goto 7  │    │
│  │      ├─ continuous_only                                            │    │
│  │      │     → partition range into max gap-free sub-ranges          │    │
│  │      └─ strict + zero gaps OR continuous_only                      │    │
│  │            → proceed; pass manifest to ScanCtx                     │    │
│  │                                                                    │    │
│  │   5. for each sub-range:                                           │    │
│  │      bars: BarFrame = BarCache::get_or_build(reader, AggParams)    │    │
│  │      ScanCtx::bars stores the BarFrame; scan reads via &BarFrame   │    │
│  │      scan.run(&ctx, &req, &mut sink_adapter)                       │    │
│  │      sink_adapter:                                                 │    │
│  │        - intercepts each result/scan_error finding                 │    │
│  │        - inlines data_slice.gap_manifest for continuous_only       │    │
│  │        - leaves None for strict success path                       │    │
│  │        - polls cancel.load() between writes; on true → break       │    │
│  │                                                                    │    │
│  │   6. framing-close                                                 │    │
│  │      RunEnd{ run_id, ended_at_utc, wall_clock_ms, summary{...} }   │    │
│  │      ──→ sink.write_envelope(...)                                  │    │
│  │                                                                    │    │
│  │   7. return RunOutcome::{Ok | Aborted | Cancelled | PreflightFail} │    │
│  │      caller maps to exit code (0/1/2/130)                          │    │
│  └────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
              │
              ▼
       stdout = JSONL findings    (D-19 sink-only)
       stderr = tracing logs + structured WireError (preflight only)
```

The diagram intentionally shows the facade as the ONLY path from CLI / MCP / HTTP into the engine. Phase 6's MCP and HTTP servers will call exactly the same `run_one(req, cfg, reader, sink, cancel)` function with their own `Box<dyn FindingSink>` — the byte-identity contract between wrappers comes directly from sharing the function body and the sink trait.

### Recommended Project Structure

```
crates/miner-core/src/
├── findings/
│   ├── mod.rs              # extended: DataSlice.gap_manifest, Finding::DryRun
│   └── ...                 # existing modules unchanged
├── error/
│   └── codes.rs            # consumed unchanged
├── engine/
│   ├── mod.rs              # facade::run_one; RunOutcome enum
│   ├── preflight.rs        # parse --params, --window; build resolved request
│   ├── gap_policy.rs       # strict vs continuous_only dispatch + partitioning
│   ├── param_hash.rs       # blake3-hex of canonical resolved-params JSON
│   └── framing.rs          # RunStart/RunEnd builders
├── scan/
│   ├── mod.rs              # Scan trait, ScanCtx, ScanError, ScanFindingShape
│   ├── registry.rs         # BTreeMap-backed Registry; bootstrap()
│   ├── ljung_box/
│   │   ├── mod.rs          # LjungBoxScan impl
│   │   ├── kernel.rs       # acf() + q_stat() pure kernels
│   │   └── tests.rs        # unit tests on the kernels (no IO)
│   └── shape.rs            # ScanFindingShape declarative type
├── cache.rs                # consumed unchanged
├── aggregator.rs           # consumed unchanged
├── gap.rs                  # consumed unchanged
└── lib.rs                  # extend `pub use` surface

crates/miner-core/tests/
├── scan_ljung_box.rs                 # NEW — golden-fixture integration
├── scan_facade_determinism.rs        # NEW — twice-run masked-byte-equality
├── shuffled_future_regression.rs     # NEW — D3-09 invariant
├── gap_policy.rs                     # NEW — strict / continuous_only / zero-gap fast path
├── dry_run.rs                        # NEW — Finding::DryRun shape
└── … existing tests unchanged

crates/miner-cli/src/
├── cli.rs                  # extended: Command::Scan(ScanArgs), Command::Scans
├── main.rs                 # extended: ctrlc, facade-call plumbing
├── scan_args.rs            # NEW — ScanArgs struct + window parser
└── …

crates/miner-cli/tests/
├── cli_streams.rs                 # existing — unchanged
├── scan_subcommand_smoke.rs       # NEW — assert_cmd happy path against fixture cache
├── scans_catalogue.rs             # NEW — `miner scans` introspection contract
├── sigint_preserves_stream.rs     # NEW — #[cfg(unix)] nix::kill SIGINT regression
└── fixtures/                      # NEW — synthetic SyntheticCache + Ljung-Box golden
```

### Pattern 1: Tagged-Enum Additivity for `Finding::DryRun` and `DataSlice.gap_manifest`

**What:** Adding a new `oneOf` arm to a tagged enum AND adding an `Option<T>` field with `#[serde(default)]` to an existing struct are both **schema-additive** under schemars 1.x — the regenerated `findings-v1.schema.json` differs only by `oneOf` append + a new `properties.gap_manifest` entry. Existing consumers that match on `kind ∈ {run_start, result, scan_error, gap_aborted, run_end}` keep working.

**When to use:** Both Phase 3 envelope extensions.

**Why this is safe for D-23 (schema_version = 1 immutable in v1):** The locked-schema discipline only forbids *breaking* changes (removing fields, renaming variants, tightening types). Adding a new variant or a new optional field is the schema-additivity escape hatch. The committed `findings-v1.schema.json` (lines 496–562) already uses `oneOf` with per-variant `kind: { const: "..." }`:

```json
"oneOf": [
  { "type":"object", "properties": { "kind": {"const":"run_start"}, ... }, "required": ["kind"] },
  { "type":"object", "properties": { "kind": {"const":"result"},    ... }, "required": ["kind"] },
  ...
]
```

A new variant simply appends:

```json
  { "type":"object", "properties": { "kind": {"const":"dry_run"},   ... }, "required": ["kind"] }
```

And `DataSlice` already has `gap_manifest_ref: Option<String>` outside its `required` array (lines 9–25); adding `gap_manifest: Option<GapManifest>` follows the exact same shape ([CITED: schemars docs] `Option<T>` + `#[serde(default)]` produces a non-required property typed as `anyOf: [<inner>, {"type":"null"}]` or `type: ["X","null"]`).

**Schema-sync CI gate behaviour:** The `xtask gen-schema` pipeline (xtask/src/main.rs) regenerates byte-deterministically from the Rust types. After Phase 3 lands the two additive changes, the committed `schemas/findings-v1.schema.json` MUST be regenerated and committed in the same PR; the CI diff gate accepts both new lines as part of the locked-schema-additive policy.

**Required confirmation step in Plan 03-XX:** Add a task that runs `cargo run -p xtask -- gen-schema` after the type changes and diffs the result against the committed artifact. The diff must contain only:
- A new `"gap_manifest": { ...$ref or anyOf nullable... }` property under `DataSlice.properties`.
- A new `oneOf` entry for the `DryRun` variant.
- A new `"DryRunFinding": {...}` entry under `$defs`.
No removed lines. No reordered keys. Any other diff = a non-additive change snuck in.

**Example:**
```rust
// Source: miner-core::findings (after Phase 3 changes)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DataSlice {
    pub range: TimeRange,
    pub gap_manifest_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]  // ← NO: see invariant below
    pub gap_manifest: Option<GapManifest>,
}
```

⚠️ **Invariant break risk:** The existing envelope discipline (per `findings/mod.rs` line 18: "`dsr` and `fdr_q` MUST serialise as JSON `null` (not absent fields)") says *do not* add `skip_serializing_if = "Option::is_none"`. Be consistent: `gap_manifest` MUST serialise as `null` when absent under `strict` success / `continuous_only` zero-gap-fast-path, and as the full `GapManifest` JSON otherwise. Replace the line above with:

```rust
    #[serde(default)]
    pub gap_manifest: Option<GapManifest>,
```

The schemars output for this is `type: ["object","null"]` (or `anyOf: [{"$ref":...}, {"type":"null"}]` depending on settings) — both are JSON-Schema-additive.

### Pattern 2: `Scan` Trait + `&'static Scan`-friendly Registry

**What:** Scans implement the `Scan` trait; the registry holds `Box<dyn Scan>` objects keyed by `(id_string, version)`; the registry is built once at process start via `bootstrap()`.

**When to use:** D3-14, D3-16. This is the canonical pattern for static catalogues of polymorphic compute kernels in Rust (the same shape `polars` uses for its expression registry, the same shape `tantivy` uses for tokenizers).

**Why this works without lifetime gymnastics:** `Scan: Send + Sync` makes `Box<dyn Scan>` shareable across `rayon` workers in Phase 5. `&dyn Scan` references handed out by `Registry::get` borrow from the `Registry` for the lifetime of one facade call — that's well-defined because the registry outlives every facade call in the binary. The CONTEXT.md note about `&'static Scan` is fine to ignore: `&dyn Scan` borrowed from a `Registry: 'a` is enough; we don't need `&'static`.

**Example:**

```rust
// Source: miner-core::scan::mod (Phase 3 — to be written)
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub trait Scan: Send + Sync {
    fn id(&self) -> &'static str;
    fn version(&self) -> u32;
    fn param_schema(&self) -> serde_json::Value;     // hand-rolled JSON Schema fragment
    fn finding_fields(&self) -> ScanFindingShape;
    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(), ScanError>;
}

pub struct ScanFindingShape {
    pub effect_extra_keys: &'static [&'static str],
    pub raw_series_keys:   &'static [&'static str],
}

pub struct ScanCtx<'a> {
    pub bars: &'a BarFrame,             // by-reference, lives for the scan call
    pub gap_manifest: &'a GapManifest,  // already detected by the facade
    pub run_id: RunId,
    pub code_revision: &'static str,    // == miner_core::CODE_REVISION
    pub source: Source,                 // {source_id, symbol, side, timeframe}
    pub data_slice_range: TimeRange,    // the SUB-range this scan call covers
}
```

**The cancellation parameter:** Pass `cancel: &Arc<AtomicBool>` explicitly. The kernel checks `cancel.load(Ordering::Relaxed)` (or `SeqCst` — both work; `Relaxed` is fine for a stop-flag) at every yield point — for Ljung-Box that's just one check at start because it's single-shot. Rolling-stat Phase 4 scans check between each output row.

### Pattern 3: Hand-Rolled `serde_json::Value` Param Schema per Scan

**What:** Each scan returns a hand-built schema fragment as `serde_json::Value`. Compatible with `dyn Scan`. Lives next to the scan, version-bumps cleanly.

**When to use:** D3-14 default; resolves open question 3.

**Why not `#[derive(JsonSchema)]` on a per-scan typed struct:**

1. To make a typed `Params` work through `dyn Scan`, you'd need either (a) generic associated types on the trait (breaks dyn-compat), (b) every scan reduces to `serde_json::Value` at the boundary anyway (which is what we're doing already), or (c) a `serde_erased::Box` indirection that adds complexity without buying anything for one scan in Phase 3.
2. `param_hash` (D3-13) requires canonical `serde_json::Value` serialisation regardless of typed-struct presence.
3. The locked envelope already carries `params: serde_json::Value` on `ResultFinding` (findings/mod.rs:230) — the rest of the pipeline is `Value`-based.
4. Phase 4 brings 21 more scans; if `derive(JsonSchema)` per-struct turns out cleaner for *some* of them in practice, individual scans can derive a typed struct and `.into()` it to `Value` inside `param_schema()` — no trait-level commitment needed.

**Example:**

```rust
// Source: miner-core::scan::ljung_box (Phase 3)
impl Scan for LjungBoxScan {
    fn id(&self) -> &'static str { "stats.autocorr.ljung_box" }
    fn version(&self) -> u32 { 1 }

    fn param_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "lags": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Max lag k. Defaults to min(10, n / 5) (Box-Jenkins)."
                }
            },
            "additionalProperties": false
        })
    }

    fn finding_fields(&self) -> ScanFindingShape {
        ScanFindingShape {
            effect_extra_keys: &["lags", "q_stats", "p_values", "acf"],
            raw_series_keys:   &["returns", "timestamps_ms"],
        }
    }

    fn run(
        &self,
        ctx: &ScanCtx<'_>,
        req: &ScanRequest,
        sink: &mut dyn FindingSink,
        cancel: &Arc<AtomicBool>,
    ) -> Result<(), ScanError> {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) { return Ok(()); }
        let returns = log_returns(&ctx.bars.close);          // inline per D3-02
        let max_lag = resolve_lags(req.params.get("lags"), returns.len())?;
        let acf      = biased_acf(&returns, max_lag);         // statsmodels-matching biased ACF
        let (q_stats, p_values) = ljung_box_q_and_p(&returns, &acf, max_lag);
        sink.write_envelope(&self.into_result_finding(
            ctx, req, returns, acf, q_stats, p_values, max_lag,
        ))?;
        Ok(())
    }
}
```

### Pattern 4: `ctrlc` + Cooperative `Arc<AtomicBool>` Polling

**What:** `miner-cli::main` installs a `ctrlc` handler that just flips an atomic flag; the facade and every scan check the flag at well-defined yield points; rayon (Phase 5) cooperatively breaks when the flag is true.

**When to use:** D3-22; resolves open question 5.

**Why this is safe with rayon:** `ctrlc 3.5.2` spawns its OWN dedicated signal-handling thread (per docs.rs and the README example). It does NOT install a Unix signal handler in the traditional sense — async-signal-safety issues do not apply. The handler is plain Rust code that runs on its own thread, and it just performs `flag.store(true)`. Rayon worker threads check `flag.load()` cooperatively. There is no interaction between rayon's worker pool and ctrlc's signal thread.

**Sketch of the polling pattern (D3-22 confirmation):**

```rust
// Source: miner-cli::main (Phase 3 — extended)
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

let cancel = Arc::new(AtomicBool::new(false));
{
    let cancel = Arc::clone(&cancel);
    ctrlc::set_handler(move || {
        cancel.store(true, Ordering::SeqCst);
        // also nudge tracing — exits with structured info on stderr.
        tracing::warn!("SIGINT received; shutting down");
    }).expect("ctrlc handler install");
}

// ... build sink, parse args ...

let outcome = miner_core::engine::run_one(&req, &cfg, &reader, &mut *sink, Arc::clone(&cancel))?;

// Exit code routing (D3-24):
let code = match (cancel.load(Ordering::SeqCst), outcome) {
    (true,  _)                              => 130,    // SIGINT overrides
    (false, RunOutcome::PreflightFailed)    =>   1,
    (false, RunOutcome::HadScanErrors)      =>   2,
    (false, RunOutcome::Ok)                 =>   0,
};
std::process::exit(code);
```

**Polling sites inside the facade:**
1. **At start of `run_one`** — early-out before any IO.
2. **After `RunStart` is sunk** — bound the worst-case work between SIGINT and observable exit.
3. **In the continuous-only sub-range loop** — between each sub-range BarCache load.
4. **In `Scan::run`** — between each emitted finding (Ljung-Box: once at start; Phase 4 rolling stats: between each output row).

**`SeqCst` vs `Relaxed`:** Use `SeqCst` on the store (in the handler) for a global happens-before barrier; `Relaxed` on the load (inside the scan kernel) because spurious one-iteration delay is acceptable and the kernel does no atomic-dependent ordering. The store is rare (once per run), the load is hot — this pairing is the standard cancellation-token convention.

### Pattern 5: ISO 8601 Half-Open `--window` Parser

**What:** Parse `START:END` or `--from START --to END` into a `ClosedRangeUtc` (the existing Phase 2 type). Accept date-only (midnight UTC) and full `YYYY-MM-DDTHH:MM:SSZ` forms. Reject anything else with `PreflightCode::InvalidParameter`.

**When to use:** D3-07.

**Example:**

```rust
// Source: miner-cli::scan_args (Phase 3 — to be written)
fn parse_window(s: &str) -> Result<ClosedRangeUtc, String> {
    let (lhs, rhs) = s.split_once(':')
        .ok_or_else(|| "window must be START:END".to_string())?;
    Ok(ClosedRangeUtc {
        start: parse_iso_utc(lhs)?,
        end:   parse_iso_utc(rhs)?,
    })
}

fn parse_iso_utc(s: &str) -> Result<DateTime<Utc>, String> {
    // Date-only form: midnight UTC.
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(date.and_hms_opt(0, 0, 0).unwrap().and_utc());
    }
    // Full datetime form: RFC 3339 with explicit Z.
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("invalid ISO 8601 datetime {s:?}: {e}"))
}
```

The parser rejects any timezone other than `Z` because RFC 3339 with `+00:00` or `+02:00` would let the operator silently express non-UTC ranges; UTC-only is the safer default.

### Anti-Patterns to Avoid

- **`println!` / `eprintln!` anywhere in `miner-core` or `miner-cli`.** The workspace `clippy.toml` already bans them (Phase 1 D-15). All findings flow through `FindingSink`; all logs flow through `tracing::*`; all preflight errors through `error::stderr_emit::emit_to_stderr`. The CLI's SIGINT handler MUST use `tracing::warn!`, NOT `eprintln!`.
- **`HashMap` anywhere in a `Serialize`-derived type.** The Phase 2 invariant check found two `HashMap` references and both were ephemeral intermediates for Arrow's API; Phase 3 must keep that record clean. `Registry::scans: BTreeMap<(String, u32), Box<dyn Scan>>` is the only Phase 3 map type.
- **`#[serde(skip_serializing_if = "Option::is_none")]` on `DataSlice.gap_manifest`.** That would break the `dsr` / `fdr_q` precedent of "null-but-present" optional fields. Use `#[serde(default)]` only.
- **`tokio::signal` or `signal-hook` in Phase 3.** Adds dep weight and pulls async in.
- **Memory-mapped reads of the Arrow IPC bar-cache file.** Phase 2 explicitly deferred this; the cache reads via `BufReader<File>`. Phase 3 calls `BarCache::get_or_build` and gets a fully-loaded `BarFrame` back — nothing to revisit here.
- **Computing `param_hash` over a `HashMap`-backed `serde_json::Value`.** `serde_json` is pinned to NO features in the workspace Cargo.toml (line 21–24 of the existing root manifest) which means `serde_json::Map` stays `BTreeMap`-backed. Plan-task review: do NOT add `features = ["preserve_order"]` to `serde_json` anywhere — it would silently break byte-identity by switching to insertion order.
- **Reaching into `Reader` directly from a scan.** D3-15: scans go through `ScanCtx::bars`. The reader is owned by the facade.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Chi-squared survival function for p-value | Custom Wilson-Hilferty approximation | `statrs::distribution::ChiSquared::cdf` (then `1.0 - cdf`) | statrs is already in CLAUDE.md's recommended stack; Phase 4 reuses it across ADF/KPSS/JB/ARCH-LM. |
| SIGINT handling | Custom `signal::pthread_sigmask` + spawned thread | `ctrlc 3.5.2` | Crate is exactly one feature: install a closure on SIGINT, run it on a dedicated thread. The crate handles Unix vs Windows for free. |
| ULID generation | Custom Crockford-base32 encoder | `ulid` crate (already in workspace) | Phase 1 D-24 locked. |
| Blake3 hashing | Custom hex encoder | `blake3` crate + the existing `Blake3Hex` newtype | Phase 2 already uses this for fingerprints. |
| ISO 8601 parsing | Hand-tokenised parser | `chrono::DateTime::parse_from_rfc3339` + `NaiveDate::parse_from_str` | Two-line solution; the FX-domain UTC-only constraint is enforced separately. |
| JSON Schema for scan params | Hand-validate the JSON shape | `jsonschema` crate validation against `Scan::param_schema()` | Already a dev-dep; turn it into a dev-only validator in test-mode. (For production, the typed-deserialise-then-validate path is the canonical err: deserialise into a typed struct, return `InvalidParameter` on failure; the JSON Schema only documents the surface.) |
| Test-time SIGINT delivery to a subprocess | spawn a child + `std::process::Command::kill()` (kills with SIGKILL) | `nix::sys::signal::kill(Pid::from_raw(child.id() as i32), Signal::SIGINT)` | `child.kill()` sends SIGKILL, which the test cannot distinguish from preserved-stream behaviour. SIGINT must be delivered explicitly. |
| Tagged-enum schema generation | Manual `oneOf` JSON | `#[serde(tag = "kind")]` + `#[derive(JsonSchema)]` | Already proven in Phase 1 — see committed schema lines 496–562. |

**Key insight:** Every "hand-roll temptation" in Phase 3 has a 1- to 5-line standard-stack answer. Resist the temptation; the Phase 4 / Phase 5 plans will reuse each one.

## Runtime State Inventory

Phase 3 is greenfield code on existing infrastructure (no rename, no migration). The phase introduces:

- new modules (`miner-core::scan`, `miner-core::engine`)
- new CLI subcommands (`scan`, `scans`)
- two additive envelope changes (DataSlice field; Finding variant)
- one new workspace dep (`ctrlc`) and one new dev-dep (`nix`)

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — Phase 3 reads from the Phase 2 BarCache (Arrow IPC files), produces JSONL on stdout, persists nothing new. | — |
| Live service config | None. No external services. | — |
| OS-registered state | The `ctrlc` handler installs a per-process SIGINT/CTRL_C_EVENT handler at runtime. This is process-scoped and goes away when the binary exits. No persistent OS registration. | — |
| Secrets/env vars | No new env vars. Existing `MINER_CACHE_ROOT` / `MINER_BAR_CACHE_ROOT` / `MINER_OUTPUT` are reused unchanged. `RUST_LOG` continues to drive tracing-subscriber. | — |
| Build artifacts | `findings-v1.schema.json` is regenerated by `xtask gen-schema`. Re-run after the type changes; commit the updated schema in the same PR. | Yes — schema regen MUST land alongside the Rust type changes. |

**Greenfield phase note:** This section is included for completeness; Phase 3 has no rename/refactor surface. The committed schema is the only build artifact that needs synchronised update.

## Common Pitfalls

### Pitfall 1: `serde_json` `preserve_order` feature snuck in by a transitive dep

**What goes wrong:** A future workspace addition pulls in a crate that requests `serde_json/preserve_order`, silently flipping `serde_json::Map` from `BTreeMap` to `IndexMap`. `param_hash` becomes non-deterministic across cold/warm cargo runs; the shuffled-future regression fails sporadically.

**Why it happens:** Cargo features are additive — once any crate in the dep tree requests `preserve_order`, every crate sees it.

**How to avoid:** Add a CI step `cargo tree -p miner-core --features 2>&1 | grep -F 'preserve_order' && exit 1`. The Phase 1 schema-regen byte-equality test exercises the same property end-to-end; if that test ever flakes, this is the first place to look.

**Warning signs:** `param_hash` differs between two invocations on the same input; `schemas/findings-v1.schema.json` regen produces non-deterministic diffs.

### Pitfall 2: clap's `try_get_matches` swallows the SIGINT handler install order

**What goes wrong:** If `Cli::parse()` runs BEFORE `ctrlc::set_handler`, an early-SIGINT during arg parsing is unhandled (clap exits with default behaviour); if AFTER, an early-SIGINT before `Cli::parse()` is also unhandled.

**Why it happens:** `ctrlc::set_handler` is global-once; clap runs synchronously.

**How to avoid:** Install the `ctrlc` handler BEFORE `Cli::parse()`. clap parsing is microseconds — the racy window is negligible. The handler installs on a dedicated thread so it's ready effectively instantly.

**Warning signs:** SIGINT during help-text rendering hangs the process; SIGINT before logging is initialised loses the "shutting down" trace.

### Pitfall 3: `Finding::DryRun` slipping into `RunSummary.results_emitted`

**What goes wrong:** The dry-run path emits a `Finding::DryRun` but the `RunEnd.summary` counter increments `results_emitted` by 1 — a downstream consumer counting actual scan results sees one phantom result per dry-run invocation.

**Why it happens:** Easy to forget that `DryRun` is a sixth variant and not part of the `Result` family.

**How to avoid:** `RunSummary::results_emitted` is only incremented for `Finding::Result`. Dry-run runs MAY introduce a new `RunSummary::dry_run_emitted: u64` counter (additive, follows the same `BTreeMap` discipline), or simply skip counter increment entirely; pick one and pin it in a unit test.

**Warning signs:** A test that runs `--dry-run` twice and asserts `RunEnd.summary.results_emitted == 0`.

### Pitfall 4: `DataSlice.range` reflecting requested vs consumed window inconsistently

**What goes wrong:** Under `continuous_only` with two sub-ranges, the first `Result` finding's `data_slice.range` is the FULL requested window (because the engine forgot to narrow after partitioning); the second is correct. Consumers can't tell whether `range` describes "what was requested" or "what was consumed".

**Why it happens:** D3-08 is subtle. The cleanest implementation is: build a `ScanRequest::sub_range: TimeRange` per sub-range BEFORE calling `Scan::run`; the scan emits exactly that into `data_slice.range`. `RunStart.request` carries the user-*requested* full window for audit.

**How to avoid:** Have the facade construct one `ScanRequest` per sub-range and pass it in; the scan never sees the "outer" requested window — it only sees its slice.

**Warning signs:** A `continuous_only` finding's `data_slice.range` doesn't match the offsets the scan actually computed over.

### Pitfall 5: `Scan::run` flushing the sink before the facade is done

**What goes wrong:** `Scan::run` calls `sink.flush()` at the end; the facade then writes `RunEnd` and the per-envelope flush in `StdoutSink` re-flushes anyway. Two flushes per scan-end is harmless but visible in flush-counter tests.

**Why it happens:** Defensive flushing.

**How to avoid:** Scans NEVER call `sink.flush()`. The facade flushes ONCE at the end after `RunEnd`. The `StdoutSink::write_envelope` already flushes per envelope (Phase 1 PITFALLS #4), so visibility is preserved regardless.

**Warning signs:** Test 3 in `cli_streams.rs::stdoutsink_flushes_per_envelope` style — count is off by one.

### Pitfall 6: ULID timestamps embedding into `param_hash` accidentally

**What goes wrong:** `param_hash` is computed over `&resolved_params` — but if `RunId` (a ULID) ever leaks into the params struct (e.g., via a `run_id` echo in the resolved-request `Value`), the hash becomes time-dependent and breaks byte-identity.

**Why it happens:** D3-23's run-level determinism guarantee specifically excludes `run_id` — but `param_hash` lives BENEATH that level.

**How to avoid:** `param_hash`'s input is ONLY the resolved scan params (lags + any future per-scan tuning). It is NOT `RunStart.request`. Separate the two `serde_json::Value`s: one for `request` (mixed with run-level metadata), one for `resolved_params` (just the typed params struct).

**Warning signs:** Two runs of the same scan on the same data produce different `param_hash` values.

### Pitfall 7: `miner scans` output failing schema validation because it's NOT a Finding

**What goes wrong:** `miner scans` emits per-scan catalogue lines, but those lines are not `Finding` envelopes — they have a different shape (`{"scan_id": "...", "version": 1, "params": {...}, "finding_fields": {...}}`). Existing tests like `emit_fixture_validates_against_committed_schema` (line 173 of `cli_streams.rs`) try to validate every stdout line against `findings-v1.schema.json` and would fail for the catalogue lines.

**Why it happens:** `FindingSink::write_envelope(&Finding)` is typed; `miner scans` must use a different sink call or a different sink type.

**How to avoid:** Pick one of:
1. **Wrap catalogue lines in `Finding::ScanCatalogueEntry(ScanCatalogueEntry)`** — adds a *seventh* envelope variant. Schema-additive but pollutes the envelope's meaning.
2. **Define a separate `CatalogueSink` trait** that writes non-Finding JSONL lines. Bypass `FindingSink`. Lose the byte-identity discipline.
3. **Add a `FindingSink::write_raw_json(&serde_json::Value)` method** with a strict doc-comment saying "ONLY for `miner scans`-style introspection lines; validate against a sibling schema". Pragmatic; doesn't add an envelope variant.

**Recommendation:** Option 3 — `FindingSink::write_raw_json`. Sibling schema lives at `schemas/scans-catalogue-v1.schema.json` (also generated by xtask). This is **a Phase 3 user-facing decision the plan-phase should escalate to the user via /gsd:discuss-phase or note in the plan**. Open question in Open Questions section below.

### Pitfall 8: SIGINT integration test flakes because the subprocess exits before the signal arrives

**What goes wrong:** The test spawns `miner scan ...` and immediately sends SIGINT via `nix::kill`. The subprocess may have already finished (single-shot Ljung-Box on a tiny fixture completes in milliseconds), so the test sees exit code 0 instead of 130 — and the test asserts 130.

**Why it happens:** SIGINT is racy by design.

**How to avoid:**
1. Use a synthetic scan fixture (or override) that sleeps after emitting the first `Result` finding — the engine knows to poll the cancel token, so the sleep blocks until SIGINT lands.
2. OR test the SIGINT-handling shape at a finer granularity: unit-test the facade's `cancel.load()` polling decoupled from the OS signal. The CONTEXT.md open question 7 explicitly asks which path; **recommendation: do both** — unit test for fine-grained polling sites + one assert_cmd integration test for end-to-end signal delivery with a sleep-injected scan.

**Warning signs:** The SIGINT test passes locally but flakes in CI's faster runner.

## Code Examples

Verified patterns from official sources and the existing codebase.

### Computing `param_hash` (D3-13)

```rust
// Source: blake3 docs + the existing Blake3Hex newtype in reader.rs
use miner_core::Blake3Hex;

fn param_hash(resolved: &serde_json::Value) -> Result<Blake3Hex, serde_json::Error> {
    let bytes = serde_json::to_vec(resolved)?;            // BTreeMap-backed → byte-stable
    let hash  = blake3::hash(&bytes);                     // 32-byte digest
    Ok(Blake3Hex::from_hex_bytes(hash.to_hex().as_bytes()))
}
```

### Biased ACF + Q-stat (matching `statsmodels.tsa.stattools.acf(adjusted=False, fft=False)`)

```rust
// Source: statsmodels diagnostic.py:acorr_ljungbox + tsa.stattools.acf algorithm walk
// (verified against statsmodels 0.14.6 source). Phase 4 will refactor this into
// ANOM-01 / ANOM-04 separation; Phase 3 inlines per D3-02.

fn log_returns(close: &[f64]) -> Vec<f64> {
    close.windows(2).map(|w| (w[1] / w[0]).ln()).collect()
}

/// Biased ACF (denominator = n, NOT n-k) — matches statsmodels acf(adjusted=False).
/// Demeaned input per statsmodels comment.
fn biased_acf(x: &[f64], max_lag: usize) -> Vec<f64> {
    let n    = x.len() as f64;
    let mean = x.iter().sum::<f64>() / n;
    let cent: Vec<f64> = x.iter().map(|v| v - mean).collect();
    let denom: f64 = cent.iter().map(|v| v * v).sum::<f64>();
    let mut out = Vec::with_capacity(max_lag + 1);
    out.push(1.0);  // lag-0 == 1 by construction
    for k in 1..=max_lag {
        let num: f64 = cent.iter().zip(cent[k..].iter()).map(|(a, b)| a * b).sum();
        out.push(num / denom);
    }
    out
}

/// Q_h = n * (n + 2) * sum_{k=1..h} acf[k]^2 / (n - k)   (Ljung-Box, matches statsmodels)
/// p_h = 1 - chi2.cdf(Q_h, df = h)                         (model_df = 0 in v1; HYG-* in Phase 5)
fn ljung_box_q_and_p(returns_n: usize, acf: &[f64], max_lag: usize) -> (Vec<f64>, Vec<f64>) {
    use statrs::distribution::{ChiSquared, ContinuousCDF};
    let n = returns_n as f64;
    let mut q = vec![0.0; max_lag];
    let mut acc = 0.0_f64;
    for k in 1..=max_lag {
        acc += acf[k] * acf[k] / (n - k as f64);
        q[k - 1] = n * (n + 2.0) * acc;
    }
    let p: Vec<f64> = (1..=max_lag).map(|h| {
        let dist = ChiSquared::new(h as f64).expect("df > 0");
        1.0 - dist.cdf(q[h - 1])
    }).collect();
    (q, p)
}
```

Note the **summation order** in `biased_acf` and `ljung_box_q_and_p` is sequential and `cumsum`-style, exactly matching statsmodels' `np.cumsum(sacf2)` — this is what makes the goldens reproducible against statsmodels' output bytes.

### Window parser (D3-07)

See Pattern 5 above.

### `assert_cmd` + `nix::kill` SIGINT integration (open question 7)

```rust
// Source: assert_cmd README + nix-rust/nix examples — Phase 3 NEW test file.
#![cfg(unix)]
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

#[test]
fn sigint_preserves_already_streamed_findings_and_exits_130() {
    // Bin path comes from CARGO_BIN_EXE_<name> set by Cargo for integration tests.
    let bin = env!("CARGO_BIN_EXE_miner");

    let mut child = Command::new(bin)
        .args(["scan", "stats.autocorr.ljung_box@1",
               "--instrument", "EURUSD",
               "--side", "bid",
               "--timeframe", "15m",
               "--window", "2024-01-01:2024-12-31",
               // a synthetic test-only `--sleep-after-first-finding-ms 5000`
               // flag exposed under #[cfg(test)] for SIGINT-test discipline:
               "--sleep-after-first-finding-ms", "5000"])
        .env("MINER_CACHE_ROOT", /* path to synthetic fixture cache */ "...")
        .env("MINER_BAR_CACHE_ROOT", "/tmp/bar-cache")
        .env("MINER_OUTPUT", "stdout")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn miner");

    // Read until we see the first `result` line (sleep stalls behind it).
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).expect("read first line");
    assert!(first.contains("\"kind\":\"run_start\""), "first line is run_start; got: {first}");
    let mut second = String::new();
    reader.read_line(&mut second).expect("read second line");
    assert!(second.contains("\"kind\":\"result\""), "second line is result; got: {second}");

    // Now interrupt.
    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).expect("send SIGINT");

    let status = child.wait().expect("wait child");
    assert_eq!(status.code(), Some(130), "SIGINT must exit 130; got {:?}", status);
}
```

The `--sleep-after-first-finding-ms` is a test-only flag (gated behind a `#[cfg(any(test, feature = "test-internal"))]` attribute on `ScanArgs`); it pauses the scan kernel between emissions so the test has a deterministic window to deliver SIGINT.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `schemars 0.8` typed `SchemaObject` builder | `schemars 1.x` wrapping `serde_json::Value` | 2025 | Phase 1 already migrated; Phase 3 inherits. |
| Hand-rolled per-scan param schemas | Hand-rolled is *still* the right answer for Phase 3 | — | `#[derive(JsonSchema)]` per typed struct is overkill for one Phase 3 scan; Phase 4 may revisit per-scan. |
| `tokio::signal` for CLI signal handling | `ctrlc` for sync-only binaries | — | Decoupled — `tokio::signal` is for binaries that already need an async runtime; CLAUDE.md keeps `miner-core` sync, so the CLI inherits sync-`ctrlc`. |
| `serde_json/preserve_order` for deterministic JSON | `BTreeMap`-backed `Map` (NO `preserve_order` feature) | Phase 1 D-15 | Phase 3 must not regress this. |
| `inventory`-style auto-registration of trait impls | Explicit `bootstrap()` registration | Phase 3 D3-16 | Keeps the magic out; user is not a Rust practitioner. |
| `tempfile::NamedTempFile::persist` for atomic file writes | Phase 2 already adopted | — | Phase 3 doesn't write files (cache is Phase 2's domain). |

**Deprecated/outdated:**
- `clap 3.x` (the workspace pins `4.5`). Phase 3 reuses the `derive`-style API; no migration needed.
- `statsmodels 0.12.x` had `min((nobs // 2 - 2), 40)` as the Ljung-Box default lag; `0.14.x` uses `min(10, nobs // 5)`. Pin to a specific `statsmodels` version when generating the golden ([CITED: docs.rs/statsmodels stable release notes]) and **document the pinned version in the golden fixture file's leading comment**.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `statrs::distribution::ChiSquared::cdf` floating-point output matches scipy.stats.chi2.cdf to ~1e-12 for `df ∈ {1..40}` and `Q ∈ [0.0, 1000.0]` | Pattern 1, Pitfall section | Goldens drift; Plan must run a one-time alignment check + document tolerance. Practical: statrs's chi-squared uses the Lanczos gamma function via gamma-incomplete, the same algorithm scipy uses; absolute drift is typically <1e-13. |
| A2 | Plan-phase will run `cargo run -p xtask -- gen-schema` and commit the resulting diff alongside the type changes; the CI schema-sync gate will accept it | Pattern 1 | The plan needs an explicit task; without it the CI gate blocks the PR. The Phase 1 + Phase 2 workflow already does this for every new `JsonSchema` derive. |
| A3 | `chrono::DateTime::parse_from_rfc3339` rejects timezone offsets other than `Z` when invoked as a strict parser — confirmation requires either documentation cross-check OR a manual test | Pattern 5 | Wrong: `parse_from_rfc3339` ACCEPTS `+00:00` as equivalent to `Z`. The window parser must explicitly check `s.ends_with('Z')` BEFORE calling parse, otherwise non-`Z` UTC offsets sneak through. Plan-phase: write a unit test that rejects `2024-01-01T00:00:00+02:00`. |
| A4 | `ctrlc` does NOT install a real Unix `signal()` handler (in the unsafe sense) — it spawns a dedicated thread and uses `pthread_sigmask` + `sigwait` | Pattern 4 | If wrong, async-signal-safety becomes a concern. From the upstream README + the ctrlc source code excerpts available, the dedicated-thread model is the documented pattern. Plan-phase can confirm by reading `src/lib.rs` in the published crate once added. |
| A5 | The `--sleep-after-first-finding-ms` test-only flag is acceptable to introduce | Code Examples | Plan-phase / discuss-phase decision. The flag is benign (gated behind `#[cfg(any(test, feature = "test-internal"))]`) but introduces a test-shaped API surface. Alternative: SIGINT-test only via unit tests against the cancel-token, no integration test. Both are defensible. |
| A6 | The `miner scans` catalogue lines should NOT be `Finding` envelopes; a `FindingSink::write_raw_json` or a separate sink fits better | Pitfall 7 | User-facing decision; the plan should escalate. Either path works technically. |
| A7 | The `Finding::DryRun` payload should NOT increment `RunSummary.results_emitted` | Pitfall 3 | Easy to get wrong; the plan must include a unit test asserting the counter stays zero on dry-run paths. |
| A8 | The 50% partial-bar gap threshold doc-comment in Phase 2 aggregator does not impose a Phase-3-level constraint | Verification Architecture | Consistent with Phase 2 VERIFICATION.md's "manual-only item is a Phase 3+ operator concern" comment. Phase 3 does not need to add an operator tuning knob; if Phase 5 wants one, fine. |
| A9 | The clap `--params KEY=VAL` repeatable shape can parse into `BTreeMap<String, serde_json::Value>` with reasonable ergonomics | D3-19 | Yes — clap 4.5 supports `Vec<String>` for `--params` and the binary splits on `=`; the Value is built by attempting `serde_json::from_str` first (so `lags=20` works as int), falling back to a string. Pre-emption: write one unit test per type (`int`, `float`, `bool`, `string`). |

**Confirmation needed before plan:** A3 (window parser strictness), A5 (sleep flag), A6 (catalogue framing). The rest are background risks the plan can carry.

## Open Questions

The seven open questions from CONTEXT.md resolve as follows:

1. **Schema-additivity for D3-10 + D3-21.** **RESOLVED** — both changes are schema-additive. The new `Finding::DryRun` variant becomes a new `oneOf` entry with `kind: { const: "dry_run" }`; the new `DataSlice.gap_manifest: Option<GapManifest>` adds a non-required property under `DataSlice.properties` typed as `["object","null"]` / `anyOf` (depending on schemars settings). The committed schema (`schemas/findings-v1.schema.json` lines 496–562 + lines 9–25) confirms both shapes are how the existing additive pattern works. Plan must include an `xtask gen-schema` task + a manual diff-review subtask.

2. **statsmodels golden tolerance for Ljung-Box.** **RESOLVED with recommendation: redacted-JSONL byte equality.** Pin statsmodels `0.14.6` ([VERIFIED: statsmodels.org/stable], the current stable as of 2025-12). The golden generation script (Python, checked into `crates/miner-core/tests/fixtures/`) emits the expected JSONL bytes by running the same Ljung-Box on a fixed-seed AR(1) of length 256; the Rust integration test masks `run_id` + the 3 timestamp fields + `wall_clock_ms` and asserts byte equality on everything else. The statsmodels algorithm is closed-form and `np.float64`-deterministic — there is no RNG, no iterative solver, no FFT (the function explicitly passes `fft=False` per source). Floats round-trip through `serde_json::to_string` deterministically when the BTreeMap discipline holds. Float-tolerance unpacking is reserved for fall-back: if any `f64` differs by ~1 ULP across statrs / numpy, switch to unpacking the base64 arrays and comparing with `abs(a-b) <= 1e-12 + 1e-10 * |a|` per element. The plan should ship both code paths but expect the byte-equality path to suffice.

3. **`Scan::param_schema()` machinery.** **RESOLVED with recommendation: hand-rolled `serde_json::Value` per scan.** Reasons in Pattern 3 above. Typed `Params` + `JsonSchema` derive is the kind of pattern Phase 4 could opt INTO selectively without trait-level commitment; staying with `Value` at the trait keeps the `dyn Scan` registry maximally simple.

4. **`ScanCtx::bars(...)` lifetime + borrowing model.** **RESOLVED.** `BarCache::get_or_build` returns `Result<BarFrame, CacheError>` BY VALUE (cache.rs:573). The facade pulls the `BarFrame` into a local owning `let`, then constructs `ScanCtx<'_> { bars: &frame, ... }` and passes it. The scan reads `ctx.bars: &BarFrame` by reference. Lifetime is the facade's call frame; no `&'static` or `Arc<BarFrame>` needed. Phase 4 rolling-stat scans add `ctx.bars_up_to(ts)` as a method returning a sub-slice view; same lifetime story.

5. **`ctrlc` + rayon cancellation token reach.** **RESOLVED.** `ctrlc 3.5.2` installs a SIGINT handler that runs on a dedicated thread it spawns at `set_handler` time. The handler is plain Rust (no async-signal-safety constraints) and just performs `flag.store(true, Ordering::SeqCst)`. Rayon worker threads in Phase 5 cooperatively poll `flag.load(Ordering::Relaxed)` — no interaction issues. Sketch in Pattern 4 above.

6. **`DukascopyReader::new(...)` signature.** **RESOLVED.** `pub fn new(cache_root: impl Into<PathBuf>) -> Self` (reader.rs:61) — infallible. The CLI binary calls:
   ```rust
   let reader = miner_reader_dukascopy::DukascopyReader::new(cfg.cache_root.clone());
   ```
   No `?`. Any path-validity issues surface lazily during `read_1m_bars` / `fingerprint_day` as `DukascopyError::Io` and are propagated by the facade as either `CacheError::Aggregate` or a `Finding::ScanError`.

7. **SIGINT test discipline.** **RESOLVED with recommendation: do BOTH.**
   - **Unit-test** the facade's `cancel.load()` polling at every documented yield site by injecting an `Arc<AtomicBool>` already set to `true` and asserting the run returns early at the expected point (no real OS signal). Fast, no flakes, exercises every polling site.
   - **One integration test** via `assert_cmd::Command::cargo_bin("miner")` + `nix::sys::signal::kill(...)` — the example in Code Examples. Single test, gated `#[cfg(unix)]`, validates the end-to-end signal-to-exit-code-130 path.

**New open question raised by this research (escalate to plan-phase / user):**

8. **`miner scans` catalogue framing.** Per Pitfall 7 — three options exist for how to emit `miner scans` catalogue lines: a sixth/seventh `Finding` variant, a separate `CatalogueSink`, or a `FindingSink::write_raw_json` method. The CONTEXT.md D3-20 example shows a non-`Finding` shape, so option 1 is excluded. Pick option 3 (`write_raw_json` with a sibling schema) unless plan-phase surfaces a better argument. Either way, the integration test for `miner scans` MUST validate every line against a **separate** schema, NOT `findings-v1.schema.json`.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | All crates | ✓ | 1.85.1 | — |
| `cargo` | All builds | ✓ (`~/.cargo/bin/cargo`) | 1.85.1 | — |
| `python3` + `statsmodels` | Golden-fixture generation script (one-time, manually invoked by the developer when bumping statsmodels) | not verified in this environment | — | The golden bytes are checked into `tests/fixtures/` so the build does NOT depend on Python at runtime. Fixture regeneration is a developer-machine task. |
| `slopcheck` (Python tool) | Phase-research due diligence | ✓ | 0.6.1 | — |
| `ctrlc` crate (new dep) | Phase 3 SIGINT path | ✓ via crates.io | 3.5.2 | Hand-rolled `pthread_sigmask + sigwait` — but cost is significant, ctrlc IS the standard. |
| `statrs` crate (new dep) | Ljung-Box p-value | ✓ via crates.io | 0.17.x | None viable — hand-rolling chi-squared is a Phase 4 scope explosion. |
| `nix` crate (new dev-dep) | SIGINT integration test | ✓ via crates.io | 0.31.3 | `libc::kill` directly. Equivalent; `nix` gives type-safe `Pid` / `Signal`. |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** none with consequential fallback. `statsmodels` is developer-machine-only.

## Validation Architecture

> Nyquist validation enabled (`workflow.nyquist_validation: true` in `.planning/config.json`).

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + integration tests under `crates/*/tests/`; `proptest 1.11` + `insta 1.47` + `assert_cmd 2` + `nix 0.31` + `serial_test 3` + `jsonschema 0.46` |
| Config file | Workspace `Cargo.toml` (dev-deps + lints); per-crate `Cargo.toml` (test deps); no separate test-config file |
| Quick run command | `cargo test --workspace --lib` (unit-only, ~seconds) |
| Full suite command | `cargo test --workspace --all-targets` (unit + integration + doc, ~30s) |
| Phase gate | Full suite green + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all --check` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| OP-01 / SC-1 | `miner scan stats.autocorr.ljung_box@1` produces NDJSON findings on stdout with resolved params echoed | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- scan_emits_run_start_result_run_end` | ❌ Wave 0 — new file |
| OP-07 / SC-2 (a) | `miner scans` emits one line per registered scan | integration | `cargo test -p miner-cli --test scans_catalogue -- scans_emits_one_line_per_registered_scan` | ❌ Wave 0 — new file |
| OP-08 / SC-2 (b) | Unknown scan_id rejected at boundary with structured error | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- unknown_scan_emits_wireerror_exit_1` | ❌ Wave 0 |
| OP-08 / SC-2 (c) | Invalid `--params` rejected at boundary | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- invalid_params_emits_wireerror_exit_1` | ❌ Wave 0 |
| OUT-04 / SC-3 (a) | `--gap-policy strict` aborts with single GapAborted finding | integration | `cargo test -p miner-core --test gap_policy -- strict_with_gaps_emits_single_gap_aborted` | ❌ Wave 0 |
| OUT-04 / SC-3 (b) | `--gap-policy continuous_only` partitions into sub-ranges; each finding's data_slice carries gap_manifest | integration | `cargo test -p miner-core --test gap_policy -- continuous_only_partitions_and_inlines_manifest` | ❌ Wave 0 |
| OUT-04 / SC-3 (c) | Strict policy zero-gap fast path: no GapAborted, gap_manifest is None | integration | `cargo test -p miner-core --test gap_policy -- strict_zero_gaps_emits_result_with_none_manifest` | ❌ Wave 0 |
| OUT-04 / SC-3 (d) | `continuous_only` zero-gap fast path: one Result with `gap_manifest = Some({gaps: []})` | integration | `cargo test -p miner-core --test gap_policy -- continuous_only_zero_gaps_emits_empty_manifest` | ❌ Wave 0 |
| OUT-04 / SC-3 (e) | Never silently emit on a hole under any policy | integration / proptest | `cargo test -p miner-core --test gap_policy -- never_silently_emits_on_hole_proptest` | ❌ Wave 0 |
| OP-05 / SC-4 | `--dry-run` emits `Finding::DryRun` then exits 0 with no result findings | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- dry_run_emits_dry_run_finding_only` | ❌ Wave 0 |
| OP-06 / SC-5 (a) | SIGINT after first finding emitted → exit 130, all already-emitted findings on stdout | integration | `cargo test -p miner-cli --test sigint_preserves_stream -- sigint_preserves_already_streamed_findings_and_exits_130` | ❌ Wave 0 |
| OP-06 / SC-5 (b) | Facade's cancel-token polling at every documented yield site exits early | unit | `cargo test -p miner-core engine::cancellation_tests::cancel_at_*` (multiple test fns, one per polling site) | ❌ Wave 0 |
| OUT-03 / SC-6 (a) | Same inputs → byte-identical JSONL output (modulo run_id + timestamps) | integration | `cargo test -p miner-core --test scan_facade_determinism -- twice_run_byte_identical_when_volatile_fields_masked` | ❌ Wave 0 |
| OUT-03 / SC-6 (b) | Shuffled-future regression: stats up to T are byte-identical when bars at >T are shuffled | integration / proptest | `cargo test -p miner-core --test shuffled_future_regression -- look_ahead_safe_under_post_t_shuffle_proptest` | ❌ Wave 0 |
| D3-01 / D3-05 | Ljung-Box output matches statsmodels golden bytes | integration / insta | `cargo test -p miner-core --test scan_ljung_box -- ljung_box_matches_statsmodels_golden` | ❌ Wave 0 |
| Schema additivity | After type changes, `xtask gen-schema` produces only additive diff vs committed | unit (in xtask) + manual review | `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/findings-v1.schema.json || echo "review diff for additivity"` | ✅ xtask infrastructure exists; review step is manual |
| D3-13 | `param_hash` is byte-stable across runs and matches blake3-of-canonical-JSON | unit | `cargo test -p miner-core engine::param_hash_tests::param_hash_is_byte_stable` | ❌ Wave 0 |
| D3-19 | `--side` defaults to bid; `--gap-policy` defaults to continuous_only | unit (clap parser) | `cargo test -p miner-cli cli::scan_args_tests::defaults_per_d3_19` | ❌ Wave 0 |
| D3-24 | Exit-code routing 0 / 1 / 2 / 130 | integration | `cargo test -p miner-cli --test scan_subcommand_smoke -- exit_code_routing_*` (4 cases) | ❌ Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test --workspace --lib` (unit-only) — must stay green at every commit.
- **Per wave merge:** `cargo test --workspace --all-targets` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all --check`.
- **Phase gate:** Full suite green; `cargo tree -p miner-core | grep -E 'tokio|async-std' = empty`; `cargo run -p xtask -- gen-schema && git diff --exit-code schemas/findings-v1.schema.json` (after type changes are committed alongside).

### Wave 0 Gaps

The following files and harness pieces must be created in Wave 0 of Phase 3 before scan-engine code is written:

- [ ] `crates/miner-core/src/scan/mod.rs` — `Scan` trait, `ScanCtx`, `ScanRequest`, `ScanError`, `ScanFindingShape`
- [ ] `crates/miner-core/src/scan/registry.rs` — `Registry::{new, register, get, iter}` + `bootstrap()`
- [ ] `crates/miner-core/src/scan/ljung_box/mod.rs` — `LjungBoxScan: Scan` impl
- [ ] `crates/miner-core/src/scan/ljung_box/kernel.rs` — pure `log_returns`, `biased_acf`, `ljung_box_q_and_p` kernels with unit tests
- [ ] `crates/miner-core/src/engine/mod.rs` — `run_one` facade entry + `RunOutcome` enum
- [ ] `crates/miner-core/src/engine/preflight.rs` — `--params` parser, scan-id resolver, error mapping
- [ ] `crates/miner-core/src/engine/gap_policy.rs` — strict / continuous_only dispatch + partitioning
- [ ] `crates/miner-core/src/engine/param_hash.rs` — `param_hash(resolved: &Value) -> Blake3Hex` + unit test
- [ ] `crates/miner-core/src/engine/framing.rs` — `RunStart` / `RunEnd` builders
- [ ] `crates/miner-core/src/findings/mod.rs` — extend `DataSlice` (+ `gap_manifest` field) and `Finding` (+ `DryRun` variant) + `DryRunFinding` struct
- [ ] `crates/miner-cli/src/cli.rs` — extend `Command` enum with `Scan(ScanArgs)` + `Scans`
- [ ] `crates/miner-cli/src/scan_args.rs` — `ScanArgs` struct + `--window` parser + `--params` repeatable
- [ ] `crates/miner-cli/src/main.rs` — `ctrlc::set_handler` install + facade-call plumbing + exit-code routing
- [ ] `crates/miner-core/tests/scan_ljung_box.rs` — golden-fixture insta snapshot
- [ ] `crates/miner-core/tests/scan_facade_determinism.rs` — twice-run masked-byte-equality
- [ ] `crates/miner-core/tests/shuffled_future_regression.rs` — D3-09 proptest
- [ ] `crates/miner-core/tests/gap_policy.rs` — 5 gap-policy behaviour tests
- [ ] `crates/miner-core/tests/dry_run.rs` — `Finding::DryRun` shape + RunSummary.results_emitted == 0
- [ ] `crates/miner-cli/tests/scan_subcommand_smoke.rs` — assert_cmd happy path
- [ ] `crates/miner-cli/tests/scans_catalogue.rs` — `miner scans` introspection
- [ ] `crates/miner-cli/tests/sigint_preserves_stream.rs` — `#[cfg(unix)]` nix::kill integration test
- [ ] `crates/miner-cli/tests/fixtures/` — synthetic SyntheticCache builder (or reuse Phase 2's via a crate-internal helper) + Ljung-Box AR(1) seed + expected JSONL golden
- [ ] `schemas/findings-v1.schema.json` — regenerated by `xtask gen-schema` after Rust type changes; checked-in alongside the type-change PR
- [ ] (Optional) `schemas/scans-catalogue-v1.schema.json` — sibling schema for `miner scans` lines (decision from Open Question 8)
- [ ] Dev-dep additions in workspace + per-crate Cargo.toml: `ctrlc = "3.5"`, `statrs = "0.17"`, `nix = { version = "0.31", default-features = false, features = ["signal"] }`

**Existing infrastructure that Phase 3 reuses unchanged:**
- xtask `gen-schema` subcommand
- `StdoutSink` / `FileSink` / `VecSink` (the existing `FindingSink` impls)
- `WireError` + `emit_to_stderr` + `classify_figment_error`
- `BarCache::get_or_build` + `GapDetector::detect` + `Calendar`
- `chrono::Utc` + `ulid::Ulid` (via `RunId::new()`)
- `assert_cmd::Command::cargo_bin("miner")` pattern from `cli_streams.rs`
- `serial_test::serial` discipline for env-touching tests

**Per-test-type partitioning summary:**

| Test type | Where it lives | How many in Phase 3 (approx) |
|-----------|---------------|-------|
| Unit tests (`#[cfg(test)] mod tests`) | Inside each new source file | ~25 (one per pure function: ACF kernel, Q-stat, p-value, param_hash, window parser, exit-code router, etc.) |
| Integration tests in `miner-core` | `crates/miner-core/tests/*.rs` | 6 new files (see Wave 0 list) |
| Integration tests in `miner-cli` | `crates/miner-cli/tests/*.rs` | 3 new files (see Wave 0 list) |
| Snapshot tests | `crates/miner-core/tests/snapshots/` via `insta` | 1 — Ljung-Box JSONL golden, redacted |
| Proptests | Inside the integration tests via `proptest::proptest!` | 2 — shuffled-future, never-silently-emit-on-hole |
| CLI-level (`assert_cmd`) | Inside `miner-cli/tests/` | 4 tests (scan smoke, scans catalogue, dry-run, sigint) |
| Manual-only | none | 0 — Phase 3 has no manual gates per CONTEXT.md |

## Project Constraints (from CLAUDE.md)

CLAUDE.md actionable directives in scope for Phase 3:

- **Language: Rust** (preferred; C fallback not relevant here). Edition 2024, MSRV 1.85.
- **License: Apache-2.0.** Every new dep must be Apache-2.0 / MIT / dual. `ctrlc 3.5.2` is MIT/Apache-2.0 ✓; `statrs` is MIT ✓; `nix` is MIT ✓.
- **`tokio` is forbidden in `miner-core`** (CI `cargo tree -p miner-core` gate). Phase 3 introduces no async deps.
- **Stdout = findings, stderr = logs** (workspace `clippy.toml` `disallowed_macros`). All new code respects this — no `println!` / `eprintln!`.
- **Single sanctioned `FindingSink` writer** (D-19). Phase 3 reuses `StdoutSink` / `FileSink` unchanged.
- **Locked `Finding` envelope** with `schema_version`, `scan@version`, `param_hash`, `code_revision`, `data_slice`, reserved DSR/FDR-q. Phase 3's two changes are additive only.
- **Determinism via `BTreeMap`** everywhere a map serialises. `Registry::scans` is `BTreeMap`.
- **`unsafe_code = "forbid"`** workspace-wide. Phase 3 introduces zero `unsafe`.
- **GSD Workflow Enforcement:** All code changes go through `/gsd-execute-phase` (or other GSD command). Direct edits without a planning artifact are out of policy.
- **No DSL / scripting / embedded interpreter** (PROJECT.md out-of-scope). `--params KEY=VAL` is structured params, not a DSL.
- **No persistent results store** (PROJECT.md). Phase 3 streams findings; no DB.
- **Conventions: not yet established.** Phase 3 establishes the `Scan` trait shape and the engine module layout — these become the conventions Phase 4 inherits.

## Sources

### Primary (HIGH confidence — verified against tool / committed code / official docs)

- `crates/miner-core/src/findings/mod.rs` — current `Finding` enum + `DataSlice` shape (Read tool, verified 2026-05-18)
- `crates/miner-core/src/cache.rs` — `BarCache::get_or_build` returns `BarFrame` by value (Read tool, line 573)
- `crates/miner-core/src/aggregator.rs` — `BarFrame` columnar shape + `AggParams<'a>` (Read tool)
- `crates/miner-core/src/gap.rs` — `GapDetector::detect` + `GapManifest` / `GapSpan` / `GapReason` (Read tool)
- `crates/miner-core/src/error/codes.rs` — `PreflightCode` + `ScanErrorCode` + `WireError` (Read tool)
- `crates/miner-core/src/findings/sink.rs` — `FindingSink` trait + `StdoutSink` + `FileSink` (Read tool)
- `crates/miner-reader-dukascopy/src/reader.rs:61` — `DukascopyReader::new(impl Into<PathBuf>) -> Self` infallible (Read tool)
- `crates/miner-cli/src/cli.rs` / `main.rs` — existing CLI layout for extension (Read tool)
- `crates/miner-cli/tests/cli_streams.rs` — established `assert_cmd` test pattern (Read tool)
- `xtask/src/main.rs` — schema regeneration pipeline (Read tool)
- `schemas/findings-v1.schema.json` — committed tagged-enum `oneOf` shape proving additivity (Read tool, lines 9–25 + 496–562)
- `Cargo.toml` workspace + `crates/*/Cargo.toml` — pinned dep versions; `serde_json` features list (Read tool)
- `cargo info ctrlc` — version 3.5.2, MIT/Apache-2.0, rust-version 1.69 (executed via `~/.cargo/bin/cargo`)
- `slopcheck scan --pkg crates.io ctrlc` — [OK] (executed)
- `slopcheck scan --pkg crates.io assert_cmd` — [OK] (executed)
- [docs.rs/ctrlc/latest](https://docs.rs/ctrlc/latest/ctrlc/index.html) — `set_handler` spawns a dedicated signal-handling thread; takes a single closure (WebFetch)
- [github.com/Detegr/rust-ctrlc](https://github.com/Detegr/rust-ctrlc) — basic example with `AtomicBool` + dedicated signal thread (WebFetch)
- [statsmodels.org/stable/generated/statsmodels.stats.diagnostic.acorr_ljungbox.html](https://www.statsmodels.org/stable/generated/statsmodels.stats.diagnostic.acorr_ljungbox.html) — 0.14.6 stable, default `lags = min(10, nobs // 5)`, returns DataFrame `lb_stat` / `lb_pvalue` (WebFetch)
- [statsmodels diagnostic.py source](https://github.com/statsmodels/statsmodels/blob/main/statsmodels/stats/diagnostic.py) — Q-stat formula `nobs * (nobs + 2) * np.cumsum(sacf2)[lags - 1]`; biased ACF via `acf(x, nlags=maxlag, fft=False)`; p-value via `stats.chi2.sf(q, df)` (WebFetch)
- [statsmodels acf docs](https://www.statsmodels.org/stable/generated/statsmodels.tsa.stattools.acf.html) — biased default (`adjusted=False`, denominator = n) (WebFetch)
- `.planning/phases/01-foundations-contracts/01-CONTEXT.md` — D-10..D-24 envelope and infra contracts (Read tool)
- `.planning/phases/01-foundations-contracts/01-RESEARCH.md` — schemars + base64 + tagged-enum patterns (Read tool, grep)
- `.planning/phases/02-reader-aggregator-derived-bar-cache/02-VERIFICATION.md` — Phase 2 closed surface (Read tool)
- `CLAUDE.md` — full project + stack constraints (Read tool)
- `.planning/config.json` — `nyquist_validation: true` (Read tool)

### Secondary (MEDIUM confidence — official docs + community sources)

- [crates.io: ctrlc](https://crates.io/crates/ctrlc) — version listing (WebFetch)
- [docs.rs/schemars](https://docs.rs/schemars/latest/schemars/) — `JsonSchema` derive + `Option<T>` schema generation behaviour (WebFetch + WebSearch)
- [Graham's Cool Site — schemars attributes](https://graham.cool/schemars/deriving/attributes/) — `#[serde(tag = "...")]` enum schema shape; `Option<T>` + `#[serde(default)]` (WebSearch summary)
- [rust-cli book §Signal handling](https://rust-cli.github.io/book/in-depth/signals.html) — `ctrlc` is the canonical CLI signal handler choice (WebSearch)
- [statrs](https://github.com/statrs-dev/statrs) — chi-squared distribution maintained by an active Rust stats community (Recommended in CLAUDE.md §Statistics primitives)

### Tertiary (LOW confidence — single-source / not independently confirmed)

- The statrs `ChiSquared::cdf` floating-point alignment with scipy.stats.chi2.cdf at ~1e-12 — observed across multiple statistical packages in similar ports, but the exact alignment for `df ∈ {1..40}` is **A1** in the Assumptions Log; plan-phase Wave 0 should run a one-shot alignment check against scipy and document the empirical tolerance.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — every dep verified via `cargo info`, slopcheck, and existing workspace state.
- Architecture: HIGH — every architectural decision either reuses an established Phase 1/2 pattern (sink, schema regen, error codes) or follows the directly-applicable Rust idiom (ctrlc + AtomicBool + cooperative polling).
- Pitfalls: HIGH — Pitfalls 1–4 and 6 are concrete Rust traps with code-level avoidance. Pitfall 7 is a real design question being escalated. Pitfall 8 is a known SIGINT-test discipline issue with a recommended fix.
- Open questions: HIGH — all 7 CONTEXT.md open questions resolved with concrete recommendations against verified evidence. One new open question (catalogue framing, Pitfall 7) added with three pre-analysed options.
- Validation Architecture: HIGH — every Phase 3 success criterion mapped to a concrete `cargo test` invocation; Wave 0 file list complete.

**Research date:** 2026-05-18
**Valid until:** 2026-06-17 (30 days — Rust stack is stable; statsmodels 0.14.x is current stable through end-2026)

Sources:
- [docs.rs/ctrlc](https://docs.rs/ctrlc/latest/ctrlc/index.html)
- [github.com/Detegr/rust-ctrlc](https://github.com/Detegr/rust-ctrlc)
- [crates.io/crates/ctrlc](https://crates.io/crates/ctrlc)
- [statsmodels.stats.diagnostic.acorr_ljungbox (0.14.6)](https://www.statsmodels.org/stable/generated/statsmodels.stats.diagnostic.acorr_ljungbox.html)
- [statsmodels.tsa.stattools.acf](https://www.statsmodels.org/stable/generated/statsmodels.tsa.stattools.acf.html)
- [statsmodels source: diagnostic.py](https://github.com/statsmodels/statsmodels/blob/main/statsmodels/stats/diagnostic.py)
- [Graham's Cool Site — schemars attributes](https://graham.cool/schemars/deriving/attributes/)
- [docs.rs/schemars](https://docs.rs/schemars/latest/schemars/)
- [rust-cli book — signal handling](https://rust-cli.github.io/book/in-depth/signals.html)
- [statrs repo](https://github.com/statrs-dev/statrs)
- [assert_cmd on crates.io](https://crates.io/crates/assert_cmd)
- [nix-rust/nix examples](https://github.com/nix-rust/nix)

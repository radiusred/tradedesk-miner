# Phase 1: Foundations & Contracts - Context

**Gathered:** 2026-05-15
**Status:** Ready for planning

<domain>
## Phase Boundary

Phase 1 delivers the foundations every later phase consumes:

1. **Cargo workspace skeleton** — `miner-core` (library) plus `miner-reader-dukascopy`, `miner-cli`, `miner-mcp`, `miner-http`, and `miner-bench` crates, with strictly one-way dependency direction enforced.
2. **Locked `Finding` envelope JSON schema** — every field of the agent contract decided and frozen behind `schema_version = 1` before any scan exists. Published as `schemas/findings-v1.schema.json`, derived from Rust types in CI.
3. **stdout = findings, stderr = logs discipline** — CI-enforced via `clippy::disallowed_macros` banning `println!` / `eprintln!` outside the single findings sink module and the logging adapter.
4. **tokio-free `miner-core`** — CI gate via `cargo tree -p miner-core` checking zero async/tokio transitive deps.
5. **Config precedence** — CLI flag > env var > config file with zero hardcoded paths in the library; covers cache root, derived-bar-cache root, output destination.

What Phase 1 does NOT deliver (belongs in later phases): the Dukascopy reader implementation (Phase 2), the aggregator (Phase 2), any scans (Phase 4), wrappers' actual scan plumbing (Phases 3 / 6).

The user is not a Rust practitioner; downstream agents should default to the most pragmatic Rust-community-standard patterns for any choice not explicitly captured below.

</domain>

<decisions>
## Implementation Decisions

### Finding Envelope — Raw-Array Encoding

- **D-01: Always base64.** Every raw array embedded in a finding is encoded as little-endian `f64` bytes, base64-encoded into a JSON string. No `--raw=inline-json` flag. The Quant agent's Python decoder is the canonical one-liner: `np.frombuffer(base64.b64decode(s), dtype="<f8").reshape(shape)`. Single fast path, no encoding-mode branching in the schema. A future `miner decode-raw` CLI subcommand can be added for ad-hoc terminal inspection if it ever becomes a pain point — out of scope for Phase 1.

- **D-02: Self-contained array objects.** Each raw array is one object carrying its own bytes, shape, and dtype: `{ "data": "<base64>", "shape": [n, ...], "dtype": "f64" }`. NOT parallel `series` / `shapes` maps. Eliminates the drift bug where one map gets a key the other doesn't, and makes each array independently decodable without cross-referencing.

- **D-03: Every raw payload includes `timestamps_ms`.** When a finding ships raw arrays, the envelope always carries a `timestamps_ms` array alongside the primary numeric series (same shape/length, encoded as base64 f64 LE epoch milliseconds). The consumer can re-plot or re-test without re-querying the cache. Reason: gap-policy filtering means timestamps are NOT a regular grid derivable from `data_slice.range` — they have to be shipped explicitly. Modest size overhead (~8 bytes per bar) is acceptable.

- **D-04: Clean input/output split in the envelope.**
  - `raw.series.*` carries ONLY the input data the scan consumed (e.g., `returns`, `prices`, `timestamps_ms`). One object per input array using the D-02 shape.
  - `effect.value` is the headline scalar; `effect.extra.*` carries scan-derived outputs (e.g., Ljung-Box `lags`, `acf` array; OLS `alpha`, `beta`, `r_squared`, `residuals` array).
  - Arrays inside `effect.extra` use the same `{ data, shape, dtype }` shape as `raw.series`.
  - Consumer can distinguish "what did this scan see" from "what did this scan produce" without per-scan knowledge.

### Finding Envelope — Error Record Semantics

- **D-05: Mid-stream errors → `kind: scan_error` finding, sweep continues.** If one scan in a sweep blows up mid-run (compute issue, partial cache problem discovered during iteration, scan-internal panic caught at the engine boundary), miner emits a `kind: scan_error` envelope into the stream and proceeds to the next scan. The error record carries: `scan_id@version`, `params`, `instrument(s)`, `side`, `timeframe`, `window`, `error_code` (machine-parseable string), `message` (human-readable), `run_id`. Sweep stops only when literally no scans remain.

- **D-06: Pre-flight errors → stderr structured error + non-zero exit; stdout stays empty.** Unknown scan name, invalid parameters, no such instrument in catalog, missing required config — all rejected synchronously before any work begins. Stderr emits a single structured-error JSON line (e.g., `{ "error": "invalid_parameter", "parameter": "timeframe", "got": "1m", "allowed": ["15m","1h","1d"] }`); stdout emits nothing (no rogue bytes — protects MCP transport). Process exits non-zero. The agent treats pre-flight failure as "fix your request," not "data issue."

- **D-07: Three-tier exit codes.**
  - `0` = run completed; results emitted (zero findings is a valid outcome — "no anomalies" is an answer).
  - `1` = pre-flight error OR catastrophic failure (cache root missing, disk full, internal panic before any output). Stream may be empty.
  - `2` = run completed but at least one mid-stream `scan_error` was emitted. Mixed results + scan_error records present.
  - Agent branches on exit code without parsing the stream.

- **D-08: Gap-policy outputs are findings, not errors.** Under `--gap-policy=strict`, miner emits ONE `kind: gap_aborted` record per scan run carrying the gap manifest; zero result findings; exit code 0 (the policy did its job — this is NOT an error). Under `--gap-policy=continuous_only`, the gap manifest rides along inside each finding's `data_slice` field. Gaps are data conditions, not errors.

### Finding Envelope — Stream Framing

- **D-09: Always emit `run_start` and `run_end` framing records.** Every invocation, every wrapper, every mode (single scan, sweep, CLI, MCP, HTTP): the stream begins with `kind: run_start` and ends with `kind: run_end`. Two extra records per run is cheap; the agent gains explicit "was this run complete or cut off?" detection without exit-code parsing. Human users filter with `jq 'select(.kind=="result")'` in one line.

- **D-10: `run_id` is an always-unique ULID, time-prefixed.** Format: 26-char Crockford-base32 ULID like `01HZF9G09T8K3M4P5Q6R7S8T9V`. Lexicographically sortable by creation time; globally unique; two identical-input replays get different `run_id`s (correct — they're distinct executions). The ULID is also embedded in every result and scan_error finding's `run_id` field for correlation.

- **D-11: Rich framing payloads.**
  - **`run_start`:** `{ kind: "run_start", run_id, started_at_utc, miner_version, code_revision, request }` where `request` is the fully-resolved invocation (scan_id@version, instrument(s), side, timeframe, window, params with defaults applied, gap_policy).
  - **`run_end`:** `{ kind: "run_end", run_id, ended_at_utc, wall_clock_ms, summary }` where `summary` is `{ results_emitted, scan_errors, gap_aborted, per_scan: { "<scan_id@version>": { results, errors, gap_aborted } } }`.
  - Per-scan breakdown is mandatory in sweep mode and trivially small (`{ results: 0, errors: 0, gap_aborted: 0 }`) in single-scan mode.

### Finding Envelope — Schema Artifact

- **D-12: Publish `schemas/findings-v1.schema.json` as a checked-in public-contract artifact.** Quant agent, future tradedesk integration, and any other consumer can validate parsed findings against it. Breaking changes show up as a file diff in PRs — visible, reviewable, hard to merge by accident. Part of the v1 contract from day one.

- **D-13: Schema derived from Rust types via `schemars`.** Envelope structs in `miner-core::findings` carry `#[derive(JsonSchema)]`. A workspace `xtask` or build script regenerates `schemas/findings-v1.schema.json` from those derivations; CI fails the build if the checked-in file is out of sync with what the derivation produces. Single source of truth, no hand-sync drift. `schemars` is the Rust-community default for this pattern.

- **D-14: One schema file per major `schema_version`.** `schemas/findings-v1.schema.json` now. When `schema_version` ever bumps to 2 (breaking change), add `findings-v2.schema.json` alongside; keep v1 for replay/audit of old data. Consumer picks which version to validate against based on the finding's own `schema_version` field. Clear migration story; old findings remain validatable forever.

### Locked Envelope Fields (from STATE.md decisions, reaffirmed here)

Every finding (regardless of `kind`) MUST carry:

- `schema_version` — currently `1`; bumps on breaking envelope change
- `scan_id@version` — e.g., `"stats.autocorr.ljung_box@1"`; identifies which scan emitted it (omitted on `run_start`/`run_end`)
- `param_hash` — blake3 hash of the resolved params after defaults applied
- `code_revision` — git commit SHA (or `dirty-<sha>` on uncommitted builds)
- `data_slice` — what input range was actually used (post-gap-partitioning); structure includes `range: { start_utc, end_utc }`, optionally `gap_manifest_ref` under `continuous_only`
- `dsr` — reserved-but-null in v1; populated in Phase 5 (Deflated Sharpe Ratio)
- `fdr_q` — reserved-but-null in v1; populated in Phase 5 (BH-FDR adjusted q-value)

Result findings additionally carry: `kind: "result"`, `run_id`, `produced_at_utc`, `source` (source_id, symbol, side, timeframe), `params` (resolved), `effect` (`metric`, `value`, optional `p_value`, `n`, optional `ci95`, optional `extra` object), optional `raw` (input arrays per D-04).

Error findings additionally carry: `kind: "scan_error"`, `run_id`, `produced_at_utc`, `error_code`, `message`, and the request context that failed.

Gap-aborted findings additionally carry: `kind: "gap_aborted"`, `run_id`, `produced_at_utc`, `source`, `gap_manifest`.

Framing records carry only the fields specified in D-09 / D-11; they do NOT carry `schema_version` (the envelope contract is owned by the schema file, not the framing).

### Claude's Discretion — Rust-Ecosystem Defaults

The user explicitly deferred Rust-ecosystem choices to "the most pragmatic and/or popular options Rust community members would recognise." Downstream agents should treat the following as defaults; flag in PLAN.md only if research surfaces a concrete reason to deviate.

- **D-15: stdout discipline mechanism.** Add `disallowed-macros` to `clippy.toml` (workspace-level) banning `std::println!` and `std::eprint!` / `std::eprintln!`. The ONLY exemption is the `miner-core::findings::sink` module, which carries a single `#[allow(clippy::disallowed_macros)]` at the module head and wraps stdout in a `BufWriter<Stdout>` newtype that the rest of the crate uses. Logging uses `tracing` macros only — `tracing-subscriber` is configured to write to stderr in every binary's `main()`.

- **D-16: Config crate + format.** `figment` crate for layering. Default file format: TOML. Env-var prefix: `MINER_*` (e.g., `MINER_CACHE_ROOT`, `MINER_BAR_CACHE_ROOT`, `MINER_OUTPUT`). Layering order: defaults < TOML file < env vars < CLI flags (later overrides earlier). Default file lookup path: `$XDG_CONFIG_HOME/miner/miner.toml` (or `~/.config/miner/miner.toml`), falling back to `./miner.toml` in CWD. `--config <path>` flag for explicit override. Library crate carries ZERO hardcoded paths — the CLI / MCP / HTTP wrappers each construct the figment and inject resolved values into `miner-core` calls.

- **D-17: Time crate.** `chrono` 0.4+ throughout. UTC internally for all instants (`DateTime<Utc>`); `NaiveDate` for disk-file day keys; never use `chrono::Local`. `jiff` is the newer/nicer alternative but `chrono` is what Rust folks reach for first and is what ARCHITECTURE.md sketches assume.

- **D-18: Error model.** `thiserror`-derived typed error enums inside `miner-core` (`MinerError`, `ReaderError`, `ScanError` per the architecture sketch); `anyhow::Error` in the wrapper binaries (`miner-cli`, `miner-mcp`, `miner-http`). Library errors implement `serde::Serialize` so they can be embedded inside `kind: scan_error` envelopes via D-05.

- **D-19: Stdout writer pattern.** The `miner-core::findings::sink::StdoutSink` (and any other `FindingSink` impl) is the ONLY type that calls `Write` against `io::stdout()`. All scan and engine code emits findings by calling `sink.write_envelope(&Finding)` — never by formatting JSON themselves. This is what enforces byte-identical output across CLI / MCP / HTTP per ARCHITECTURE.md anti-pattern #6.

- **D-20: Workspace conventions.** `resolver = "2"`, edition `"2024"`, MSRV pinned to `1.85` via workspace `Cargo.toml`. Use `rust-toolchain.toml` (pinned to stable, MSRV-matching) so contributors and CI agree on the compiler version.

- **D-21: CI provider.** GitHub Actions. The four mandatory CI gates from the Phase 1 success criteria are:
  1. `cargo build --workspace` succeeds.
  2. `cargo clippy --workspace --all-targets -- -D warnings` succeeds (disallowed-macros catches stdout violations).
  3. `cargo tree -p miner-core` returns zero matches for `tokio` and `async-` prefixes (custom script grep).
  4. Schema-sync check: regenerate `schemas/findings-v1.schema.json` from the Rust types via `xtask`; `git diff --exit-code schemas/` must succeed.

- **D-22: Schema validation in CI.** Run a small `cargo test` that constructs one example of each envelope `kind` (`result`, `scan_error`, `gap_aborted`, `run_start`, `run_end`), serializes them, and validates each against `schemas/findings-v1.schema.json` using the `jsonschema` crate. This is the runtime-side check that the schema and the actual emitted bytes agree.

- **D-23: `miner-bench` scaffolding for Phase 1.** Phase 1 ships only an empty `miner-bench` binary crate with a placeholder `fn main() { println!("no benches yet"); }` — wait, no: `println!` is banned. Use a single-line `eprintln!` instead with the same module-level `#[allow]` if needed, or just print via the findings-sink discipline. Either way the scaffolding is empty; real bench harness lands in Phase 7.

- **D-24: ULID crate.** `ulid` crate (most-popular Rust ULID implementation) for generating `run_id` values. Seeded RNG inside the ULID generation is acceptable in v1; deterministic-ULID work belongs in Phase 5 (reproducibility envelope).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project-level (always relevant)
- `.planning/PROJECT.md` — Scope, constraints, out-of-scope, agent-operability promise, license.
- `.planning/REQUIREMENTS.md` — v1 requirements, especially FOUND-01..05 and OUT-01..03 mapped to Phase 1.
- `.planning/ROADMAP.md` §"Phase 1: Foundations & Contracts" — Goal statement, depends-on, success criteria (5 enumerated).
- `.planning/STATE.md` — Locked decisions list (workspace shape, `Finding` envelope core fields, sync+rayon core, stdout/findings discipline).

### Research (HIGH-confidence prior work — read before redoing)
- `.planning/research/SUMMARY.md` — Reconciled phase ordering, six CRITICAL pitfalls, stack rationale. Phase 1 corresponds to "Phase 0 / Phase 1: Foundations and Contracts" in this doc.
- `.planning/research/ARCHITECTURE.md` §1 (System Overview), §2 (Workspace Layout), §3 (Module Boundaries), §7 (Findings Envelope), §11 (Architectural Patterns), §14 (Anti-Patterns), §16 (Confidence). The envelope sketch in §7 is the starting point for the locked schema — extend with the decisions above (D-01..D-14).
- `.planning/research/STACK.md` §"TL;DR — The Stack" and §"Recommended Stack — Detailed". Crate picks are recommendations, not yet locked into `Cargo.toml`. Per D-13..D-24 the defaults are: `serde` + `serde_json`, `schemars`, `figment`, `chrono`, `thiserror` + `anyhow`, `tracing` + `tracing-subscriber`, `clap` (in `miner-cli` only), `clippy::disallowed_macros` enforcement, `ulid`, `jsonschema`.
- `.planning/research/PITFALLS.md` — Cross-check planning against this list. Phase 1 must close pitfalls #2 (stdout discipline), #4 (schema breakage), #5 (async contamination — workspace layout), and reserve fields for pitfall #6 (multiple-testing). Pitfalls #1 (look-ahead bias) and #7 (naive aggregation) are Phase 2/3 work but the envelope must accommodate their outputs.
- `.planning/research/FEATURES.md` — Scan-shape inventory; Phase 1 must NOT add scan code, but the envelope `effect` shape (D-04) must be expressive enough to carry every v1 scan's outputs without per-scan envelope extensions.

### Crate documentation to consult during planning (live-verify versions)
- `schemars` (`docs.rs/schemars`) — `JsonSchema` derive, custom-format handling for base64-bytes-with-shape, generating standalone `.schema.json` files.
- `figment` (`docs.rs/figment`) — TOML provider, env provider with prefix, CLI overlay pattern.
- `chrono` (`docs.rs/chrono`) — `DateTime<Utc>`, `NaiveDate`, serde feature flag.
- `ulid` (`docs.rs/ulid`) — generation + lexicographic-sort properties.
- `jsonschema` (`docs.rs/jsonschema`) — runtime validation against a `.schema.json` file.
- `tracing-subscriber` (`docs.rs/tracing-subscriber`) — `fmt().with_writer(std::io::stderr)` pattern.
- `clippy` book — `disallowed-macros` lint configuration via `clippy.toml`.

### Open questions to resolve during plan-phase research
- **`xtask` vs `build.rs` for schema regeneration.** Both work; `xtask` is more idiomatic for "extra commands the user runs occasionally"; `build.rs` makes regen automatic but slows every build. Lean `xtask`.
- **How to express base64-with-shape inside a `schemars`-derived JSON Schema.** Likely a custom `JsonSchema` impl on a newtype wrapper or a manual schema fragment composed in. Verify with a small spike during plan-phase.
- **Exact set of `error_code` strings for D-05 / D-06.** Roadmap Phase 6 mentions `invalid_parameter`, `coverage_gap`, `sweep_too_large`. Lock the Phase 1 vocabulary during planning so the enum is stable from day one.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- **None.** Greenfield Rust project; no prior `tradedesk-miner` source code exists. Sibling repos `tradedesk` (Python) and `tradedesk-dukascopy` (Python) define the upstream cache shape but contain no reusable Rust code. The Dukascopy cache layout (`<root>/<SYMBOL>/<YYYY>/<MM 0-indexed>/<DD>_<bid|ask>.csv.zst`) is the only "asset" inherited from the ecosystem — relevant in Phase 2, not Phase 1.

### Established Patterns
- **No project-internal patterns yet.** Phase 1 establishes them. Downstream agents should treat ARCHITECTURE.md §11 ("Architectural Patterns") as the pattern playbook for Phase 1: trait-object boundary at every plug-point, facade as single contract, reader-driven cache keys, findings as immutable envelopes, rayon for parallelism / no async in core.

### Integration Points
- **`tradedesk-dukascopy` cache on disk** — read-only consumer in Phase 2. Phase 1 must not assume a fixed path; cache root comes through `figment` per D-16. Phase 1 may ship a tiny checked-in fixture cache (a few days, 2-3 instruments) for tests but the format / location of that fixture is a Phase 2 concern.
- **Quant agent (downstream)** — consumes `findings-v1.schema.json` and the JSONL output. Phase 1's job is to lock the contract; no integration code lives in Phase 1.

</code_context>

<specifics>
## Specific Ideas

User is non-Rust and deferred all ecosystem-specific choices to standard practice. The contract-level decisions captured under "Implementation Decisions" are the user's specifics; the rest is delegated to Claude's discretion (D-15..D-24).

Two recurring themes from PROJECT.md the user has emphasised across the project so far:

1. **The Quant agent is THE consumer.** Every envelope decision in Phase 1 should optimise for an automated Python agent that fans out wide sweeps, not for a human staring at JSON. (D-01, D-04, D-05, D-09, D-12 all reflect this.)
2. **Agent-operability across CLI / MCP / HTTP is a hard contract.** Byte-identical findings across all three wrappers — enforced by the `FindingSink` pattern in D-19. This is non-negotiable per ARCHITECTURE.md anti-pattern #6.

</specifics>

<deferred>
## Deferred Ideas

Items that came up during discussion or that the user explicitly wanted deferred:

- **`miner decode-raw` CLI subcommand for ad-hoc terminal inspection** (D-01 follow-up). Not needed in v1; revisit if base64-only output becomes a debugging-friction problem in practice.
- **Progress-event records (`kind: progress`)** for long-running sweeps. Roadmap doesn't mention them; SIGINT is the cancellation pattern (Phase 3). Defer — revisit if sweep wall-clock times grow to the point an agent needs progress signals.
- **Deterministic `run_id` generation from request shape** (would let replay/audit dedup). Conflicts with D-10 (always-unique). If replay/audit ever needs a stable ID, add a separate `request_fingerprint` field rather than overload `run_id`.
- **JSON Schema `$id` / `$schema` URL hosting** (so consumers can fetch by URL). Defer — v1 ships the schema as a checked-in file; hosting / a documentation site is a v2 concern when there's a public consumer outside the RadiusRed family.
- **Real `miner-bench` harness** — Phase 1 ships an empty crate per D-23; the actual criterion + hyperfine harness lands in Phase 7 (Hardening).
- **Schema-derivation strategy alternatives** (hand-written schema with CI cross-validation). Considered as the alternative to D-13; `schemars` derive wins on single-source-of-truth grounds.

None — discussion stayed within Phase 1 scope. Reader / aggregator / cache / scan / wrapper concerns surfaced repeatedly during the envelope conversation but were redirected to their proper phases (2, 2, 2, 4, 6) without losing the context (it's captured in ARCHITECTURE.md and FEATURES.md already).

</deferred>

---

*Phase: 1-Foundations & Contracts*
*Context gathered: 2026-05-15*

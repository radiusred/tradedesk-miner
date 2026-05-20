# Phase 5 — Discussion Log

**Session:** 2026-05-20 (two-part — see "Session interruption" below)
**Phase:** 5 — Statistical Hygiene & Sweep Runner
**Outcome:** All four gray areas delegated to Claude's-discretion pragmatic defaults; CONTEXT.md captures D5-01..D5-05.

## Session interruption

The discuss-phase 5 session was paused mid-flow when the user surfaced a more urgent concern: "before moving on to phase 5, the CI build for phase 4 failed with dozens of warnings. Can we address this first?"

This led to a full Plan 04-13 gap-closure workflow (CI gate 2 / clippy::pedantic unblock), executed inline as a 9-commit atomic-per-category series. See `.planning/phases/04-scan-catalogue-anom-cross-seas/04-13-SUMMARY.md` for the full record.

After Plan 04-13 landed with all 5 CI gates green locally + Phase 4 baseline preserved (796 tests passing, 3 ignored), the user re-invoked `/gsd-discuss-phase 5` to resume Phase 5 discovery. Prior context (PROJECT.md, REQUIREMENTS.md, STATE.md, Phase 3/4 CONTEXTs, codebase scan) was still in conversation memory and reused without re-loading.

## Gray areas presented

The discuss-phase orchestrator presented four phase-specific gray areas via `AskUserQuestion` (multiSelect):

1. **Sweep manifest shape & grid semantics** — the TOML the Quant agent writes (single-scan vs multi-scan, cartesian vs zip / matrix expansion, instruments × timeframes × windows × params combination).
2. **Sweep-summary record & BH-FDR scope** — where q-values live (per-finding `fdr_q` vs end-of-sweep `Finding::SweepSummary` vs both) and what counts as a family (whole sweep, per scan_id, per family prefix, caller-declared).
3. **Effect-size location in envelope (HYG-01)** — parallel scalars vs typed struct vs `effect.extra`.
4. **Bootstrap / null opt-in surface + seed envelope (HYG-03/04/05)** — universal CLI/manifest flags vs per-scan blocks vs on-by-default; per-scan declared support; where the RNG seed + algo lives.

## User response

> "I'll accept pragmatic defaults on all of the above. Let's move to planning"

The user explicitly delegated all four gray areas to Claude's-discretion. The CONTEXT.md captures the pragmatic defaults as D5-01..D5-05 (with D5-04 + D5-05 bundling the bootstrap/null/seed area into two cleanly-separable decisions). Each default is documented clearly enough that plan-phase research can confirm or refine against the cited literature (Politis-Romano 1994, Politis-White 2004, Theiler et al. 1992, Benjamini-Hochberg 1995, statsmodels / scipy reference).

## Decisions captured (Claude's-discretion pragmatic defaults)

### D5-01: Sweep manifest is a TOML file with `[[jobs]]` array; each job expands cartesian across (instruments × timeframes × windows × params)
- Multi-scan job-set in one manifest. Per-job override of `[sweep]` + `[hygiene]` defaults.
- Pair-arity scans take nested `instruments` arrays; Single-arity take flat string arrays.
- Validation: `PreflightCode::InvalidParameter` on arity mismatch; new `PreflightCode::SweepTooLarge` if estimated job count > `[sweep].max_jobs` (default 100,000).
- Deterministic job-graph emission order: block declaration order × within-block cartesian iteration order (instruments → timeframes → windows → params alphabetical).

### D5-02: BH-FDR adjustment scopes per `scan_id@version`; q-values land in an end-of-sweep `Finding::SweepSummary` envelope, NOT in per-finding `fdr_q`
- Streaming-friendly: per-finding `fdr_q` stays `null` during streaming; one `SweepSummary` emitted at end-of-sweep carries per-family `{raw_p, q_value}` keyed by `(scan_id_at_version, finding_index)`.
- FDR family scope default: per `scan_id@version`. Caller override via `[fdr].family = "scan_id" | "scan_family" | "all" | "none"`.
- `Finding::SweepSummary` is a new additive envelope variant (matches Phase 3 D3-21 `Finding::DryRun` precedent).
- Single-shot `miner scan` does NOT emit `SweepSummary` (no family to adjust over).

### D5-03: Effect size lives in a new typed `Effect.effect_size: Option<EffectSize { kind: String, value: f64 }>` field
- NOT parallel scalars (decoupled fields invite drift); NOT `effect.extra` (too unstructured — HYG-01 says it's a first-class envelope output).
- Per-scan canonical `kind` table documented in CONTEXT.md (D5-03 table): `"cohens_d_vs_zero"`, `"hedges_g"`, `"cliffs_delta"`, `"vr_minus_one"`, `"acf_lag_max_abs"`, `"hedge_ratio"`, `"omega_squared"`, etc.
- Plan-phase research pins the exact `kind` strings against scipy / statsmodels.
- Schema-additive change to `Effect`; new `EffectSize` struct.

### D5-04: Bootstrap + null are caller-opt-in via universal CLI / manifest flags; per-scan declared support via two new `Scan` trait methods
- CLI: `--bootstrap stationary|block --bootstrap-n N --null phase_scramble|circular_shift --null-n N --seed <u64>`.
- Manifest: `[hygiene]` block applied to all jobs unless per-job override.
- Default: BOTH OFF (both are O(N) extra resamples).
- New trait methods: `Scan::supports_bootstrap() -> bool` (default false) + `Scan::supports_null_method(NullMethod) -> bool` (default false).
- New `PreflightCode::HygieneNotSupported` variant rejects unsupported requests at preflight.
- New `miner_core::scan::hygiene` module hosts the kernels (`stationary_bootstrap_ci`, `block_bootstrap_ci`, `phase_scramble_null_p`, `circular_shift_null_p`).

### D5-05: Bit-for-bit reproducibility via a new `ResultFinding.repro: Option<ReproEnvelope>` field with derived per-job seeds
- `ReproEnvelope { master_seed, job_seed, bootstrap: Option<BootstrapSpec>, null: Option<NullSpec> }`.
- `master_seed` is user-supplied (`--seed` or `[sweep].seed`); defaults to `blake3(manifest_hash || run_id)` when absent.
- `job_seed = blake3(master_seed || scan_id_at_version || instruments || timeframe || window || param_hash)` — independently reproducible per-job.
- `repro` is `Some(_)` when bootstrap or null was run; `None` otherwise.
- RNG: `rand::rngs::SmallRng` (default) with plan-phase fallback to `rand_xoshiro::Xoshiro256PlusPlus` for explicit cross-version stability.

## Items the user explicitly chose NOT to discuss

By accepting pragmatic defaults on all four areas, the user did not engage on:
- Specific sweep manifest field naming (e.g., `bootstrap_n` vs `n_bootstrap`).
- Whether `Finding::SweepSummary` should also carry per-family `effect_size` summaries.
- Whether `Effect.effect_size.kind` should be a typed enum or a documented open-string. (CONTEXT.md uses open-string for additive flexibility — matches the `ScanErrorCode` precedent.)
- Specific bootstrap inner-loop poll cadence for cancellation.
- Whether `--bootstrap=stationary` should be a flag or a `--bootstrap-method=stationary` named arg.
- Default RNG crate choice (`SmallRng` vs `Xoshiro256PlusPlus`).

All of these are technical-discretion items the user explicitly delegated; plan-phase research owns them per the relevant `<open_questions>` entries in CONTEXT.md.

## Scope creep redirected

None during this session. The user's interruption to address the CI failure was NOT scope creep — it was a hard prerequisite (the Phase 5 plan would have shipped onto a red CI baseline otherwise, undermining the per-task atomic-commit discipline).

## Deferred ideas captured

The CONTEXT.md `<deferred>` block enumerates 13 items explicitly outside Phase 5 scope (DSR, top-N findings, in-memory arena, side-channel raw arrays, PyO3, Johansen, Granger, Hurst, PELT, etc.). All trace back to REQUIREMENTS.md v2 lists or Phase 6/7 boundaries.

## Open questions handed to plan-phase

12 open questions in CONTEXT.md `<open_questions>` covering:
1. Schema-additive guarantee for D5-02 + D5-03 + D5-05.
2. Block-length default (Politis-White auto vs fixed `n^(1/3)`).
3. Per-scan `supports_bootstrap()` + `supports_null_method()` defaults table.
4. Phase scramble algorithm (IAAFT recommended).
5. `SmallRng` cross-version stability (fallback to `Xoshiro256PlusPlus`).
6. `SweepSummary` envelope final schema.
7. Sweep dry-run emission shape (one aggregate vs per-job).
8. Sweep result emission ordering (deterministic recommended).
9. `PreflightCode::SweepTooLarge` default cap (100,000).
10. TOML deserialisation crate (`toml` vs `figment` — `toml` recommended).
11. Sweep + bootstrap + null performance envelope estimate.
12. `hygiene/bootstrap.rs` kernel signature (generic-over-closure recommended).

---

*Discussion log written 2026-05-20. Phase 5 context ready for `/gsd-plan-phase 5`.*

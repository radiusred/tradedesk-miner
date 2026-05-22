---
phase: 04-scan-catalogue-anom-cross-seas
plan: 09
subsystem: scan-catalogue-seas

tags:
  - rust
  - scan
  - seas
  - bucketed

requires:
  - phase: 04-scan-catalogue-anom-cross-seas
    plan: 02
    provides: "primitives::returns::log_returns, primitives::raw_array::f64_slice_to_raw_array, scan::seas::register_seas_scans helper stub, ScanArity::Single + Scan::arity() contract"

provides:
  - "HourOfDayScan (SEAS-01) with id=seas.bucket.hour_of_day, version=1, arity=Single, 24-bucket UTC-hour return profile, effect.value = max-abs t-stat, effect.extra vectors length 24"
  - "DayOfWeekScan (SEAS-02) with id=seas.bucket.day_of_week, version=1, arity=Single, 7-bucket return profile (0=Mon..6=Sun via chrono::Datelike::weekday().num_days_from_monday()), effect.value = max-abs t-stat, effect.extra vectors length 7"
  - "SessionScan (SEAS-03) with id=seas.bucket.session, version=1, arity=Single, 4-bucket trading-session return profile with FX-major defaults (Asia 22-07 wrap, London 07-16, NY 12-21, Overlap 12-16) and configurable boundaries via `--params sessions`; sessions are INDEPENDENT buckets (a bar at 13:00 UTC falls in London, NY, AND Overlap simultaneously per RESEARCH §1.8)"
  - "scan::seas::register_seas_scans now registers three Wave-3 scans alphabetical by id (day_of_week -> hour_of_day -> session)"
  - "Shared scan::seas::bucketing::bucket_stats helper (Welford per-bucket means, Bessel-corrected std, linear-interp IQR) + bucket_stats_from_groups variant for the independent-group case (SessionScan)"
  - "Three integration tests with insta envelope snapshots (scan_seas_hour_of_day, scan_seas_day_of_week, scan_seas_session)"

affects:
  - "04-10 (remaining SEAS scans — eom_som, anova_kw, event_window): consumes the same bucketing helper and Pattern A/D discipline; appends after seas.bucket.session alphabetical-by-id; tightens Plan 04-09 test assertion to expect 6 SEAS registrations"
  - "04-11 (Phase-end integration): the registry::bootstrap() count assertion gets tightened once all 22 scans are registered; goldens for seas.bucket.hour_of_day are authored against pandas.groupby(returns.index.hour).agg([mean, std, count, t_stat])"
  - "Future Phase 5/6 wrappers: catalogue grows by 3 in this plan; every entry includes `arity: \"Single\"`"

tech-stack:
  added:
    - "(none) — Plan 04-01 already added every Phase 4 dep; this plan consumes ndarray-free pure-f64 primitives + chrono date/time API only"
  patterns:
    - "Pattern A — Single-leg SEAS scan body (Scan impl + helpers + named-tests block); applied 3x"
    - "Pattern B — Kernel split (mod.rs + kernel.rs); #[inline] pub(super) pure fns + sibling tests block; applied 3x"
    - "Pattern D — Vector-output finding (ONE envelope per invocation with bucket arrays in effect.extra); applied to all three SEAS scans"
    - "Pattern E — Per-family registrar (`register_seas_scans` appends `r.register(Box::new(...))` lines INSIDE the helper; registry.rs::bootstrap() body untouched)"
    - "Pattern J — Integration test 8-step walk (insta envelope snapshot of masked finding via common::mask_volatile_fields); applied 3x"

key-files:
  created:
    - "crates/miner-core/src/scan/seas/bucketing.rs (479 lines) — shared bucket_stats helper + bucket_stats_from_groups variant + unit tests"
    - "crates/miner-core/src/scan/seas/hour_of_day/mod.rs — HourOfDayScan impl"
    - "crates/miner-core/src/scan/seas/hour_of_day/kernel.rs — hour_keys(ts) -> Vec<usize>"
    - "crates/miner-core/src/scan/seas/day_of_week/mod.rs — DayOfWeekScan impl"
    - "crates/miner-core/src/scan/seas/day_of_week/kernel.rs — weekday_keys(ts) -> Vec<usize>"
    - "crates/miner-core/src/scan/seas/session/mod.rs (802 lines) — SessionScan impl with configurable boundaries + bucket_labels JSON encoding"
    - "crates/miner-core/src/scan/seas/session/kernel.rs (165 lines) — SessionDef + FX_MAJOR_DEFAULTS + wrap-around-aware hour_in_session predicate"
    - "crates/miner-core/tests/scan_seas_hour_of_day.rs"
    - "crates/miner-core/tests/scan_seas_day_of_week.rs"
    - "crates/miner-core/tests/scan_seas_session.rs (139 lines)"
    - "crates/miner-core/tests/snapshots/scan_seas_hour_of_day__scan_seas_hour_of_day_happy_path.snap"
    - "crates/miner-core/tests/snapshots/scan_seas_day_of_week__scan_seas_day_of_week_happy_path.snap"
    - "crates/miner-core/tests/snapshots/scan_seas_session__scan_seas_session_happy_path.snap"
  modified:
    - "crates/miner-core/src/scan/seas/mod.rs — register HourOfDayScan, DayOfWeekScan, SessionScan inside register_seas_scans helper; test renamed to register_seas_scans_registers_three_after_plan_04_09 with >= 3 assertion"

key-decisions:
  - "Plan choice (a) vs (b) for bucket-stats helper: chose (b) — single `bucketing::bucket_stats` helper module inside scan/seas. Cleaner reuse path for SEAS-02/-03/-04/-06 than inlining per-scan."
  - "SEAS-01 / SEAS-02 use sequential Welford accumulation per bucket (Pitfall 4 — deterministic summation); Bessel-corrected std (ddof=1); IQR via linear-interpolation quantile over sorted bucket values."
  - "Sparse buckets (count < min_obs OR count < 2) emit NaN for mean / std / t-stat / IQR rather than rejecting the entire scan; max_abs_finite skips NaN when computing effect.value."
  - "SEAS-03 sessions are INDEPENDENT (NOT mutually exclusive) per RESEARCH §1.8 — a bar at 13:00 UTC falls in London, NY, AND Overlap simultaneously. This required a separate `bucket_stats_from_groups` helper that takes per-session value groups rather than parallel `(values, bucket_keys)`."
  - "SEAS-03 boundary handling: half-open `[start, end)` for non-wrap sessions; explicit wrap arm (`hour >= start || hour < end`) for Asia 22-07."
  - "SEAS-03 configurable boundaries via `--params sessions: [{name, start_utc_hour, end_utc_hour}]`; bounded above by MAX_SESSIONS to keep the wire-form predictable; default = FX_MAJOR_DEFAULTS."
  - "SEAS-03 effect.metric = \"session_max_abs_t_stat\"; effect.extra carries bucket_labels (UTF-8 JSON-encoded session-name string array bytes) AND session_boundaries_utc (JSON-encoded `[{name, start, end}, ...]` bytes), so consumers can recover the active boundary set without re-parsing params."
  - "Per-family registrar contract preserved: `crate::scan::registry.rs::bootstrap()` body NOT touched in this plan. The only registry.rs diff is in test assertions tightened in Plan 04-03 to `>=`."
  - "Lint hygiene: scoped `#[allow(clippy::cast_precision_loss)]` annotations with explicit `reason = ...` added to hour_of_day/day_of_week (counts and bucket-index `as f64` casts) — the values trivially fit f64's 52-bit mantissa. No behaviour change."

requirements-completed:
  - SEAS-01
  - SEAS-02
  - SEAS-03

deviations:
  - "Recovery from watchdog stall: the original executor agent completed all three tasks and ran the full miner-core test suite GREEN, but was killed by the 600s stream-idle watchdog after Task 2's commit landed and before Task 3 could commit. The orchestrator took over inline: verified the staged Task 3 work matched the plan's task definition (SEAS-03 SessionScan with FX_MAJOR_DEFAULTS + wrap-around predicate + bucket_stats_from_groups + integration test + snapshot), re-ran the full test suite (still GREEN), then committed Task 3 with `feat(04-09): SEAS-03 seas.bucket.session@1` and wrote this SUMMARY.md. No code modifications beyond what the executor agent had already staged."

verification:
  - "Full `cargo test -p miner-core` GREEN after Task 3 commit (256 lib tests + every integration test pass)"
  - "scan_seas_hour_of_day_happy_path, scan_seas_day_of_week_happy_path, scan_seas_session_happy_path all GREEN"
  - "register_seas_scans_registers_three_after_plan_04_09 GREEN; SessionScan discoverable via Registry::get(\"seas.bucket.session\", 1)"
  - "Per-family registrar invariant intact: `registry::bootstrap()` body untouched"

next-steps:
  - "Wave 4 (Plan 04-10) appends EOM/SOM + ANOVA/Kruskal-Wallis + event-window to the SEAS family, lifting the register assertion to 6"
  - "Plan 04-11 authors a pandas/scipy golden for seas.bucket.hour_of_day per Success Criterion #5"
---

# Plan 04-09 — SEAS Wave-3 (hour_of_day / day_of_week / session)

## What was built

Three single-shot Single-arity bucketed SEAS scans landed under `crates/miner-core/src/scan/seas/`, following the Pattern A + Pattern D discipline established by Phase 3 LjungBoxScan and refined by Plan 04-03's ANOM scans.

- **SEAS-01 `seas.bucket.hour_of_day@1`** — 24-bucket UTC-hour return profile. Bucket keys come from `chrono::Timelike::hour()` on each timestamp following the first log-return. `effect.value` is the max-abs finite t-stat across the 24 buckets; `effect.extra` carries `buckets` (0..=23), `means`, `stds`, `counts`, `t_stats`, `iqrs` as parallel f64 arrays of length 24.
- **SEAS-02 `seas.bucket.day_of_week@1`** — 7-bucket weekday return profile. Bucket keys come from `chrono::Datelike::weekday().num_days_from_monday()` so 0=Mon..6=Sun per the SEAS-02 contract. Same envelope shape as SEAS-01 with `num_buckets = 7` and `effect.metric = "day_of_week_max_abs_t_stat"`.
- **SEAS-03 `seas.bucket.session@1`** — 4-bucket FX-major trading-session return profile with configurable UTC boundaries. Defaults match RESEARCH §1.8 (Asia 22-07 wrap, London 07-16, NY 12-21, Overlap 12-16). Sessions are **independent** — a bar at 13:00 UTC falls in London, NY, AND Overlap simultaneously, so the helper had to be split from the mutually-exclusive bucketing path (see `bucket_stats_from_groups` below). Boundaries are user-overridable via `--params sessions: [{name, start_utc_hour, end_utc_hour}]`; the active boundary set is echoed in `effect.extra.session_boundaries_utc`.

Shared `scan::seas::bucketing` module provides the per-bucket Welford accumulator with Bessel-corrected std and linear-interpolation IQR, plus a `bucket_stats_from_groups` variant for the independent-group case.

## Recovery note

The original executor agent (`aedf04c2d7e1dffd2`) completed all three tasks and ran the full `cargo test -p miner-core` suite GREEN, but was killed by the 600-second stream-idle watchdog after Task 2's commit landed and before Task 3's commit could be authored. The orchestrator took over inline: re-ran the test suite to confirm the staged Task 3 work was still green, committed Task 3 with the same message style and detail level the executor had used for Tasks 1 and 2, and wrote this SUMMARY. **No code modifications were applied beyond what the executor had already staged in the worktree.**

## Status

3/3 tasks complete. 22-of-22 phase requirements: SEAS-01, SEAS-02, SEAS-03 ticked. Registry now exposes 3 SEAS scans; Plan 04-10 will bring it to 6.

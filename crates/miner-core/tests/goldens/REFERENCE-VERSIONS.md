# Reference Implementation Versions (Phase 4 Golden Pin)

This document pins the Python-side reference implementations used to generate
golden JSONL fixtures for the Phase 4 scan catalogue (ANOM / CROSS / SEAS).
Every byte-equality assertion in `crates/miner-core/tests/scan_<name>.rs`
compares the Rust kernel output against numbers generated with EXACTLY these
versions; bumping any pin requires regenerating every affected golden and
auditing the diff.

Per `04-RESEARCH.md` §1.2 the policy is "pin the path of least resistance" —
Phase 3 already pinned `statsmodels==0.14.6` for the Ljung-Box golden, so
Phase 4 keeps that version and adds `scipy`, `arch`, and `numpy` pins that
the new tests need.

## Pinned versions

| Library                | Version    | Used by                                            |
|------------------------|------------|----------------------------------------------------|
| `python`               | 3.11.x     | Interpreter — all generators                       |
| `statsmodels`          | 0.14.6     | ANOM-04 Ljung-Box variants, ANOM-05 ADF, ANOM-06 KPSS, ANOM-08 ARCH-LM, SEAS-05 ANOVA / Kruskal-Wallis |
| `scipy`                | 1.14.1     | ANOM-09 Jarque-Bera (`scipy.stats.jarque_bera`), SEAS-05 (`scipy.stats.f_oneway` / `kruskal`), variance distributions |
| `arch`                 | 7.2.0      | ANOM-07 Lo-MacKinlay variance ratio (`arch.unitroot.VarianceRatio`) reference  |
| `numpy`                | 1.26.4     | Array kernels, transitive dep of statsmodels/scipy |
| `pandas`               | 2.2.x      | Bar fixture builders / event-window indexing (SEAS-06) |

The Phase 3 statsmodels golden (`crates/miner-core/tests/scan_ljung_box.rs`)
already uses `statsmodels==0.14.6`; Phase 4 keeps the same version pin (the
path of least resistance per `04-RESEARCH.md` §1.2). Bumping statsmodels in
Phase 4 would force regenerating the Phase 3 golden too — not justified.

## Regeneration

```bash
python -m venv /tmp/miner-goldens
/tmp/miner-goldens/bin/pip install --no-deps -r python-requirements.lock
# Regenerate one golden:
/tmp/miner-goldens/bin/python crates/miner-core/tests/goldens/generate_<scan>.py \
  > crates/miner-core/tests/goldens/<scan_id>.jsonl
```

Each per-scan `generate_<scan>.py` script is committed alongside its golden
fixture so the regeneration recipe is self-contained.

## Wire-form provenance

Every golden JSONL carries a top-level `provenance` object documenting the
exact library versions used to generate it. The integration tests assert
`golden["provenance"]["statsmodels_version"] == "0.14.6"` (or `scipy_version`,
`arch_version`, etc.) BEFORE running the equality check — this prevents a
silent version drift from passing the test simply because the algorithms
agree to within the kernel tolerance for the new version.

## Floating-point tolerance budget

Per `04-RESEARCH.md` §2 each scan declares a per-row tolerance:

- `1e-12` — pure kernel arithmetic (`log_returns`, `simple_returns`,
  basic moment statistics).
- `1e-10` — `chi2.cdf` / `norm.cdf` paths (transcendental CDF lookups).
- `1e-8`  — composite estimators that go through multiple statsmodels
  internal calls (ADF, KPSS, ARCH-LM).
- `1e-6`  — full-pipeline composites where statsmodels itself rounds at
  some intermediate step (variance ratio with overlapping windows).

The tolerances are set high enough to absorb expected library-internal
rounding but low enough to detect kernel bugs. Tightening any tolerance to
match exact equality is the regression guard for "did we accidentally drift
from the reference algorithm".

## Phase 3 cross-reference

The Phase 3 `LjungBoxScan` integration test (`crates/miner-core/tests/scan_ljung_box.rs`)
pins `golden["provenance"]["statsmodels_version"] == "0.14.6"`. Phase 4 keeps
this pin verbatim. The Phase 4 D4-06 refactor (LjungBoxScan calls
`primitives::returns::log_returns`) is byte-identical at the kernel level
and therefore the Phase 3 golden continues to pass — no regen needed.

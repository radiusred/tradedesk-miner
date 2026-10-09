#!/usr/bin/env python3
"""Generate `stats.summary.welford.jsonl` from scipy 1.14.1 — golden
reference for Phase 4 Plan 04-11 cross-check of ANOM-02
(`stats.summary.welford@1`).

The Rust integration test
`scan_summary_welford.rs::summary_welford_matches_scipy_describe_golden`
loads the JSONL this script emits, builds a `BarFrame` with the SAME LCG
close-price seed Rust uses, runs `SummaryWelfordScan::run`, and compares
the emitted envelope fields (mean / std / skew / excess_kurtosis / iqr /
min / max / n) against the golden's `expected.*` block within 1e-10
(per `04-RESEARCH.md` §Section 2 tolerance pin for ANOM-02).

REGEN RECIPE (pip-pinned reproducibility):

  python -m venv /tmp/miner-goldens
  /tmp/miner-goldens/bin/pip install \
      statsmodels==0.14.6 scipy==1.14.1 arch==7.2.0 numpy==1.26.4 pandas==2.2.3
  /tmp/miner-goldens/bin/python \
      crates/miner-core/tests/goldens/generate_summary_welford.py \
      > crates/miner-core/tests/goldens/stats.summary.welford.jsonl

Note: Python 3.11.x is the pinned interpreter per
`crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`. Bumping ANY
pin requires regenerating ALL three Phase 4 goldens together (same
session, same library versions) and auditing the diff.

Determinism contract:
- Input is a 64-element LCG-seeded close array reproduced verbatim
  from the Rust `lcg_closes(64, 42)` fixture (multiplier 1664525,
  increment 1013904223, frac = s / u32::MAX, close = 1.0 + frac).
- Output is a SINGLE JSON object on ONE line (JSONL). No trailing
  newline beyond the line terminator; sort_keys=True so byte
  equality is stable across pandas/numpy versions.
"""

from __future__ import annotations

import json
import sys
from datetime import datetime, timezone

import numpy as np
import scipy
from scipy import stats

SCIPY_REQUIRED = "1.14.1"
SEED = 42
N = 64


def lcg_closes(n: int, seed: int) -> list[float]:
    """Reproduce the Rust `lcg_closes(n, seed)` helper byte-for-byte.

    The Rust kernel uses a u32 state with multiplier 1_664_525 and
    increment 1_013_904_223 (the Numerical Recipes / glibc LCG); each
    step computes `frac = f64::from(state) / f64::from(u32::MAX)` and
    emits `1.0 + frac`. Python repro keeps the same f64 conversions so
    the close array is byte-identical to Rust's.
    """
    state = seed & 0xFFFF_FFFF
    out: list[float] = []
    u32_max = float(0xFFFF_FFFF)
    for _ in range(n):
        state = (state * 1_664_525 + 1_013_904_223) & 0xFFFF_FFFF
        frac = float(state) / u32_max
        out.append(1.0 + frac)
    return out


def main() -> None:
    if scipy.__version__ != SCIPY_REQUIRED:
        raise SystemExit(
            f"scipy version mismatch: required {SCIPY_REQUIRED}, got {scipy.__version__}"
        )

    closes = lcg_closes(N, SEED)
    # The ANOM-02 default param is `series=log_returns`; compute log
    # returns the same way Rust's `scan::primitives::returns::log_returns`
    # does — `ln(close[t] / close[t-1])` for t in 1..n.
    closes_np = np.asarray(closes, dtype=np.float64)
    returns = np.log(closes_np[1:] / closes_np[:-1])

    # scipy.stats.describe returns mean, variance (ddof=1 by default for
    # scipy >= 1.10), skewness (BIASED by default — set bias=False to
    # match Rust's G1 estimator), and kurtosis (Fisher = excess kurtosis;
    # BIASED by default — set bias=False to match Rust's G2 estimator).
    n = returns.size
    mean = float(np.mean(returns))
    std = float(np.std(returns, ddof=1))
    skew = float(stats.skew(returns, bias=False))
    excess_kurtosis = float(stats.kurtosis(returns, bias=False, fisher=True))
    iqr = float(stats.iqr(returns, interpolation="linear"))
    minv = float(np.min(returns))
    maxv = float(np.max(returns))

    out = {
        "provenance": {
            "scipy_version": SCIPY_REQUIRED,
            "statsmodels_version": "0.14.6",
            "script_path": "crates/miner-core/tests/goldens/generate_summary_welford.py",
            "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "input_recipe": (
                "Rust lcg_closes(64, 42) — u32 LCG (m=1664525, c=1013904223) "
                "seeded from u32(42); close[i] = 1.0 + f64(state) / f64(u32::MAX)."
            ),
            "input_n": N,
            "input_seed": SEED,
        },
        "input": {
            "closes": closes,
            "series": "log_returns",
            "returns": returns.tolist(),
        },
        "expected": {
            "mean": mean,
            "std": std,
            "skew": skew,
            "excess_kurtosis": excess_kurtosis,
            "iqr": iqr,
            "min": minv,
            "max": maxv,
            "n": n,
        },
    }

    # JSONL is one object per line; sort_keys for byte-stable output.
    sys.stdout.write(json.dumps(out, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Generate `seas.bucket.hour_of_day.jsonl` from pandas 2.2.x + scipy
1.14.1 — golden reference for Phase 4 Plan 04-11 cross-check of
SEAS-01 (`seas.bucket.hour_of_day@1`).

The Rust integration test
`scan_seas_hour_of_day.rs::hour_of_day_matches_pandas_groupby_golden`
loads the JSONL this script emits, builds a 7-day × 24h × 4 bars/hour
BarFrame from the SAME LCG-seeded close array the Rust scan does,
runs `HourOfDayScan::run`, and compares the 24-bucket parallel arrays
(`buckets`, `means`, `stds`, `counts`, `t_stats`, `iqrs`) against the
golden's `expected.*` block within RESEARCH §Section 2 tolerance 1e-12
(pure aggregation — no CDFs).

REGEN RECIPE: see `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`.

Determinism contract:
- Input matches `scan_seas_hour_of_day.rs::build_synthetic_15m_bars(672, 0xDEAD_BEEF)`
  byte-for-byte: 672 closes from the u32 LCG (seed 0xDEAD_BEEF, multiplier
  1664525, increment 1013904223). Timestamps start 2024-01-01T00:00Z at
  15-minute intervals.
- Log returns are `ln(close[t] / close[t-1])` for t in 1..672 (length 671).
- Bucket key is `ts_open_utc[1+i].hour()` for the i-th return, i in 0..671
  — the bar whose CLOSE produced the return.
- Per-bucket stats: mean (numpy default), std (ddof=1 / sample std),
  count, t-stat = mean / (std / sqrt(count)) (NaN when count < 2),
  iqr (scipy.stats.iqr default linear interpolation, NaN when count < 2).

Output is a SINGLE JSON object on ONE line (JSONL); sort_keys=True.
"""

from __future__ import annotations

import json
import sys
from datetime import datetime, timedelta, timezone

import numpy as np
import scipy
from scipy import stats

SCIPY_REQUIRED = "1.14.1"
N = 672
SEED = 0xDEAD_BEEF


def lcg_closes(n: int, seed: int) -> list[float]:
    """Reproduce Rust's `build_synthetic_15m_bars(n, seed).close` array
    via the u32 LCG (1_664_525, 1_013_904_223)."""
    state = seed & 0xFFFF_FFFF
    u32_max = float(0xFFFF_FFFF)
    out: list[float] = []
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
    closes_np = np.asarray(closes, dtype=np.float64)
    returns = np.log(closes_np[1:] / closes_np[:-1])
    nret = returns.size  # 671

    start = datetime(2024, 1, 1, 0, 0, 0, tzinfo=timezone.utc)
    ts_open = [start + timedelta(minutes=15 * i) for i in range(N)]
    # Bucket key for each return: hour of the bar whose close produced it.
    hours = np.array([t.hour for t in ts_open[1:]], dtype=np.int64)

    means = np.full(24, np.nan, dtype=np.float64)
    stds = np.full(24, np.nan, dtype=np.float64)
    counts = np.zeros(24, dtype=np.int64)
    t_stats = np.full(24, np.nan, dtype=np.float64)
    iqrs = np.full(24, np.nan, dtype=np.float64)

    for h in range(24):
        mask = hours == h
        bucket = returns[mask]
        counts[h] = int(bucket.size)
        if bucket.size >= 1:
            means[h] = float(np.mean(bucket))
        if bucket.size >= 2:
            stds[h] = float(np.std(bucket, ddof=1))
            iqrs[h] = float(stats.iqr(bucket, interpolation="linear"))
            sem = stds[h] / float(np.sqrt(bucket.size))
            t_stats[h] = float(means[h] / sem) if sem > 0.0 else float("nan")

    # Headline `effect.value` per the scan: max_abs_t_stat (NaN buckets
    # excluded via np.nanmax).
    finite_t = t_stats[np.isfinite(t_stats)]
    max_abs_t_stat = float(np.max(np.abs(finite_t))) if finite_t.size > 0 else float("nan")

    out = {
        "provenance": {
            "scipy_version": SCIPY_REQUIRED,
            "pandas_version": "2.2.x",
            "script_path": "crates/miner-core/tests/goldens/generate_hour_of_day.py",
            "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "input_recipe": (
                "672 bars at 15m from 2024-01-01T00:00Z. Closes from u32 "
                "LCG (seed 0xDEAD_BEEF). Returns = ln(c[t]/c[t-1]) bucketed "
                "by ts_open_utc[1+i].hour()."
            ),
            "input_n": N,
            "input_seed": f"0x{SEED:08X}",
        },
        "input": {
            "closes": closes,
            "returns": returns.tolist(),
            "hours": hours.tolist(),
        },
        "expected": {
            "buckets": list(range(24)),
            "means": [float(x) if np.isfinite(x) else None for x in means.tolist()],
            "stds": [float(x) if np.isfinite(x) else None for x in stds.tolist()],
            "counts": counts.tolist(),
            "t_stats": [float(x) if np.isfinite(x) else None for x in t_stats.tolist()],
            "iqrs": [float(x) if np.isfinite(x) else None for x in iqrs.tolist()],
            "max_abs_t_stat": max_abs_t_stat,
            "n_returns": nret,
        },
    }

    sys.stdout.write(json.dumps(out, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()

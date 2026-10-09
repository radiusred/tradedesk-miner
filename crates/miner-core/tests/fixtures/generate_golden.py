#!/usr/bin/env python3
"""Generate `ljung_box_golden.json` from statsmodels 0.14.6 — canonical
source-of-truth for Phase 3 Plan 06 Task 1 (Blocker 4 fix per D3-05).

The Rust integration test `scan_ljung_box.rs::ljung_box_matches_statsmodels_golden`
loads the JSON this script emits and compares the Rust kernel output against
it element-by-element within 1e-12. The direction is statsmodels-to-Rust;
NEVER the inverse (per D3-05 / Blocker 4).

Prerequisites: `python3 -m pip install 'statsmodels==0.14.6'`.

Run from the repo root:
    python3 crates/miner-core/tests/fixtures/generate_golden.py

The committed `ljung_box_golden.json` is regenerated on statsmodels version
bumps; commit the diff alongside any consumer changes in the Rust test.
"""

import hashlib
import json
import struct
from datetime import datetime, timezone

import numpy as np
import statsmodels
from statsmodels.stats.diagnostic import acorr_ljungbox
from statsmodels.tsa.stattools import acf

STATSMODELS_REQUIRED = "0.14.6"
N = 256
LAGS = 10
AR_PHI = 0.4
SEED = 0


def generate_close() -> np.ndarray:
    """Deterministic AR(1) close-price series; identical bytes per (seed, N)."""
    rng = np.random.default_rng(SEED)
    eps = rng.standard_normal(N)
    x = np.zeros(N)
    for t in range(1, N):
        x[t] = AR_PHI * x[t - 1] + eps[t]
    return 100.0 * np.exp(x / 100.0)


def log_returns(close: np.ndarray) -> np.ndarray:
    return np.log(close[1:] / close[:-1])


def main() -> None:
    assert (
        statsmodels.__version__ == STATSMODELS_REQUIRED
    ), f"statsmodels version mismatch: required {STATSMODELS_REQUIRED}, got {statsmodels.__version__}"

    close = generate_close()
    returns = log_returns(close)
    acf_values = acf(returns, nlags=LAGS, adjusted=False, fft=False)
    lb = acorr_ljungbox(returns, lags=LAGS, return_df=False)
    q_stats = np.asarray(lb["lb_stat"], dtype=np.float64)
    p_values = np.asarray(lb["lb_pvalue"], dtype=np.float64)

    # input_sha256 is over the LE-f64-packed close-price array bytes.
    input_bytes = struct.pack(f"<{N}d", *close.tolist())
    input_sha256 = hashlib.sha256(input_bytes).hexdigest()

    out = {
        "q_stats": q_stats.tolist(),
        "p_values": p_values.tolist(),
        "acf": acf_values.tolist(),
        "close": close.tolist(),
        "provenance": {
            "statsmodels_version": STATSMODELS_REQUIRED,
            "script_path": "crates/miner-core/tests/fixtures/generate_golden.py",
            "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "input_sha256": input_sha256,
        },
    }
    path = "crates/miner-core/tests/fixtures/ljung_box_golden.json"
    with open(path, "w") as f:
        json.dump(out, f, indent=2, sort_keys=True)
        f.write("\n")
    print(f"wrote {path} ({len(q_stats)} q_stats, {len(acf_values)} acf, input_sha256={input_sha256[:16]}...)")


if __name__ == "__main__":
    main()

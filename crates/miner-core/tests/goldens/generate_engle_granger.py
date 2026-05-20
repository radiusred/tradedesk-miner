#!/usr/bin/env python3
"""Generate `cross.cointegration.engle_granger.jsonl` from statsmodels
0.14.6 — golden reference for Phase 4 Plan 04-11 cross-check of CROSS-05
(`cross.cointegration.engle_granger@1`).

The Rust integration test
`scan_engle_granger.rs::engle_granger_matches_statsmodels_coint_golden`
loads the JSONL this script emits, builds two BarFrames from the SAME
LCG-seeded close arrays Rust uses, runs `EngleGrangerScan::run`, and
compares `effect.value` (β), `effect.extra.hedge_ratio_alpha`,
`effect.extra.adf_stat`, `effect.extra.ou_half_life`,
`effect.extra.residual_std`, and `effect.extra.residuals` against the
golden's `expected.*` block within RESEARCH §Section 2 tolerances:
1e-10 on β / α / residuals (pure OLS arithmetic), 1e-8 on the ADF
MacKinnon p-value table.

REGEN RECIPE: see `crates/miner-core/tests/goldens/REFERENCE-VERSIONS.md`.

Determinism contract:
- Inputs are two 200-element LCG-seeded close arrays built EXACTLY as
  the Rust fixture in `scan_engle_granger.rs::scan_engle_granger_happy_path`
  does: leg_b is a deterministic random walk with seed 0x1357_9BDF and
  step magnitude 0.01; leg_a = leg_b + stationary AR(1) residual with
  φ=0.3 and noise seed 0x0ACE_F123, magnitude 0.005.
- Per D4-09: y = leg_a (regressand, instruments[0]) and x = leg_b
  (regressor, instruments[1]). The reported β is β_y_on_x. Matches
  statsmodels.tsa.stattools.coint(y0=close_a, y1=close_b).
- The two-step coint() function returns (stat, p, crits) but NOT the
  underlying OLS regression — we replicate the OLS step manually via
  statsmodels.regression.linear_model.OLS so we can emit residuals and
  hedge-ratio coefficients alongside.

Output is a SINGLE JSON object on ONE line (JSONL); sort_keys=True.
"""

from __future__ import annotations

import json
import sys
from datetime import datetime, timezone

import numpy as np
import statsmodels
import statsmodels.api as sm
from statsmodels.tsa.stattools import coint

STATSMODELS_REQUIRED = "0.14.6"
N = 200


def lcg_step(state: int) -> tuple[int, float]:
    """One step of the same u32 LCG Rust uses (multiplier 1664525,
    increment 1013904223). Returns (new_state, frac) where frac is
    f64(state) / f64(u32::MAX)."""
    state = (state * 1_664_525 + 1_013_904_223) & 0xFFFF_FFFF
    frac = float(state) / float(0xFFFF_FFFF)
    return state, frac


def build_two_legs() -> tuple[list[float], list[float]]:
    """Reproduce `scan_engle_granger.rs::scan_engle_granger_happy_path`
    leg_a / leg_b close arrays byte-for-byte.

    leg_b = deterministic random walk seeded 0x1357_9BDF with step
    magnitude 0.01; leg_a = leg_b + stationary AR(1) residual with
    φ=0.3 seeded 0x0ACE_F123 and noise magnitude 0.005.
    """
    s_b = 0x1357_9BDF
    closes_b: list[float] = []
    acc = 1.0
    for _ in range(N):
        s_b, frac = lcg_step(s_b)
        dx = (frac - 0.5) * 0.01
        acc += dx
        closes_b.append(acc)

    s_e = 0x0ACE_F123
    closes_a: list[float] = []
    e_prev = 0.0
    for cb in closes_b:
        s_e, frac = lcg_step(s_e)
        noise = (frac - 0.5) * 0.005
        e_t = 0.3 * e_prev + noise
        closes_a.append(cb + e_t)
        e_prev = e_t

    return closes_a, closes_b


def fit_engle_granger(y: np.ndarray, x: np.ndarray) -> dict:
    """Replicate statsmodels.tsa.stattools.coint() two-step pipeline
    while exposing the underlying OLS coefficients + residuals.

    Step 1: OLS y = α + β x + ε via statsmodels.api.OLS.
    Step 2: statsmodels.tsa.stattools.coint() for the canonical ADF
    statistic + MacKinnon p-value (regression='c' = constant trend).
    Step 3: hand-rolled OU half-life on the residual AR(1).
    """
    # Step 1 — OLS hedge regression.
    x_design = sm.add_constant(x)
    ols = sm.OLS(y, x_design).fit()
    alpha = float(ols.params[0])
    beta = float(ols.params[1])
    residuals = (y - (alpha + beta * x)).astype(np.float64)
    residual_std = float(np.std(residuals, ddof=1))

    # Step 2 — coint() returns the ADF test statistic on the residuals
    # using its own internal regression. We use trend='c' (default) and
    # autolag=None (lag=0) to match the local Rust kernel's mid-plan
    # stub. The canonical AIC-lag path lands via the
    # anom.adf reconciliation in this same plan.
    coint_stat, coint_p, _coint_crit = coint(y, x, trend="c", autolag=None, maxlag=0)
    adf_stat = float(coint_stat)
    adf_p_value = float(coint_p)

    # Step 3 — OU half-life: Δr_t = α' + ρ * r_{t-1} + η_t.
    n = residuals.size
    delta_r = residuals[1:] - residuals[:-1]
    lag_r = residuals[:-1]
    lag_design = sm.add_constant(lag_r)
    ou_ols = sm.OLS(delta_r, lag_design).fit()
    rho = float(ou_ols.params[1])
    if rho >= 0.0 or rho <= -1.0:
        ou_half_life = float("inf")
    else:
        denom = np.log(1.0 + rho)
        ou_half_life = float(-np.log(2.0) / denom) if denom != 0.0 else float("inf")

    return {
        "hedge_ratio_alpha": alpha,
        "hedge_ratio_beta": beta,
        "adf_stat": adf_stat,
        "adf_p_value": adf_p_value,
        "ou_half_life": ou_half_life,
        "residual_std": residual_std,
        "residuals": residuals.tolist(),
        "ar1_rho": rho,
    }


def main() -> None:
    if statsmodels.__version__ != STATSMODELS_REQUIRED:
        raise SystemExit(
            f"statsmodels version mismatch: required {STATSMODELS_REQUIRED}, "
            f"got {statsmodels.__version__}"
        )

    closes_a, closes_b = build_two_legs()
    y = np.asarray(closes_a, dtype=np.float64)
    x = np.asarray(closes_b, dtype=np.float64)
    fit = fit_engle_granger(y, x)

    # Deterministic timestamp ladder: 200 bars at 15m starting
    # 2024-01-01T00:00Z. Epoch ms = 1_704_067_200_000 + i * 900_000.
    base_epoch_ms = 1_704_067_200_000
    timestamps_ms = [base_epoch_ms + i * 900_000 for i in range(N)]

    out = {
        "provenance": {
            "statsmodels_version": STATSMODELS_REQUIRED,
            "script_path": "crates/miner-core/tests/goldens/generate_engle_granger.py",
            "generated_at_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "input_recipe": (
                "leg_b: u32 LCG seeded 0x1357_9BDF, step magnitude 0.01, "
                "leg_a: leg_b + AR(1)(φ=0.3, noise=0x0ACE_F123, mag=0.005)."
            ),
            "input_n": N,
        },
        "input": {
            "close_a": closes_a,
            "close_b": closes_b,
            "timestamps_ms": timestamps_ms,
        },
        "expected": {
            "hedge_ratio_alpha": fit["hedge_ratio_alpha"],
            "hedge_ratio_beta": fit["hedge_ratio_beta"],
            "adf_stat": fit["adf_stat"],
            "adf_p_value": fit["adf_p_value"],
            "ou_half_life": fit["ou_half_life"],
            "residual_std": fit["residual_std"],
            "residuals": fit["residuals"],
            "ar1_rho": fit["ar1_rho"],
        },
    }

    sys.stdout.write(json.dumps(out, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()

# Scan catalogue

## Overview

The v1 catalogue exposes 22 requirement-IDs across three families: 11 statistical-anomaly scans on single instruments (ANOM), 5 cross-instrument scans (CROSS), and 6 seasonality scans (SEAS). These map to **23 distinct `scan_id@version` pairs** because `cross.corr.*_rolling` ships both Pearson and Spearman variants under requirement CROSS-02, and `stats.autocorr.ljung_box` ships both raw-returns and squared-returns variants under requirement ANOM-04.

Each scan emits a `Finding::Result` envelope per `(scan x instrument(s) x timeframe x window x param-point)`. The envelope shape is locked — see [findings_envelope.md](findings_envelope.md). Per-scan variation is confined to `effect.metric`, `effect.value`, and the `effect.extra` keys documented below.

Invoke any scan from the CLI:

```bash
miner scan <scan_id@version> --instrument SYM:side --timeframe 15m \
  --window 2024-06-12:2024-06-13 --params lags=5
```

CROSS scans take two `--instrument` flags (one per leg). All scans are byte-identical reproducible across re-runs when given the same `--seed` — see the `repro` envelope in [findings_envelope.md](findings_envelope.md).

`miner scans` introspects the registered catalogue and emits one JSONL row per scan with the canonical `scan_id@version`, `param_schema`, `arity`, and `finding_fields()` shape.

## ANOM — Single-instrument statistical scans

Eleven REQUIREMENTS rows (ANOM-01..11), twelve distinct `scan_id@version` pairs (the Ljung-Box family ships raw + squared-returns variants under ANOM-04).

### stats.returns.profile@1

Returns-series profile dispatching log / simple / intraday / overnight returns by `params.variant`.

- **Arity:** Single
- **effect.metric:** `returns_{variant}_mean` (dynamic per-invocation)
- **effect.value:** arithmetic mean of the chosen returns variant
- **effect.extra keys:** `mean`, `n`, `returns_vector`, `std`, `variant_label`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** numpy.diff (log returns) + numpy.mean / numpy.std
- **When to reach for it:** baseline characterisation of any new instrument / timeframe before deeper tests
- **Requirement:** ANOM-01

### stats.summary.welford@1

Welford running-moments + IQR summary statistics on log returns or closes.

- **Arity:** Single
- **effect.metric:** `summary_welford_mean`
- **effect.value:** arithmetic mean
- **effect.extra keys:** `excess_kurtosis`, `iqr`, `max`, `min`, `n`, `skew`, `std`
- **Raw arrays:** `returns`, `timestamps_ms` (or `closes`, `timestamps_ms` under `series=close`)
- **Reference:** scipy.stats.describe
- **When to reach for it:** quick distributional summary including skew + excess kurtosis
- **Requirement:** ANOM-02

### stats.vol.rolling@1

Rolling realised volatility + vol-of-vol over log returns.

- **Arity:** Single
- **effect.metric:** `vol_rolling_last`
- **effect.value:** last-window realised volatility
- **effect.extra keys:** `values`, `vol_of_vol`, `window_length`, `window_starts_ms`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** pandas.Series.rolling(window).std (vol) + nested rolling std on vol (vol-of-vol)
- **When to reach for it:** detecting volatility regime shifts and vol-of-vol clustering
- **Requirement:** ANOM-03

### stats.autocorr.ljung_box@1

Ljung-Box Q-statistic on raw log returns, testing serial correlation at lags 1..L.

- **Arity:** Single
- **effect.metric:** `ljung_box_q`
- **effect.value:** Q-statistic at max lag (chi-squared with `lags` degrees of freedom)
- **effect.extra keys:** `acf`, `lags`, `p_values`, `q_stats`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** statsmodels.stats.diagnostic.acorr_ljungbox
- **When to reach for it:** detecting linear autocorrelation in returns at short lags (predictability red flag for naive trend models)
- **Requirement:** ANOM-04 (raw returns variant)

### stats.autocorr.ljung_box_sq@1

Ljung-Box Q-statistic on **squared** log returns, testing ARCH effects / volatility clustering.

- **Arity:** Single
- **effect.metric:** `ljung_box_q_squared`
- **effect.value:** Q[lags-1] on squared returns
- **effect.extra keys:** `acf`, `lags`, `p_values`, `q_stats`, `series_kind`
- **Raw arrays:** `returns_squared`, `timestamps_ms`
- **Reference:** statsmodels.stats.diagnostic.acorr_ljungbox (applied to squared returns)
- **When to reach for it:** ARCH / GARCH effect detection; non-trivial Q[sq] with trivial Q[raw] is the classic volatility-clustering signature
- **Requirement:** ANOM-04 (squared returns variant)

### stats.stationarity.adf@1

Augmented Dickey-Fuller unit-root test with AIC lag selection.

- **Arity:** Single
- **effect.metric:** `adf_statistic`
- **effect.value:** τ (the test statistic)
- **effect.extra keys:** `crit_values`, `lag_selected`, `nobs`, `p_value`, `regression`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** statsmodels.tsa.stattools.adfuller (`maxlag=None`, `regression='c'|'ct'|'ctt'|'nc'`, default AIC search)
- **When to reach for it:** unit-root / mean-reversion screening before fitting any mean-reverting model
- **Requirement:** ANOM-05

### stats.stationarity.kpss@1

KPSS stationarity test with Schwert / Hobijn-Franses-Ooms auto-lag formula.

- **Arity:** Single
- **effect.metric:** `kpss_statistic`
- **effect.value:** KPSS statistic (always non-negative)
- **effect.extra keys:** `crit_values`, `lag_truncation`, `p_value`, `regression`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** statsmodels.tsa.stattools.kpss (`regression='c'`, `nlags='auto'`)
- **When to reach for it:** stationarity confirmation; KPSS rejects-with-low-p where ADF fails-to-reject is the textbook stationary case
- **Requirement:** ANOM-06

### stats.variance_ratio.lo_mackinlay@1

Lo-MacKinlay multi-`k` variance ratio test for random-walk behaviour.

- **Arity:** Single
- **effect.metric:** `variance_ratio_max_k`
- **effect.value:** VR at the largest `k` in `k_values`
- **effect.extra keys:** `k_values`, `p_values`, `vr_values`, `z_stats`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** Lo & MacKinlay (1988); the `arch` Python package's variance-ratio module is the canonical reference (no direct statsmodels equivalent)
- **When to reach for it:** detecting mean-reversion (VR<1) or trending (VR>1) deviations from random walk across multiple horizons
- **Requirement:** ANOM-07

### stats.heteroskedasticity.arch_lm@1

Engle ARCH-LM test for conditional heteroskedasticity.

- **Arity:** Single
- **effect.metric:** `arch_lm_statistic`
- **effect.value:** LM statistic
- **effect.extra keys:** `f_p_value`, `f_statistic`, `lag`, `p_value`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** statsmodels.stats.diagnostic.het_arch (with `nlags=L`)
- **When to reach for it:** detecting time-varying conditional variance; complements `ljung_box_sq` with a proper LM test
- **Requirement:** ANOM-08

### stats.normality.jarque_bera@1

Jarque-Bera test of normality based on sample skewness + excess kurtosis.

- **Arity:** Single
- **effect.metric:** `jarque_bera_statistic`
- **effect.value:** JB statistic (chi-squared with 2 df under H0)
- **effect.extra keys:** `excess_kurtosis`, `n`, `p_value`, `skew`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** scipy.stats.jarque_bera
- **When to reach for it:** sanity-checking the normality assumption baked into many parametric tests; fat-tailed returns reject hard
- **Requirement:** ANOM-09

### stats.outliers.z_and_mad@1

Combined z-score + Iglewicz-Hoaglin modified-z outlier detector.

- **Arity:** Single
- **effect.metric:** `outliers_count`
- **effect.value:** number of bars flagged as outliers
- **effect.extra keys:** `mad`, `median`, `modified_z_threshold`, `outlier_indices`, `outlier_values_modified_z`, `outlier_values_z`, `z_threshold`
- **Raw arrays:** `returns`, `timestamps_ms` (or `closes`, `timestamps_ms` under `series=close`)
- **Reference:** Iglewicz & Hoaglin (1993) — `0.6745 * (x - median) / MAD` threshold
- **When to reach for it:** identifying anomalous bars before any moment-based summary; MAD-based detector is robust where z-score is not
- **Requirement:** ANOM-10

### stats.drawdown.profile@1

Drawdown profile of the cumulative-returns equity curve.

- **Arity:** Single
- **effect.metric:** `max_drawdown`
- **effect.value:** maximum drawdown (negative number; `0.0` if monotonically non-decreasing)
- **effect.extra keys:** `dd_distribution_p50_p95_p99`, `drawdown_durations_ms`, `equity_curve`, `peaks`, `time_to_recover_ms`, `troughs`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** standard running-maximum drawdown formulation; equivalent to pandas (cumsum.cummax - cumsum) on log returns
- **When to reach for it:** worst-case loss profiling and recovery-time analysis; complements vol-based risk summaries
- **Requirement:** ANOM-11

## CROSS — Two-instrument scans

Five REQUIREMENTS rows (CROSS-01..05), six distinct `scan_id@version` pairs (the rolling-correlation family ships Pearson + Spearman under CROSS-02). All five scan-IDs are Pair-arity and emit a length-2 `instruments` vector.

CROSS-01 (time-alignment primitive) is a reusable building block, NOT a stand-alone scan_id — it lives at `crates/miner-core/src/scan/primitives/time_alignment.rs` and is used by every two-instrument scan via `inner_join`.

### cross.corr.pearson_rolling@1

Rolling Pearson correlation over aligned log returns.

- **Arity:** Pair
- **effect.metric:** `pearson_corr_last`
- **effect.value:** last-window Pearson r
- **effect.extra keys:** `threshold`, `threshold_crossings`, `values`, `window_length`, `window_starts_ms`
- **Raw arrays:** `returns_a`, `returns_b`, `timestamps_ms`
- **Reference:** hand-rolled rolling `numpy.corrcoef` (tolerance 1e-10 at goldens level)
- **When to reach for it:** dynamic correlation tracking; threshold crossings flag regime changes
- **Requirement:** CROSS-02 (Pearson variant)

### cross.corr.spearman_rolling@1

Rolling Spearman ρ over aligned log returns with `method='average'` tie correction.

- **Arity:** Pair
- **effect.metric:** `spearman_corr_last`
- **effect.value:** last-window Spearman ρ
- **effect.extra keys:** `threshold`, `threshold_crossings`, `values`, `window_length`, `window_starts_ms`
- **Raw arrays:** `returns_a`, `returns_b`, `timestamps_ms`
- **Reference:** scipy.stats.spearmanr (default `method='average'`; tolerance 1e-8)
- **When to reach for it:** monotonic-association tracking robust to extreme moves where Pearson breaks
- **Requirement:** CROSS-02 (Spearman variant)

### cross.ols.rolling@1

Rolling OLS regression on aligned log returns, emitting per-window β, α, R², residual_std.

- **Arity:** Pair
- **effect.metric:** `ols_rolling_beta_last`
- **effect.value:** last-window β (hedge ratio)
- **effect.extra keys:** `alphas`, `betas`, `r2s`, `residual_stds`, `window_length`, `window_starts_ms`
- **Raw arrays:** `returns_a`, `returns_b`, `timestamps_ms`
- **Reference:** statsmodels.regression.rolling.RollingOLS (tolerance 1e-9)
- **When to reach for it:** time-varying hedge-ratio estimation; β instability is a pairs-trading red flag
- **Requirement:** CROSS-03

### cross.lead_lag.ccf@1

Cross-correlation function on a symmetric ±`max_lag` grid, with absolute-value argmax-lag.

- **Arity:** Pair
- **effect.metric:** `lead_lag_argmax_lag`
- **effect.value:** argmax lag (integer-valued, cast to f64)
- **effect.extra keys:** `argmax_lag`, `argmax_value`, `ccf_values`, `lags`, `max_lag`
- **Raw arrays:** `returns_a`, `returns_b`, `timestamps_ms`
- **Reference:** scipy.signal.correlate (mode='full') with normalisation, or statsmodels.tsa.stattools.ccf (tolerance 1e-10)
- **When to reach for it:** detecting which instrument leads; non-zero argmax-lag is the canonical lead-lag signature
- **Requirement:** CROSS-04

### cross.cointegration.engle_granger@1

Engle-Granger two-step cointegration test (OLS hedge-ratio + ADF on residuals + OU half-life).

- **Arity:** Pair
- **effect.metric:** `engle_granger_hedge_ratio`
- **effect.value:** β (the hedge ratio per D4-09)
- **effect.extra keys:** `adf_stat`, `hedge_ratio_alpha`, `ou_half_life`, `residual_std`, `residuals`
- **Raw arrays:** `close_a`, `close_b`, `timestamps_ms` (operates on LEVELS, not returns)
- **Reference:** statsmodels.tsa.stattools.coint (`y0=leg_a`, `y1=leg_b`, `trend='c'`)
- **When to reach for it:** pairs-trading viability screen; cointegration + short OU half-life is the canonical statistical-arbitrage signal
- **Requirement:** CROSS-05

## SEAS — Seasonality scans

Six REQUIREMENTS rows (SEAS-01..06), six distinct `scan_id@version` pairs. All six are Single-arity.

### seas.bucket.hour_of_day@1

Hour-of-day bucketing of returns into 24 buckets with per-bucket summary stats + t-stats.

- **Arity:** Single
- **effect.metric:** `hour_of_day_max_abs_t_stat`
- **effect.value:** max-abs t-statistic across the 24 buckets
- **effect.extra keys:** `buckets`, `counts`, `iqrs`, `means`, `stds`, `t_stats`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** pandas.DataFrame.groupby(hour).agg(mean / std / count) + scipy.stats one-sample t-statistic per bucket
- **When to reach for it:** intraday seasonality detection (FX session opens / closes; equities market open / close)
- **Requirement:** SEAS-01

### seas.bucket.day_of_week@1

Day-of-week bucketing (Mon-Sun) with per-bucket summary stats + t-stats.

- **Arity:** Single
- **effect.metric:** `day_of_week_max_abs_t_stat`
- **effect.value:** max-abs t-statistic across the 7 buckets
- **effect.extra keys:** `buckets`, `counts`, `iqrs`, `means`, `stds`, `t_stats`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** pandas.DataFrame.groupby(dayofweek).agg(...) + scipy.stats one-sample t-statistic per bucket
- **When to reach for it:** weekly seasonality (Monday-effect, weekend-gap effects)
- **Requirement:** SEAS-02

### seas.bucket.session@1

Trading-session bucketing with configurable session definitions (default FX-major: Asia / London / NY / Overlap per RESEARCH §1.8).

- **Arity:** Single
- **effect.metric:** `session_max_abs_t_stat`
- **effect.value:** max-abs t-statistic across configured sessions
- **effect.extra keys:** `bucket_labels`, `counts`, `iqrs`, `means`, `session_boundaries_utc`, `stds`, `t_stats`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Note:** sessions are independent buckets; overlapping windows mean a bar is counted in every matching session. `MAX_SESSIONS = 100` DOS-mitigation ceiling.
- **When to reach for it:** FX-specific seasonality where time-of-day overlaps multiple regional sessions
- **Requirement:** SEAS-03

### seas.bucket.eom_som@1

End-of-month / start-of-month bucketing with `cutoff_n` trading days at each month edge (default 3 → 6 buckets EOM-3..EOM-1, SOM-1..SOM-3).

- **Arity:** Single
- **effect.metric:** `eom_som_max_abs_t_stat`
- **effect.value:** max-abs t-statistic across the configured buckets
- **effect.extra keys:** `bucket_labels`, `counts`, `cutoff_n`, `iqrs`, `means`, `stds`, `t_stats`
- **Raw arrays:** `returns`, `timestamps_ms`
- **When to reach for it:** month-end portfolio-rebalancing effects and month-start positioning flows
- **Requirement:** SEAS-04

### seas.test.anova_kruskal@1

Bundled parametric (ANOVA F-test) + non-parametric (Kruskal-Wallis) test for cross-bucket mean differences, with bucket source selected via `buckets_via ∈ {hour_of_day, day_of_week, session, eom_som}`.

- **Arity:** Single
- **effect.metric:** `anova_f_statistic`
- **effect.value:** F-statistic
- **effect.extra keys:** `anova_p_value`, `group_count`, `kw_p_value`, `kw_stat`, `total_n`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Reference:** scipy.stats.f_oneway (parametric F-stat) and scipy.stats.kruskal (rank-based non-parametric)
- **Note:** the bundled-output design (Plan 04-10) emits BOTH the F-stat and KW-stat in one envelope so consumers can compare parametric vs non-parametric significance without re-running.
- **When to reach for it:** confirming the bucketed-seasonality screens with a formal hypothesis test
- **Requirement:** SEAS-05

### seas.event.pre_post_window@1

Pre / post event-window aggregation around caller-supplied event timestamps.

- **Arity:** Single
- **effect.metric:** `event_post_window_mean`
- **effect.value:** arithmetic mean of post-event-window returns
- **effect.extra keys:** `event_count`, `post_window_bars`, `post_window_means`, `post_window_stds`, `pre_window_bars`, `pre_window_means`, `pre_window_stds`
- **Raw arrays:** `returns`, `timestamps_ms`
- **Note:** caller supplies `event_timestamps` (UTC ms-since-epoch); events outside the bar range or with insufficient pre/post bars are silently skipped. `MAX_EVENT_TIMESTAMPS = 100_000` DOS mitigation (Plan 04-10).
- **When to reach for it:** event-study analysis around NFP / FOMC / ECB / earnings; arbitrary caller-defined event timestamps
- **Requirement:** SEAS-06

## See Also

- [findings_envelope.md](findings_envelope.md) — envelope shape every scan emits
- [sweep_manifest.md](sweep_manifest.md) — submitting multiple scans as a sweep
- [architecture.md](architecture.md) — system map

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://github.com/radiusred) | [Contact](mailto:opensource@radiusred.uk)

# Dukascopy data source caveats

`tradedesk-miner` reads the on-disk cache layout that the sibling project
[`tradedesk-dukascopy`](https://github.com/radiusred/tradedesk-dukascopy)
produces. The cache itself is intentionally simple — daily zstd-compressed
CSVs of 1-minute OHLCV bars — but a few non-obvious conventions matter for
interpreting findings correctly. This document is the deep reference; the
README's `## Data source caveats` section is the one-screen summary.

Audience: agents and humans that need to reason about *what the bytes on
disk actually mean* before trusting a miner finding. If you only ever drive
miner via the CLI, [agent_integration.md](agent_integration.md) is the
better starting point; this doc is the supplementary "data semantics"
layer below it.

## Cache layout

The reader expects this path template, rooted at `MINER_CACHE_ROOT`:

`<root>/<SYMBOL>/<YYYY>/<MM 00-indexed>/<DD>_<bid|ask>.csv.zst`

Worked example — `EURUSD/2024/00/01_bid.csv.zst` resolves to "EURUSD bid
quotes for 2024-01-01" (the `00` in the month component is **January**, not
"missing").

### Months are 00-indexed on disk

This is the single most-surprising convention. Dukascopy's producer-side
tooling stores months as `0..=11` directory names (Python's `datetime.month`
convention minus one), so January is `00` and December is `11`. The miner
reader encapsulates the quirk in one place — see the `day_csv_zst`
constructor at `crates/miner-reader-dukascopy/src/path_layout.rs` (the
`DukascopyMonth` newtype is a sealed wrapper whose only constructors run
through the `1..=12` calendar assertion). Composing a cache path by hand
without going through `day_csv_zst` will silently misfile the data, which
is why path construction is privileged through that single function.

Round-trip and boundary behaviour is pinned by the test suite in the same
file (`jan_maps_to_00`, `dec_maps_to_11`, `round_trip_via_chrono_date_for_every_month_of_2024`,
`full_path_round_trip`, plus a proptest covering every year/month/day combo
in `2010..=2030`). These tests are the regression contract for CACHE-05.

### The synthetic fixture cache mirrors the layout exactly

The checked-in `tests/fixtures/cache/` directory used by the quickstart in
the README is a synthetic-stub cache produced by
`scripts/generate-fixture-cache.sh`. It uses byte-identical path structure
and CSV schema — 00-indexed months included — so anything that runs against
the synthetic fixture runs unchanged against a real Dukascopy cache. The
opposite is also true: a path-construction bug surfaces in the fixture
tests, not later in production. See the `## Licensing posture` section
below for why the fixture cache is synthetic rather than a checked-in
sample of real bytes.

## CSV schema

Each `.csv.zst` is a zstd-compressed CSV of 1-minute OHLCV bars. After
decompression the file has a header row plus one data row per minute, and
miner accepts it through `csv::ReaderBuilder::has_headers(true)` — see
`day_bar_iter` in `crates/miner-reader-dukascopy/src/reader.rs` for the
end-to-end pipeline.

| Column      | CSV type | In-memory type   | Meaning                                                                    | Source           |
|-------------|----------|------------------|----------------------------------------------------------------------------|------------------|
| `timestamp` | string   | `DateTime<Utc>`  | Bar open in UTC, parsed via `%Y-%m-%d %H:%M:%S%:z` (e.g. `2024-01-01 00:00:00+00:00`) | reader.rs `DateTime::parse_from_str` |
| `open`      | f64      | `RawBar.open`    | Mid-quote open for the chosen side (bid or ask)                            | reader.rs `RawRow` |
| `high`      | f64      | `RawBar.high`    | Mid-quote high                                                             | reader.rs `RawRow` |
| `low`       | f64      | `RawBar.low`     | Mid-quote low                                                              | reader.rs `RawRow` |
| `close`     | f64      | `RawBar.close`   | Mid-quote close                                                            | reader.rs `RawRow` |
| `volume`    | f64      | `RawBar.tick_volume` | **Tick count** for the bar — NOT lot/contract volume                  | reader.rs `RawRow` → renamed at boundary |

### Volume is a tick count, not lot volume

This is load-bearing for every scan that reads volume. The producer-side
code in `tradedesk-dukascopy/tradedesk_dukascopy/export.py` aggregates per-
tick volume into the bar via `vol.resample(resample_rule).sum()` (around
line 312). Each tick contributes its `bid_vol + ask_vol` average — but the
ticks themselves are *count-of-prints* from Dukascopy, not size-of-trades
from any exchange, so the resulting bar volume is best thought of as
"activity density" rather than "transacted size". Any scan that treats
volume as a money-volume proxy is treating tick density as money volume;
that's a research decision the scan author has to make explicitly.

Miner surfaces the column as `tick_volume: f64` (`RawBar.tick_volume`) and
NEVER as `volume` — the rename happens at the reader boundary inside
`day_bar_iter` so downstream code can't accidentally conflate the two
semantics. This is the CACHE-05 / D2-13 invariant; see the rename point
in reader.rs (the `RawBar` construction inside the `csv_reader.into_deserialize`
filter closure).

### Timestamp format and time zone

Timestamps in the CSV are wall-clock strings with an offset (e.g.
`2024-03-31 00:30:00+00:00`). The reader parses with the
`%Y-%m-%d %H:%M:%S%:z` format string and immediately converts to UTC via
`with_timezone(&Utc)`. From that point forward — through aggregation,
scanning, finding emission — everything runs in UTC. miner never holds a
local-time `DateTime` on the analysis path. See the `## Time zones and DST`
section below for the regression-test contract that pins this discipline.

Each `RawBar` carries a `ts_open_utc` and a `ts_close_utc = ts_open_utc + 60s`.
The 60-second close offset is a 1-minute-bar convention specific to this
cache; the aggregator preserves UTC discipline when resampling to coarser
timeframes (15m, 1h, 1d).

## Bid vs ask independence

miner treats the bid and ask sides of an instrument as **separate logical
instruments**. The CLI syntax surfaces this directly: a scan invocation
specifies `--instrument SYMBOL:side`, e.g. `--instrument EURUSD:bid`. The
two sides live in separate `.csv.zst` files on disk (`01_bid.csv.zst` vs
`01_ask.csv.zst`) and the reader's `Side` enum routes each request to the
correct file.

### Why no spread reconstruction

miner is the **discovery layer** in the RadiusRed pipeline. It surfaces
statistical candidates from OHLCV data; it does not model microstructure,
quote-driven liquidity, or transaction costs. Spread reconstruction (e.g.
"the rolling bid-ask spread of EURUSD widened by 2σ in this window") is an
execution-layer concern that belongs in `tradedesk`, where it can be
combined with order-book context and venue-specific cost models. Asking
miner to reconstruct spreads would smuggle execution semantics into a
discovery-layer tool — keeping the two layers separate is a deliberate
architectural choice (see the project's `Out of Scope` list in PROJECT.md).

### What an agent should do for spread-aware findings

Run the same scan twice — once against `EURUSD:bid`, once against
`EURUSD:ask` — and compare the two `Finding` envelopes externally. Each
side carries its own `effect.value`, `effect.p_value`, `raw.series`, etc.,
so a downstream consumer that *does* care about bid/ask asymmetry has full
material to work with. If both sides produce statistically equivalent
findings, the effect is unlikely to be a spread artefact; if they diverge,
the spread is probably the story.

## Time zones and DST

**UTC throughout.** This is non-negotiable on the analysis path. The
aggregator (`miner_core::aggregate`) operates exclusively on
`DateTime<Utc>` and `Duration`, with no `chrono-tz`, no `chrono::Local`,
and no wall-clock-to-UTC conversion anywhere inside the kernel. The
regression contract is pinned by two integration tests:

- `crates/miner-core/tests/dst_spring_forward.rs` — the 2024 London
  spring-forward transition (`2024-03-31T01:00:00Z` UTC; London wall
  clocks jump from 01:00 GMT to 02:00 BST). The test proves that the
  *missing* wall-clock hour is invisible in the UTC source data and the
  aggregator's output bars stay evenly spaced in UTC across the transition.
- `crates/miner-core/tests/dst_fall_back.rs` — the 2024 London fall-back
  transition (`2024-10-27T01:00:00Z` UTC; wall clocks fall from 02:00 BST
  to 01:00 GMT). The test proves the *repeated* wall-clock hour is
  similarly invisible — no double-counting, no duplicate `ts_open_utc`.

### The contract this gives the agent

A 15m bar starting at `2024-03-31 01:45:00 UTC` and the next bar starting
at `2024-03-31 02:00:00 UTC` are *exactly* 15 minutes apart regardless of
what London (or any other local clock) was doing that morning. Agents
should never apply local-time reasoning to miner findings. If a finding's
`data_slice.window_utc` straddles a DST boundary, the window length in
elapsed seconds is exactly what the UTC clock measured — no implicit
spring-forward hour subtraction, no implicit fall-back hour addition.

Either DST test, run with a localtime helper accidentally introduced
inside the aggregator, would fail with a 60-minute gap (spring-forward) or
a zero-length gap (fall-back) between two consecutive 15m bars. That's the
regression-gate purpose of those tests; they exist so future contributors
get a loud failure if the UTC discipline ever slips.

## Gap policies

Real OHLCV caches have gaps. miner makes the policy choice explicit per
scan run via the `--gap-policy` flag; the data-source perspective on what
each policy means is below. The consumer-perspective version is in
[agent_integration.md](agent_integration.md) (it focuses on exit-code
routing and the `GapAborted` envelope shape); this section focuses on
*what the data actually looks like*.

### Gaps are mostly NOT data corruption

The most common gaps in a Dukascopy cache are intentional:

- **Weekend gaps.** The FX market closes Friday 22:00 UTC and reopens
  Sunday 22:00 UTC. There is no data for the intervening ~48 hours
  because no quotes are produced.
- **Exchange holidays.** Christmas Day, New Year's Day, and a handful of
  market-specific holidays produce day-long gaps for the same reason.
- **Partial trading days.** Half-day sessions around Thanksgiving (US),
  Boxing Day (UK), Lunar New Year (HK/SG), and others produce partial-day
  coverage where the cache has bars up to the early close and nothing
  after.

None of these are "missing data" — they are the market not trading. A scan
that silently averages or interpolates across them would be the textbook
way to publish nonsense findings, which is why miner forces the caller to
pick a policy.

### `strict` vs `continuous_only`

- **`--gap-policy=strict`** aborts the scan when the precomputed gap
  manifest disallows the requested window. The scan emits a single
  `GapAborted` envelope (NOT a `ScanError` — see
  [findings_envelope.md](findings_envelope.md) for the variant
  taxonomy) carrying the full gap manifest, then exits cleanly (exit
  code 0). The agent can inspect the manifest to decide whether to widen
  the window, switch policy, or skip the slice entirely.
- **`--gap-policy=continuous_only`** partitions the requested window into
  gap-free sub-ranges and emits one `Result` envelope per sub-range. Each
  finding's `data_slice.window_utc` carries the sub-range it actually
  covered; aggregating those sub-ranges back into the original window is
  the consumer's responsibility.

What neither policy will EVER do is silently emit a finding computed over
a range that contains an undisclosed gap. This is the OUT-04 contract; the
gap-manifest precomputation step inside the reader is what makes the
guarantee enforceable.

## Licensing posture

Dukascopy's historical tick data is licensed to the end user under
Dukascopy's terms of use. The sibling project `tradedesk-dukascopy` is the
piece of the RadiusRed pipeline that downloads those bytes on the end
user's machine; miner is purely a consumer of bytes that already exist on
disk and never redistributes them.

### What this repo contains (and does not)

- **Does not contain Dukascopy-licensed bytes.** No real `.csv.zst` files
  from any Dukascopy download are committed to this repository. The fixture
  cache used by the README quickstart and the integration tests at
  `tests/fixtures/cache/` is **synthetic** — generated by
  `scripts/generate-fixture-cache.sh` from a deterministic seed (see
  Plan 07-02's clone-and-run quickstart). Synthetic ≠ random: the
  generator produces bars with realistic-looking OHLCV statistics so the
  fixture exercises the parser, aggregator, and scan engine identically to
  real data, but no Dukascopy bytes are involved.
- **Does contain the path layout and CSV schema.** The on-disk structure
  the reader expects is itself a fact, not a copyrighted artefact; it is
  documented in this file and pinned by the path-layout tests in
  `crates/miner-reader-dukascopy/src/path_layout.rs`.

### What you need for real data

Anyone running miner against real OHLCV bytes needs their own Dukascopy
access and a populated cache produced by `tradedesk-dukascopy` (or any
reader implementation that emits the same on-disk layout — the `Reader`
trait is pluggable, see [architecture.md](architecture.md) for the trait contract).
Dukascopy's upstream terms live at
https://www.dukascopy.com/swiss/english/marketwatch/historical/ — review
those before downloading or redistributing any bytes.

### Verification pin

Verified against tradedesk-dukascopy commit f218d41 (2026-05-13). If the
producer-side conventions (00-indexed months, tick-volume semantics, CSV
column order, timestamp format) drift in a future `tradedesk-dukascopy`
release, the reader's tests at `crates/miner-reader-dukascopy/src/` are
the canonical regression gate; re-verify against the newer commit and
update this line.

## See Also

- [agent_integration.md](agent_integration.md) — consumer-shaped guide to
  running miner from an agent subprocess.
- [findings_envelope.md](findings_envelope.md) — locked envelope schema
  reference (per-field, per-variant).
- [scan_catalogue.md](scan_catalogue.md) — the 23 v1 `scan_id@version`
  strings and per-scan `effect.extra` keys.
- [sweep_manifest.md](sweep_manifest.md) — TOML sweep grammar plus
  hygiene + FDR blocks.
- [architecture.md](architecture.md) — system map; locked envelope
  discipline in context.
- [../README.md](../README.md) — landing page and quickstart.

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

Copyright 2026 [Radius Red Ltd.](https://www.radiusred.uk) | [Contact](mailto:opensource@radiusred.uk)

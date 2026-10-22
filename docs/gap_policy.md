# Gap policy and market calendar

A scan never runs silently over missing data. Before a scan runs, the engine finds the
gaps in the requested window and applies the gap policy. This page says what a gap is,
what the two policies do, and how to tell the miner when your market is closed.

## What a gap is

A gap is a time **when the market is open** and the cache has no data:

- a missing daily file (`missing_source_file`);
- a daily file that cannot be read (`corrupt_source_file`);
- a minute with no bar (`intra_day_gap`).

A time when the market is closed is not a gap. A weekend, a holiday on which the market
does not trade, and a daily trading break have no data because there was no trading.
They do not split a window and they do not abort a `strict` run.

The **market calendar** says when the market is closed. Set it with `--calendar` on
`miner scan`, or with `calendar` in a sweep manifest. The default is `fx_new_york`.

## The two policies

- `continuous_only` (the default): the engine splits the window into the longest
  gap-free sub-ranges and runs the scan once for each. A window without a gap gives one
  `Result`. Each `Result` carries the gap manifest in `data_slice.gap_manifest`.
- `strict`: when the window has a gap, the engine emits one `GapAborted` and no
  `Result`.

## How the engine decides

1. `tradedesk-data`'s `GapDetector` lists the missing minutes and days in the window.
   It checks only the minutes that its own fixed calendar treats as open: Sunday 22:00
   UTC to Friday 22:00 UTC, with 25 December and 1 January closed.
2. The engine removes every minute in which the market calendar says the market is
   closed.
3. The rest is projected onto the timeframe: a hole that does not empty a whole bar at
   the requested timeframe is not a gap at that timeframe. (A `1d` bar can be built from
   the bars that are present.)
4. The policy applies. Under `continuous_only`, a sub-range that holds no open minute
   (closed time between two real gaps) is dropped: it has no bars, so it is not a
   result.

So a multi-year window gives one result unless a real data gap splits it.

`data_slice.gap_manifest` in a `Result` is the detector's full manifest, before steps 2
and 3, so no data-quality information is lost. A `GapAborted` carries the manifest after
steps 2 and 3: the gaps that caused the abort.

## Before the market calendar

Before the market calendar was added, step 2 did not exist. The detector's fixed UTC
calendar alone decided what was open. That calendar is right for an FX market only in the
northern winter. The FX week closes at 17:00 New York time, which is 22:00 UTC in winter
but 21:00 UTC in summer. So in every summer week the hour from 21:00 to 22:00 UTC on
Friday was "open" with no data: a gap. At `15m` and `1h` it split a `continuous_only`
window once a week from about March to November, and made a `strict` run abort. An
instrument with a daily trading break was split every day. At `1d` the hour is smaller
than one bar, so `1d` windows were not split.

The `fx_utc` preset gives the earlier behaviour exactly: `--calendar fx_utc`.

## Presets

| Preset | Time zone | Week | Trading day starts | Closed every year |
|---|---|---|---|---|
| `fx_new_york` (default) | `America/New_York` | Sunday 17:00 to Friday 17:00 | 17:00 | 25 December, 1 January |
| `fx_utc` | `UTC` | Sunday 22:00 to Friday 22:00 | 00:00 | 25 December, 1 January |

## Your own calendar

A calendar is a table. Every time and date is on the market's local clock, so daylight
saving is applied for you.

```toml
time_zone = "America/New_York"   # an IANA time zone name
week_open = "sun 18:00"          # weekday and local time
week_close = "fri 17:00"
trading_day_start = "18:00"      # optional, default "00:00"
daily_breaks = [{ start = "17:00", end = "18:00" }]   # optional
closed_every_year = ["12-25", "01-01"]                # optional, "MM-DD"
closed_dates = ["2024-03-29"]                         # optional, "YYYY-MM-DD"
```

- `week_open` and `week_close` take `mon`, `tue`, `wed`, `thu`, `fri`, `sat` or `sun`
  and a time `HH:MM`. The market is closed from the close to the next open. When the two
  are equal, the market has no weekend.
- `daily_breaks`: each break is closed from `start` to `end`. A break whose `end` is
  before its `start` runs across midnight.
- `closed_every_year` and `closed_dates` close whole **trading days**. With
  `trading_day_start = "00:00"` a trading day is the local calendar day. With a later
  start, the trading day named D begins at that time on the day before D and ends at it
  on D. This is the FX convention: the trading day of 25 December runs from 17:00 New
  York on 24 December to 17:00 New York on 25 December.

For `miner scan`, put the table in a file and give its path:

```sh
miner scan stats.autocorr.ljung_box@1 --instrument EURUSD:bid --timeframe 1h \
    --window 2024-01-01:2024-07-01 --calendar ./my-market.toml
```

In a sweep manifest, give a preset name or the table, in `[sweep]` (for every block) or
in a `[[jobs]]` block (for that block only; it wins):

```toml
[sweep]
calendar = "fx_new_york"

[[jobs]]
scan = "stats.autocorr.ljung_box@1"
instruments = ["EURUSD:bid"]
timeframes = ["1h"]
windows = ["2024-01-01:2024-06-30"]

[jobs.calendar]
time_zone = "America/New_York"
week_open = "sun 18:00"
week_close = "fri 17:00"
daily_breaks = [{ start = "17:00", end = "18:00" }]
```

An unknown preset, an unknown time zone or a field that does not parse is refused at
preflight with `invalid_parameter`. The message names the field.

`run_start.request.calendar` states the calendar in force as its full table, also when
you gave a preset name. A sweep's `run_start` states the `[sweep]` calendar.

## Limits

- The calendar can only remove gaps. The detector does not check a minute that its own
  fixed calendar treats as closed, so the calendar cannot add a gap there. For example,
  with `fx_new_york` in summer the market opens on Sunday at 21:00 UTC, but a hole from
  21:00 to 22:00 UTC is not reported.
- A partial trading day (an early close) is not modelled. Use a daily break on a
  calendar of its own, or accept the gap.
- The calendar does not change the seasonal scans' own trading-day lists; those still use
  the detector's fixed calendar.

---

## License

Licensed under the Apache License, Version 2.0.
See: https://www.apache.org/licenses/LICENSE-2.0

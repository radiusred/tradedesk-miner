//! Market calendar: when a market is closed, so that a hole in the data there is not a gap.
//!
//! The gap detector in `tradedesk-data` flags missing minutes against its own fixed
//! calendar (`Calendar::fx_major`: Sunday 22:00 UTC to Friday 22:00 UTC, 25 December and
//! 1 January closed). A market's hours are set on a local clock, so a fixed UTC calendar
//! is wrong for half of each year: the FX week closes at 17:00 New York time, which is
//! 21:00 UTC in the northern summer. An instrument with a daily trading break has a hole
//! every day. [`MarketCalendar`] states the hours on the market's own clock;
//! [`crate::engine::gap_policy`] removes its closed minutes from a gap manifest before the
//! gap policy splits a window (M33-R2).
//!
//! ## Wire form
//!
//! A calendar is given by a preset name or by a table ([`CalendarSpec`]):
//!
//! ```toml
//! calendar = "fx_new_york"
//!
//! # or
//! [calendar]
//! time_zone = "America/New_York"
//! week_open = "sun 17:00"
//! week_close = "fri 17:00"
//! trading_day_start = "17:00"
//! daily_breaks = [{ start = "17:00", end = "18:00" }]
//! closed_every_year = ["12-25", "01-01"]
//! closed_dates = ["2024-03-29"]
//! ```
//!
//! All times and dates are on the calendar's local clock (`time_zone`, an IANA name).
//! A closed date closes one whole trading day. With `trading_day_start = "00:00"` (the
//! default) a trading day is the local calendar day. With a later start, the trading day
//! named D begins at that local time on the day before D and ends at it on D: the FX
//! convention, in which the day of 25 December runs from 17:00 New York on 24 December
//! to 17:00 New York on 25 December. A daily break whose `end` is before its
//! `start` runs across local midnight. `week_open == week_close` means the market never
//! closes for a weekend.
//!
//! ## Presets
//!
//! - `fx_new_york` (the default): weekly open Sunday 17:00 and close Friday 17:00
//!   `America/New_York`; trading days start at 17:00; 25 December and 1 January closed.
//! - `fx_utc`: the gap detector's own calendar, Sunday 22:00 to Friday 22:00 UTC;
//!   25 December and 1 January closed. It gives the gap dispatch of releases before
//!   M33-R2.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Name of the default calendar preset.
pub const DEFAULT_CALENDAR: &str = "fx_new_york";

/// Preset names, in the order error messages list them.
pub const PRESETS: [&str; 2] = ["fx_new_york", "fx_utc"];

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// A calendar as a user writes it: a preset name or a full table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum CalendarSpec {
    /// A preset name (`fx_new_york` or `fx_utc`).
    Preset(String),
    /// A calendar defined in full.
    Custom(MarketCalendarSpec),
}

impl CalendarSpec {
    /// Validate the spec into a [`MarketCalendar`].
    ///
    /// # Errors
    /// A message naming the bad field when the preset is unknown or a field does not parse.
    pub fn resolve(&self) -> Result<MarketCalendar, String> {
        match self {
            Self::Preset(name) => MarketCalendar::preset(name),
            Self::Custom(spec) => MarketCalendar::try_from(spec.clone()),
        }
    }
}

/// The table form of a calendar. Every time and date is on the local clock of `time_zone`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarketCalendarSpec {
    /// IANA time zone name, for example `"America/New_York"` or `"UTC"`.
    pub time_zone: String,
    /// Weekly open: a weekday and a local time, `"sun 17:00"`.
    pub week_open: String,
    /// Weekly close: a weekday and a local time, `"fri 17:00"`.
    pub week_close: String,
    /// Local time at which a trading day begins, `"HH:MM"`. `"00:00"` (the
    /// default): the local calendar day. Later: the trading day named D begins
    /// at this time on the day before D. Closed days are trading days.
    #[serde(default = "midnight")]
    pub trading_day_start: String,
    /// Daily breaks, each a local `[start, end)`; `end < start` runs across midnight.
    #[serde(default)]
    pub daily_breaks: Vec<DailyBreakSpec>,
    /// Trading days closed every year, as `"MM-DD"`.
    #[serde(default)]
    pub closed_every_year: Vec<String>,
    /// Single trading days closed, as `"YYYY-MM-DD"`.
    #[serde(default)]
    pub closed_dates: Vec<String>,
}

fn midnight() -> String {
    "00:00".to_string()
}

/// One daily break as a user writes it: local `"HH:MM"` start and end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DailyBreakSpec {
    pub start: String,
    pub end: String,
}

// ---------------------------------------------------------------------------
// MarketCalendar — the validated calendar
// ---------------------------------------------------------------------------

/// A validated market calendar. Build it from a [`CalendarSpec`] or a preset.
///
/// Serialises as its [`MarketCalendarSpec`] table, so a `run_start` echo states every
/// field that was in force, also for a preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MarketCalendarSpec", into = "MarketCalendarSpec")]
pub struct MarketCalendar {
    tz: Tz,
    week_open: (Weekday, NaiveTime),
    week_close: (Weekday, NaiveTime),
    trading_day_start: NaiveTime,
    daily_breaks: Vec<(NaiveTime, NaiveTime)>,
    closed_every_year: Vec<(u32, u32)>,
    closed_dates: Vec<NaiveDate>,
}

impl Default for MarketCalendar {
    fn default() -> Self {
        Self::preset(DEFAULT_CALENDAR).expect("the default preset is valid")
    }
}

impl MarketCalendar {
    /// The calendar a preset name stands for.
    ///
    /// # Errors
    /// A message listing the presets when `name` is not one of them.
    pub fn preset(name: &str) -> Result<Self, String> {
        let (time_zone, open, close, day_start) = match name {
            "fx_new_york" => ("America/New_York", "sun 17:00", "fri 17:00", "17:00"),
            "fx_utc" => ("UTC", "sun 22:00", "fri 22:00", "00:00"),
            other => {
                return Err(format!(
                    "unknown calendar preset {other:?}; expected one of {PRESETS:?} or a table"
                ));
            }
        };
        Self::try_from(MarketCalendarSpec {
            time_zone: time_zone.to_string(),
            week_open: open.to_string(),
            week_close: close.to_string(),
            trading_day_start: day_start.to_string(),
            daily_breaks: Vec::new(),
            closed_every_year: vec!["12-25".to_string(), "01-01".to_string()],
            closed_dates: Vec::new(),
        })
    }

    /// Read a calendar table from TOML text (top-level `time_zone`,
    /// `week_open`, `week_close`, ...): the form of a `--calendar` file.
    ///
    /// # Errors
    /// A message when the text is not a calendar table or a field does not parse.
    pub fn from_toml_str(text: &str) -> Result<Self, String> {
        let spec: MarketCalendarSpec = toml::from_str(text).map_err(|e| e.to_string())?;
        Self::try_from(spec)
    }

    /// Whether the market is open at `ts`.
    ///
    /// Closed on a closed date or a yearly closed day (the whole trading day), in a
    /// daily break, and between the weekly close and the weekly open. Open otherwise.
    #[must_use]
    pub fn is_open_at(&self, ts: DateTime<Utc>) -> bool {
        let local = ts.with_timezone(&self.tz);
        let date = self.trading_date(local.naive_local());
        if self.closed_dates.contains(&date)
            || self.closed_every_year.contains(&(date.month(), date.day()))
        {
            return false;
        }
        let t = local.time();
        if self
            .daily_breaks
            .iter()
            .any(|&(start, end)| in_daily_span(t, start, end))
        {
            return false;
        }
        let now = week_seconds(local.weekday(), t);
        let open = week_seconds(self.week_open.0, self.week_open.1);
        let close = week_seconds(self.week_close.0, self.week_close.1);
        match open.cmp(&close) {
            std::cmp::Ordering::Equal => true,
            std::cmp::Ordering::Less => now >= open && now < close,
            std::cmp::Ordering::Greater => now >= open || now < close,
        }
    }
}

impl MarketCalendar {
    /// The trading day that local time `local` falls in.
    fn trading_date(&self, local: chrono::NaiveDateTime) -> NaiveDate {
        let start = i64::from(self.trading_day_start.num_seconds_from_midnight());
        if start == 0 {
            local.date()
        } else {
            (local + chrono::Duration::seconds(86_400 - start)).date()
        }
    }
}

/// Seconds since Monday 00:00 of the local week.
fn week_seconds(day: Weekday, t: NaiveTime) -> u32 {
    day.num_days_from_monday() * 86_400 + t.num_seconds_from_midnight()
}

/// `t` in the local daily span `[start, end)`; a span with `end < start` runs across
/// midnight.
fn in_daily_span(t: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    if start < end {
        t >= start && t < end
    } else {
        t >= start || t < end
    }
}

impl TryFrom<MarketCalendarSpec> for MarketCalendar {
    type Error = String;

    fn try_from(spec: MarketCalendarSpec) -> Result<Self, String> {
        let tz: Tz = spec.time_zone.parse().map_err(|_| {
            format!(
                "calendar.time_zone: unknown IANA time zone {:?}",
                spec.time_zone
            )
        })?;
        let week_open = parse_weekly(&spec.week_open).ok_or_else(|| {
            format!(
                "calendar.week_open: expected \"<day> HH:MM\", got {:?}",
                spec.week_open
            )
        })?;
        let week_close = parse_weekly(&spec.week_close).ok_or_else(|| {
            format!(
                "calendar.week_close: expected \"<day> HH:MM\", got {:?}",
                spec.week_close
            )
        })?;
        let trading_day_start = parse_hm(&spec.trading_day_start).ok_or_else(|| {
            format!(
                "calendar.trading_day_start: expected \"HH:MM\", got {:?}",
                spec.trading_day_start
            )
        })?;
        let mut daily_breaks = Vec::with_capacity(spec.daily_breaks.len());
        for b in &spec.daily_breaks {
            let start = parse_hm(&b.start).ok_or_else(|| {
                format!(
                    "calendar.daily_breaks: expected \"HH:MM\", got {:?}",
                    b.start
                )
            })?;
            let end = parse_hm(&b.end).ok_or_else(|| {
                format!("calendar.daily_breaks: expected \"HH:MM\", got {:?}", b.end)
            })?;
            if start == end {
                return Err(format!(
                    "calendar.daily_breaks: start and end are both {:?}; a break must not be empty",
                    b.start
                ));
            }
            daily_breaks.push((start, end));
        }
        let mut closed_every_year = Vec::with_capacity(spec.closed_every_year.len());
        for md in &spec.closed_every_year {
            closed_every_year.push(parse_month_day(md).ok_or_else(|| {
                format!("calendar.closed_every_year: expected \"MM-DD\", got {md:?}")
            })?);
        }
        let mut closed_dates = Vec::with_capacity(spec.closed_dates.len());
        for d in &spec.closed_dates {
            closed_dates.push(NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| {
                format!("calendar.closed_dates: expected \"YYYY-MM-DD\", got {d:?}")
            })?);
        }
        Ok(Self {
            tz,
            week_open,
            week_close,
            trading_day_start,
            daily_breaks,
            closed_every_year,
            closed_dates,
        })
    }
}

impl From<MarketCalendar> for MarketCalendarSpec {
    fn from(cal: MarketCalendar) -> Self {
        let weekly =
            |(day, t): (Weekday, NaiveTime)| format!("{} {}", weekday_name(day), t.format("%H:%M"));
        Self {
            time_zone: cal.tz.name().to_string(),
            week_open: weekly(cal.week_open),
            week_close: weekly(cal.week_close),
            trading_day_start: cal.trading_day_start.format("%H:%M").to_string(),
            daily_breaks: cal
                .daily_breaks
                .iter()
                .map(|(s, e)| DailyBreakSpec {
                    start: s.format("%H:%M").to_string(),
                    end: e.format("%H:%M").to_string(),
                })
                .collect(),
            closed_every_year: cal
                .closed_every_year
                .iter()
                .map(|(m, d)| format!("{m:02}-{d:02}"))
                .collect(),
            closed_dates: cal
                .closed_dates
                .iter()
                .map(|d| d.format("%Y-%m-%d").to_string())
                .collect(),
        }
    }
}

fn parse_hm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

fn parse_weekly(s: &str) -> Option<(Weekday, NaiveTime)> {
    let (day, time) = s.trim().split_once(' ')?;
    let day = match day.to_ascii_lowercase().as_str() {
        "mon" => Weekday::Mon,
        "tue" => Weekday::Tue,
        "wed" => Weekday::Wed,
        "thu" => Weekday::Thu,
        "fri" => Weekday::Fri,
        "sat" => Weekday::Sat,
        "sun" => Weekday::Sun,
        _ => return None,
    };
    Some((day, parse_hm(time)?))
}

fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "mon",
        Weekday::Tue => "tue",
        Weekday::Wed => "wed",
        Weekday::Thu => "thu",
        Weekday::Fri => "fri",
        Weekday::Sat => "sat",
        Weekday::Sun => "sun",
    }
}

fn parse_month_day(s: &str) -> Option<(u32, u32)> {
    let (m, d) = s.trim().split_once('-')?;
    let (m, d) = (m.parse::<u32>().ok()?, d.parse::<u32>().ok()?);
    // 2024 is a leap year, so 02-29 is accepted.
    NaiveDate::from_ymd_opt(2024, m, d).map(|_| (m, d))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn custom(spec_toml: &str) -> Result<MarketCalendar, String> {
        let spec: CalendarSpec = toml::from_str::<toml::Table>(spec_toml)
            .map_err(|e| e.to_string())
            .and_then(|t| {
                CalendarSpec::deserialize(toml::Value::Table(t)).map_err(|e| e.to_string())
            })?;
        spec.resolve()
    }

    /// The New York weekly close is 21:00 UTC in the northern summer and 22:00 UTC in
    /// the winter; the open moves with it.
    #[test]
    fn fx_new_york_week_follows_new_york_daylight_saving() {
        let cal = MarketCalendar::preset("fx_new_york").unwrap();
        // Summer (EDT, UTC-4): Friday 2024-06-14 closes at 21:00 UTC.
        assert!(cal.is_open_at(at(2024, 6, 14, 20, 59)));
        assert!(!cal.is_open_at(at(2024, 6, 14, 21, 0)));
        assert!(!cal.is_open_at(at(2024, 6, 15, 12, 0)));
        assert!(!cal.is_open_at(at(2024, 6, 16, 20, 59)));
        assert!(cal.is_open_at(at(2024, 6, 16, 21, 0)));
        // Winter (EST, UTC-5): Friday 2024-01-12 closes at 22:00 UTC.
        assert!(cal.is_open_at(at(2024, 1, 12, 21, 59)));
        assert!(!cal.is_open_at(at(2024, 1, 12, 22, 0)));
        assert!(!cal.is_open_at(at(2024, 1, 14, 21, 59)));
        assert!(cal.is_open_at(at(2024, 1, 14, 22, 0)));
        // Mid-week is open.
        assert!(cal.is_open_at(at(2024, 6, 12, 3, 0)));
    }

    /// `fx_utc` is the gap detector's fixed calendar.
    #[test]
    fn fx_utc_matches_the_detector_calendar() {
        let cal = MarketCalendar::preset("fx_utc").unwrap();
        let fx_major = tradedesk_data::calendar::Calendar::fx_major();
        let mut ts = at(2024, 12, 20, 0, 0);
        let end = at(2025, 1, 8, 0, 0);
        while ts < end {
            assert_eq!(cal.is_open_at(ts), fx_major.is_open_at(ts), "at {ts}");
            ts += chrono::Duration::minutes(7);
        }
    }

    /// `fx_new_york` closes the FX trading day of 25 December: 17:00 New York on
    /// 24 December (22:00 UTC) to 17:00 New York on 25 December.
    #[test]
    fn closed_every_year_closes_the_fx_trading_day() {
        let cal = MarketCalendar::preset("fx_new_york").unwrap();
        // 2024-12-25 is a Wednesday.
        assert!(cal.is_open_at(at(2024, 12, 24, 21, 59)));
        assert!(!cal.is_open_at(at(2024, 12, 24, 22, 0)));
        assert!(!cal.is_open_at(at(2024, 12, 25, 21, 59)));
        assert!(cal.is_open_at(at(2024, 12, 25, 22, 0)));
    }

    /// With the default `trading_day_start` a closed day is the local calendar day.
    #[test]
    fn closed_every_year_with_midnight_start_closes_the_local_day() {
        let cal = custom(
            r#"
            time_zone = "America/New_York"
            week_open = "sun 17:00"
            week_close = "fri 17:00"
            closed_every_year = ["12-25"]
            "#,
        )
        .unwrap();
        // New York midnight is 05:00 UTC in December.
        assert!(cal.is_open_at(at(2024, 12, 25, 4, 59)));
        assert!(!cal.is_open_at(at(2024, 12, 25, 5, 0)));
        assert!(!cal.is_open_at(at(2024, 12, 26, 4, 59)));
        assert!(cal.is_open_at(at(2024, 12, 26, 5, 0)));
    }

    #[test]
    fn closed_dates_and_daily_break_across_midnight() {
        let cal = custom(
            r#"
            time_zone = "Europe/Berlin"
            week_open = "mon 01:00"
            week_close = "fri 22:00"
            daily_breaks = [{ start = "23:30", end = "00:30" }]
            closed_dates = ["2024-03-29"]
            "#,
        )
        .unwrap();
        // Berlin is UTC+1 in winter: the break is 22:30-23:30 UTC.
        assert!(cal.is_open_at(at(2024, 1, 16, 22, 29)));
        assert!(!cal.is_open_at(at(2024, 1, 16, 22, 30)));
        assert!(!cal.is_open_at(at(2024, 1, 16, 23, 29)));
        assert!(cal.is_open_at(at(2024, 1, 16, 23, 30)));
        // 2024-03-29 (a Friday) is closed all local day; the Thursday before is open.
        assert!(cal.is_open_at(at(2024, 3, 28, 12, 0)));
        assert!(!cal.is_open_at(at(2024, 3, 29, 12, 0)));
        // The weekend runs Friday 22:00 to Monday 01:00 local.
        assert!(!cal.is_open_at(at(2024, 1, 19, 21, 0)));
        assert!(!cal.is_open_at(at(2024, 1, 21, 23, 59)));
        assert!(cal.is_open_at(at(2024, 1, 22, 0, 0)));
    }

    #[test]
    fn equal_week_open_and_close_never_closes_for_a_weekend() {
        let cal = custom(
            r#"
            time_zone = "UTC"
            week_open = "mon 00:00"
            week_close = "mon 00:00"
            closed_every_year = []
            "#,
        )
        .unwrap();
        assert!(cal.is_open_at(at(2024, 6, 15, 12, 0)));
        assert!(cal.is_open_at(at(2024, 6, 17, 0, 0)));
    }

    #[test]
    fn bad_fields_are_refused_with_the_field_name() {
        let base = |extra: &str| {
            format!(
                "time_zone = \"UTC\"\nweek_open = \"sun 22:00\"\nweek_close = \"fri 22:00\"\n{extra}"
            )
        };
        let cases = [
            ("time_zone", "time_zone = \"Mars/Olympus\"\nweek_open = \"sun 22:00\"\nweek_close = \"fri 22:00\"".to_string()),
            ("week_open", "time_zone = \"UTC\"\nweek_open = \"sunday 22:00\"\nweek_close = \"fri 22:00\"".to_string()),
            ("week_close", "time_zone = \"UTC\"\nweek_open = \"sun 22:00\"\nweek_close = \"fri 25:00\"".to_string()),
            ("daily_breaks", base("daily_breaks = [{ start = \"17:00\", end = \"17:00\" }]")),
            ("closed_every_year", base("closed_every_year = [\"02-30\"]")),
            ("closed_dates", base("closed_dates = [\"2024-13-01\"]")),
            ("trading_day_start", base("trading_day_start = \"5pm\"")),
        ];
        for (field, toml_src) in cases {
            let err = custom(&toml_src).expect_err(field);
            assert!(err.contains(field), "{field}: {err}");
        }
        let err = CalendarSpec::Preset("fx_tokyo".into())
            .resolve()
            .unwrap_err();
        assert!(
            err.contains("fx_tokyo") && err.contains("fx_new_york"),
            "{err}"
        );
    }

    /// A calendar serialises as its full table and reads back equal; a preset echoes its
    /// fields, not its name.
    #[test]
    fn serde_round_trip_through_the_table_form() {
        let cal = custom(
            r#"
            time_zone = "Asia/Tokyo"
            week_open = "mon 07:00"
            week_close = "sat 06:00"
            daily_breaks = [{ start = "15:30", end = "16:30" }]
            closed_every_year = ["01-02"]
            closed_dates = ["2024-05-03"]
            "#,
        )
        .unwrap();
        let json = serde_json::to_value(&cal).unwrap();
        assert_eq!(json["time_zone"], "Asia/Tokyo");
        assert_eq!(json["week_close"], "sat 06:00");
        assert_eq!(json["daily_breaks"][0]["start"], "15:30");
        let back: MarketCalendar = serde_json::from_value(json).unwrap();
        assert_eq!(back, cal);

        let fx = serde_json::to_value(MarketCalendar::default()).unwrap();
        assert_eq!(fx["time_zone"], "America/New_York");
        assert_eq!(fx["week_open"], "sun 17:00");
        assert_eq!(fx["trading_day_start"], "17:00");
        assert_eq!(
            fx["closed_every_year"],
            serde_json::json!(["12-25", "01-01"])
        );
    }
}

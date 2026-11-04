//! Bandwidth schedules (feature 032): the limit by time of day and day of week, timed
//! pause and slow-down overrides, and the checks behind config validation and warnings.
//!
//! Everything here is pure: callers pass the time. Times are the daemon's wall clock
//! (local time), so a window follows daylight saving changes the way a clock does.

use crate::config::DownloadConfig;
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDateTime, Timelike, Utc};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Most schedule entries a config may have
pub const MAX_ENTRIES: usize = 50;
/// Limits below this get a warning
pub const LOW_LIMIT: u64 = 64 * 1024;
/// Default `[download] slow_limit_bytes_per_sec`: 1 MiB/s
pub const DEFAULT_SLOW_LIMIT: u64 = 1_048_576;
/// Longest pause or slow-down, in minutes
pub const MAX_OVERRIDE_MINUTES: u32 = 1440;

const MINUTES_PER_DAY: u16 = 1440;
const DAYS: [(&str, &str); 7] = [
    ("mon", "monday"),
    ("tue", "tuesday"),
    ("wed", "wednesday"),
    ("thu", "thursday"),
    ("fri", "friday"),
    ("sat", "saturday"),
    ("sun", "sunday"),
];

/// A download limit. Ordered from strictest to loosest, so `min` gives the stricter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Limit {
    /// Nothing downloads
    Paused,
    /// Bytes per second, for all downloads together
    Rate(u64),
    /// No limit
    Unlimited,
}

impl Limit {
    /// `None` is unlimited, as in `bandwidth_limit_bytes_per_sec`
    pub fn from_option(bytes_per_sec: Option<u64>) -> Self {
        bytes_per_sec.map_or(Limit::Unlimited, Limit::Rate)
    }

    /// The number of bytes per second: `None` unlimited, `Some(0)` paused
    pub fn bytes_per_sec(self) -> Option<u64> {
        match self {
            Limit::Paused => Some(0),
            Limit::Rate(n) => Some(n),
            Limit::Unlimited => None,
        }
    }

    /// For people: "Unlimited", "Paused", "5.0 MB/s" (MiB-based, like the web UI)
    pub fn describe(self) -> String {
        match self {
            Limit::Paused => "Paused".to_string(),
            Limit::Unlimited => "Unlimited".to_string(),
            Limit::Rate(n) => rate(n),
        }
    }
}

/// Bytes per second for people, MiB-based: "5.0 MB/s", "512 KB/s"
pub fn rate(n: u64) -> String {
    const MIB: f64 = 1_048_576.0;
    if n as f64 >= MIB {
        format!("{:.1} MB/s", n as f64 / MIB)
    } else if n >= 1024 {
        format!("{} KB/s", n / 1024)
    } else {
        format!("{n} B/s")
    }
}

impl Serialize for Limit {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Limit::Paused => s.serialize_str("paused"),
            Limit::Unlimited => s.serialize_str("unlimited"),
            Limit::Rate(n) => s.serialize_u64(*n),
        }
    }
}

impl<'de> Deserialize<'de> for Limit {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct LimitVisitor;
        impl Visitor<'_> for LimitVisitor {
            type Value = Limit;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(r#""unlimited", "paused" or bytes per second"#)
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Limit, E> {
                match v.trim().to_ascii_lowercase().as_str() {
                    "unlimited" => Ok(Limit::Unlimited),
                    "paused" => Ok(Limit::Paused),
                    _ => Err(E::invalid_value(de::Unexpected::Str(v), &self)),
                }
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Limit, E> {
                Ok(Limit::Rate(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Limit, E> {
                u64::try_from(v)
                    .map(Limit::Rate)
                    .map_err(|_| E::invalid_value(de::Unexpected::Signed(v), &self))
            }
        }
        d.deserialize_any(LimitVisitor)
    }
}

/// One `[[download.bandwidth_schedule]]` entry, as written
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleEntry {
    /// `mon-fri`, `sat,sun`, `daily`, `weekdays`, `weekends`
    pub days: String,
    /// `HH:MM`, 00:00 to 23:59
    pub start: String,
    /// `HH:MM`, 00:01 to 24:00; before `start` runs past midnight
    pub end: String,
    /// The limit inside the window
    pub limit: Limit,
}

/// A parsed entry: minutes of the day, Monday first
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Days the window starts on, Monday first
    pub days: [bool; 7],
    /// Start minute, 0..1440
    pub start: u16,
    /// End minute, 1..=1440
    pub end: u16,
    /// The limit inside the window
    pub limit: Limit,
}

impl Window {
    /// Whether minute `m` of weekday `wd` (Monday 0) is inside. A window ending before
    /// it starts runs past midnight; its days name the day it starts on.
    fn contains_at(&self, wd: usize, m: u16) -> bool {
        if self.start < self.end {
            self.days[wd] && (self.start..self.end).contains(&m)
        } else {
            (self.days[wd] && m >= self.start) || (self.days[(wd + 6) % 7] && m < self.end)
        }
    }

    /// Whether the wall-clock time `t` is inside
    pub fn contains(&self, t: NaiveDateTime) -> bool {
        self.contains_at(weekday(t), minute(t))
    }
}

fn weekday(t: NaiveDateTime) -> usize {
    t.weekday().num_days_from_monday() as usize
}

fn minute(t: NaiveDateTime) -> u16 {
    (t.hour() * 60 + t.minute()) as u16
}

fn parse_day(name: &str) -> Option<usize> {
    DAYS.iter()
        .position(|(short, long)| name == *short || name == *long)
}

/// Parse `days`: comma-separated days, ranges (wrapping) and the words
fn parse_days(text: &str) -> Result<[bool; 7], String> {
    let mut days = [false; 7];
    let mut any = false;
    for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let item = item.to_ascii_lowercase();
        any = true;
        match item.as_str() {
            "daily" => days = [true; 7],
            "weekdays" => days[..5].iter_mut().for_each(|d| *d = true),
            "weekends" => days[5..].iter_mut().for_each(|d| *d = true),
            _ => {
                let (from, to) = match item.split_once('-') {
                    Some((a, b)) => (a.trim(), b.trim()),
                    None => (item.as_str(), item.as_str()),
                };
                let unknown = |d: &str| format!("unknown day '{d}'");
                let a = parse_day(from).ok_or_else(|| unknown(from))?;
                let b = parse_day(to).ok_or_else(|| unknown(to))?;
                let mut d = a;
                loop {
                    days[d] = true;
                    if d == b {
                        break;
                    }
                    d = (d + 1) % 7;
                }
            }
        }
    }
    if !any {
        return Err("expected at least one day".to_string());
    }
    Ok(days)
}

/// Parse `H:MM` or `HH:MM` to minutes: a start is 00:00 to 23:59, an end 00:01 to 24:00
fn parse_time(text: &str, end: bool) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || m.len() != 2 {
        return None;
    }
    let h: u16 = h.parse().ok()?;
    let m: u16 = m.parse().ok()?;
    if m > 59 {
        return None;
    }
    let total = h * 60 + m;
    let (min, max) = if end {
        (1, MINUTES_PER_DAY)
    } else {
        (0, MINUTES_PER_DAY - 1)
    };
    (min..=max).contains(&total).then_some(total)
}

fn hhmm(m: u16) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

impl ScheduleEntry {
    /// Parse strictly; the error names the field
    pub fn parse(&self) -> Result<Window, String> {
        let days = parse_days(&self.days).map_err(|e| format!("days '{}': {e}", self.days))?;
        let start = parse_time(&self.start, false).ok_or_else(|| {
            format!(
                "start '{}': expected HH:MM between 00:00 and 23:59",
                self.start
            )
        })?;
        let end = parse_time(&self.end, true)
            .ok_or_else(|| format!("end '{}': expected HH:MM between 00:01 and 24:00", self.end))?;
        if start == end {
            return Err(format!(
                "start and end are both {}; use 00:00 to 24:00 for a whole day",
                hhmm(start)
            ));
        }
        if self.limit == Limit::Rate(0) {
            return Err(r#"limit 0: write "paused" to stop downloads"#.to_string());
        }
        Ok(Window {
            days,
            start,
            end,
            limit: self.limit,
        })
    }
}

/// Strict checks for `Config::validate`: every entry, the entry count and the limits
pub fn validate(download: &DownloadConfig) -> Result<(), String> {
    let entries = &download.bandwidth_schedule;
    if entries.len() > MAX_ENTRIES {
        return Err(format!(
            "download.bandwidth_schedule: at most {MAX_ENTRIES} entries"
        ));
    }
    for (i, entry) in entries.iter().enumerate() {
        entry
            .parse()
            .map_err(|e| format!("download.bandwidth_schedule entry {}: {e}", i + 1))?;
    }
    if download.bandwidth_limit_bytes_per_sec == Some(0) {
        return Err(
            "download.bandwidth_limit_bytes_per_sec must be at least 1; leave it out for unlimited"
                .to_string(),
        );
    }
    if download.slow_limit_bytes_per_sec == 0 {
        return Err("download.slow_limit_bytes_per_sec must be at least 1".to_string());
    }
    Ok(())
}

/// What a pause or slow-down does
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverrideMode {
    /// Nothing downloads
    Pause,
    /// Downloads use the slow limit, or the schedule's if lower
    Slow,
}

/// A pause or slow-down until a time; stored so a restart keeps it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    /// Pause or slow
    pub mode: OverrideMode,
    /// When it ends
    pub until: DateTime<Utc>,
}

impl Override {
    /// An override of `minutes` (1 to [`MAX_OVERRIDE_MINUTES`]) from `now`
    pub fn for_minutes(
        mode: OverrideMode,
        minutes: u32,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        if !(1..=MAX_OVERRIDE_MINUTES).contains(&minutes) {
            return Err(format!(
                "minutes {minutes}: expected 1 to {MAX_OVERRIDE_MINUTES}"
            ));
        }
        Ok(Self {
            mode,
            until: now + Duration::minutes(i64::from(minutes)),
        })
    }

    /// Still in force at `now`
    pub fn active(&self, now: DateTime<Utc>) -> bool {
        now < self.until
    }
}

/// Where the limit in force comes from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// `bandwidth_limit_bytes_per_sec`, outside every entry
    Default,
    /// A schedule entry
    Schedule,
    /// A pause
    Pause,
    /// A slow-down
    Slow,
}

impl Source {
    /// For people: "default limit", "schedule entry 2", "pause", "slow-down"
    pub fn describe(self, entry: Option<usize>) -> String {
        match self {
            Source::Default => "default limit".to_string(),
            Source::Schedule => format!("schedule entry {}", entry.unwrap_or(0)),
            Source::Pause => "pause".to_string(),
            Source::Slow => "slow-down".to_string(),
        }
    }
}

/// The limit at some time and where it comes from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Current {
    /// The limit
    pub limit: Limit,
    /// Where it comes from
    pub source: Source,
    /// The 1-based schedule entry, when `source` is `schedule`
    pub entry: Option<usize>,
}

/// The next time the limit changes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// Daemon wall clock
    pub at: NaiveDateTime,
    /// The limit from then
    pub current: Current,
}

/// The default limit, the parsed entries and the slow limit
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Outside every entry
    pub default: Limit,
    /// Parsed entries with their 1-based number; invalid ones are left out
    pub windows: Vec<(usize, Window)>,
    /// The slow-down limit
    pub slow: u64,
}

impl Default for Policy {
    fn default() -> Self {
        Self::fixed(None)
    }
}

/// Everything the Status page, `GET /bandwidth` and the doctor show
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// The limit now
    pub limit: Limit,
    /// Its number: `None` unlimited, `0` paused
    pub bytes_per_sec: Option<u64>,
    /// Where it comes from
    pub source: Source,
    /// The 1-based schedule entry, when `source` is `schedule`
    pub entry: Option<usize>,
    /// The pause or slow-down in force
    #[serde(rename = "override")]
    pub override_: Option<Override>,
    /// The next change, if any within a week
    pub next_change: Option<NextChange>,
    /// Daemon wall clock, `YYYY-MM-DDTHH:MM:SS`
    pub local_time: String,
    /// The daemon's UTC offset, `+01:00`
    pub utc_offset: String,
    /// The slow-down limit
    pub slow_limit_bytes_per_sec: u64,
}

/// The next change as the API sends it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NextChange {
    /// Daemon wall clock, `YYYY-MM-DDTHH:MM`
    pub at: String,
    /// The limit from then
    pub limit: Limit,
    /// Where it comes from
    pub source: Source,
    /// The 1-based schedule entry, when `source` is `schedule`
    pub entry: Option<usize>,
}

impl Policy {
    /// One limit at all times
    pub fn fixed(bytes_per_sec: Option<u64>) -> Self {
        Self {
            default: Limit::from_option(bytes_per_sec),
            windows: Vec::new(),
            slow: DEFAULT_SLOW_LIMIT,
        }
    }

    /// From the parts of `[download]`; invalid entries are skipped (validation reports
    /// them) and keep their numbers
    pub fn new(default: Option<u64>, entries: &[ScheduleEntry], slow: u64) -> Self {
        Self {
            default: Limit::from_option(default),
            windows: entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.parse().ok().map(|w| (i + 1, w)))
                .collect(),
            slow: slow.max(1),
        }
    }

    /// From `[download]`
    pub fn from_config(download: &DownloadConfig) -> Self {
        Self::new(
            download.bandwidth_limit_bytes_per_sec,
            &download.bandwidth_schedule,
            download.slow_limit_bytes_per_sec,
        )
    }

    fn at_minute(&self, wd: usize, m: u16) -> Current {
        match self.windows.iter().find(|(_, w)| w.contains_at(wd, m)) {
            Some((n, w)) => Current {
                limit: w.limit,
                source: Source::Schedule,
                entry: Some(*n),
            },
            None => Current {
                limit: self.default,
                source: Source::Default,
                entry: None,
            },
        }
    }

    /// The schedule alone at wall-clock time `local`
    pub fn scheduled(&self, local: NaiveDateTime) -> Current {
        self.at_minute(weekday(local), minute(local))
    }

    /// The limit at wall-clock `local` (which is `now`), with `ov` if in force
    pub fn at(&self, local: NaiveDateTime, now: DateTime<Utc>, ov: Option<&Override>) -> Current {
        let scheduled = self.scheduled(local);
        match ov.filter(|o| o.active(now)).map(|o| o.mode) {
            Some(OverrideMode::Pause) => Current {
                limit: Limit::Paused,
                source: Source::Pause,
                entry: None,
            },
            Some(OverrideMode::Slow) if Limit::Rate(self.slow) < scheduled.limit => Current {
                limit: Limit::Rate(self.slow),
                source: Source::Slow,
                entry: None,
            },
            _ => scheduled,
        }
    }

    /// The limit at `now`, in the daemon's zone
    pub fn current(&self, now: DateTime<FixedOffset>, ov: Option<&Override>) -> Current {
        self.at(now.naive_local(), now.with_timezone(&Utc), ov)
    }

    /// The first time after `local` within 8 days where the limit differs from now
    pub fn next_change(
        &self,
        local: NaiveDateTime,
        now: DateTime<Utc>,
        ov: Option<&Override>,
    ) -> Option<Change> {
        let current = self.at(local, now, ov);
        let midnight = local.date().and_hms_opt(0, 0, 0)?;
        let mut candidates: Vec<NaiveDateTime> = (0..=8)
            .flat_map(|day| {
                self.windows.iter().flat_map(move |(_, w)| {
                    [w.start, w.end]
                        .map(|m| midnight + Duration::days(day) + Duration::minutes(i64::from(m)))
                })
            })
            .collect();
        if let Some(o) = ov.filter(|o| o.active(now)) {
            candidates.push(local + (o.until - now));
        }
        candidates.retain(|t| *t > local);
        candidates.sort();
        candidates.dedup();
        candidates.into_iter().find_map(|t| {
            let at = self.at(t, now + (t - local), ov);
            (at.limit != current.limit).then_some(Change { at: t, current: at })
        })
    }

    /// Everything for the API and the Status page at `now`
    pub fn status(&self, now: DateTime<FixedOffset>, ov: Option<&Override>) -> Status {
        let local = now.naive_local();
        let utc = now.with_timezone(&Utc);
        let ov = ov.filter(|o| o.active(utc));
        let current = self.at(local, utc, ov);
        Status {
            limit: current.limit,
            bytes_per_sec: current.limit.bytes_per_sec(),
            source: current.source,
            entry: current.entry,
            override_: ov.copied(),
            next_change: self.next_change(local, utc, ov).map(|c| NextChange {
                at: c.at.format("%Y-%m-%dT%H:%M").to_string(),
                limit: c.current.limit,
                source: c.current.source,
                entry: c.current.entry,
            }),
            local_time: local.format("%Y-%m-%dT%H:%M:%S").to_string(),
            utc_offset: now.format("%:z").to_string(),
            slow_limit_bytes_per_sec: self.slow,
        }
    }
}

/// A schedule that is valid but probably not what was meant
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Stable id: `bandwidth_overlap`, `bandwidth_paused_week`, `bandwidth_low`
    pub id: &'static str,
    /// The config key it is about
    pub key: &'static str,
    /// For people
    pub message: String,
}

/// Overlapping entries, a week with no unpaused minute, and limits below [`LOW_LIMIT`]
pub fn warnings(download: &DownloadConfig) -> Vec<Warning> {
    let policy = Policy::from_config(download);
    let mut found = Vec::new();
    let mut pairs = std::collections::BTreeSet::new();
    let mut any_unpaused = false;
    for wd in 0..7 {
        for m in 0..MINUTES_PER_DAY {
            let inside: Vec<usize> = policy
                .windows
                .iter()
                .filter(|(_, w)| w.contains_at(wd, m))
                .map(|(n, _)| *n)
                .collect();
            for (i, a) in inside.iter().enumerate() {
                for b in &inside[i + 1..] {
                    pairs.insert((*a, *b));
                }
            }
            any_unpaused |= policy.at_minute(wd, m).limit != Limit::Paused;
        }
    }
    for (a, b) in pairs {
        found.push(Warning {
            id: "bandwidth_overlap",
            key: "download.bandwidth_schedule",
            message: format!(
                "Bandwidth schedule entries {a} and {b} overlap; entry {a} wins where they do."
            ),
        });
    }
    if !any_unpaused {
        found.push(Warning {
            id: "bandwidth_paused_week",
            key: "download.bandwidth_schedule",
            message: "The bandwidth schedule pauses downloads all week; nothing will download."
                .to_string(),
        });
    }
    let low = |n: u64| n < LOW_LIMIT;
    if let Limit::Rate(n) = policy.default {
        if low(n) {
            found.push(low_warning(
                "download.bandwidth_limit_bytes_per_sec",
                format!("The default bandwidth limit, {}", rate(n)),
            ));
        }
    }
    if low(policy.slow) {
        found.push(low_warning(
            "download.slow_limit_bytes_per_sec",
            format!("The slow-down limit, {}", rate(policy.slow)),
        ));
    }
    for (n, w) in &policy.windows {
        if let Limit::Rate(r) = w.limit {
            if low(r) {
                found.push(low_warning(
                    "download.bandwidth_schedule",
                    format!("Bandwidth schedule entry {n}'s limit, {}", rate(r)),
                ));
            }
        }
    }
    found
}

fn low_warning(key: &'static str, what: String) -> Warning {
    Warning {
        id: "bandwidth_low",
        key,
        message: format!(
            "{what}, is below 64 KB/s: downloads crawl and Put.io may drop the connection."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn entry(days: &str, start: &str, end: &str, limit: Limit) -> ScheduleEntry {
        ScheduleEntry {
            days: days.into(),
            start: start.into(),
            end: end.into(),
            limit,
        }
    }

    /// 2026-11-02 is a Monday
    fn at(day: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn utc() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn download(entries: Vec<ScheduleEntry>) -> DownloadConfig {
        DownloadConfig {
            bandwidth_limit_bytes_per_sec: Some(5 * 1_048_576),
            bandwidth_schedule: entries,
            ..DownloadConfig::default()
        }
    }

    #[test]
    fn limit_reads_and_writes_words_and_numbers() {
        #[derive(Deserialize, Serialize)]
        struct T {
            limit: Limit,
        }
        for (text, limit) in [
            ("limit = \"unlimited\"", Limit::Unlimited),
            ("limit = \"Paused\"", Limit::Paused),
            ("limit = 2048", Limit::Rate(2048)),
        ] {
            let t: T = toml::from_str(text).unwrap();
            assert_eq!(t.limit, limit);
        }
        assert!(toml::from_str::<T>("limit = \"fast\"").is_err());
        assert!(toml::from_str::<T>("limit = -1").is_err());
        assert_eq!(
            serde_json::to_string(&T {
                limit: Limit::Paused
            })
            .unwrap(),
            r#"{"limit":"paused"}"#
        );
        assert_eq!(
            serde_json::to_string(&T {
                limit: Limit::Rate(7)
            })
            .unwrap(),
            r#"{"limit":7}"#
        );
        assert!(Limit::Paused < Limit::Rate(1));
        assert!(Limit::Rate(9) < Limit::Unlimited);
    }

    #[test]
    fn day_forms() {
        let days = |s| parse_days(s).unwrap();
        assert_eq!(days("daily"), [true; 7]);
        assert_eq!(
            days("Weekdays"),
            [true, true, true, true, true, false, false]
        );
        assert_eq!(
            days("weekends"),
            [false, false, false, false, false, true, true]
        );
        assert_eq!(
            days("mon-wed"),
            [true, true, true, false, false, false, false]
        );
        assert_eq!(
            days("fri-mon"),
            [true, false, false, false, true, true, true]
        );
        assert_eq!(
            days("Monday, fri"),
            [true, false, false, false, true, false, false]
        );
        assert_eq!(parse_days("mon-fry").unwrap_err(), "unknown day 'fry'");
        assert_eq!(parse_days(" , ").unwrap_err(), "expected at least one day");
    }

    #[test]
    fn entry_errors_name_the_field() {
        let err = |e: ScheduleEntry| e.parse().unwrap_err();
        assert_eq!(
            err(entry("daily", "25:00", "07:00", Limit::Unlimited)),
            "start '25:00': expected HH:MM between 00:00 and 23:59"
        );
        assert_eq!(
            err(entry("daily", "24:00", "07:00", Limit::Unlimited)),
            "start '24:00': expected HH:MM between 00:00 and 23:59"
        );
        assert_eq!(
            err(entry("daily", "01:00", "7", Limit::Unlimited)),
            "end '7': expected HH:MM between 00:01 and 24:00"
        );
        assert_eq!(
            err(entry("daily", "01:00", "07:60", Limit::Unlimited)),
            "end '07:60': expected HH:MM between 00:01 and 24:00"
        );
        assert_eq!(
            err(entry("", "01:00", "07:00", Limit::Unlimited)),
            "days '': expected at least one day"
        );
        assert_eq!(
            err(entry("daily", "08:00", "08:00", Limit::Unlimited)),
            "start and end are both 08:00; use 00:00 to 24:00 for a whole day"
        );
        assert_eq!(
            err(entry("daily", "01:00", "07:00", Limit::Rate(0))),
            r#"limit 0: write "paused" to stop downloads"#
        );
        assert_eq!(
            err(entry("daily", "22:00", "00:00", Limit::Unlimited)),
            "end '00:00': expected HH:MM between 00:01 and 24:00"
        );
        assert!(entry("daily", "0:00", "24:00", Limit::Paused)
            .parse()
            .is_ok());
    }

    #[test]
    fn validate_numbers_the_entry() {
        let mut d = download(vec![
            entry("daily", "01:00", "07:00", Limit::Unlimited),
            entry("mon", "25:00", "07:00", Limit::Unlimited),
        ]);
        assert_eq!(
            validate(&d).unwrap_err(),
            "download.bandwidth_schedule entry 2: start '25:00': expected HH:MM between 00:00 and 23:59"
        );
        d.bandwidth_schedule.truncate(1);
        assert!(validate(&d).is_ok());
        d.bandwidth_schedule = vec![d.bandwidth_schedule[0].clone(); 51];
        assert_eq!(
            validate(&d).unwrap_err(),
            "download.bandwidth_schedule: at most 50 entries"
        );
        let mut d = download(vec![]);
        d.bandwidth_limit_bytes_per_sec = Some(0);
        assert!(validate(&d).unwrap_err().contains("at least 1"));
        d.bandwidth_limit_bytes_per_sec = None;
        d.slow_limit_bytes_per_sec = 0;
        assert!(validate(&d)
            .unwrap_err()
            .contains("slow_limit_bytes_per_sec"));
    }

    #[test]
    fn roadmap_example() {
        let d = download(vec![entry("daily", "01:00", "07:00", Limit::Unlimited)]);
        let p = Policy::from_config(&d);
        assert_eq!(p.at(at(3, 3, 0), utc(), None).limit, Limit::Unlimited);
        assert_eq!(p.at(at(3, 3, 0), utc(), None).entry, Some(1));
        assert_eq!(
            p.at(at(3, 7, 0), utc(), None).limit,
            Limit::Rate(5 * 1_048_576)
        );
        assert_eq!(p.at(at(3, 7, 0), utc(), None).source, Source::Default);
        assert_eq!(p.at(at(3, 0, 59), utc(), None).source, Source::Default);
        let next = p.next_change(at(3, 8, 0), utc(), None).unwrap();
        assert_eq!(next.at, at(4, 1, 0));
        assert_eq!(next.current.limit, Limit::Unlimited);
    }

    #[test]
    fn past_midnight_belongs_to_the_start_day() {
        let d = download(vec![entry("fri", "22:00", "06:00", Limit::Rate(2048))]);
        let p = Policy::from_config(&d);
        // Friday 2026-11-06, Saturday the 7th
        assert_eq!(p.at(at(6, 23, 0), utc(), None).limit, Limit::Rate(2048));
        assert_eq!(p.at(at(7, 3, 0), utc(), None).limit, Limit::Rate(2048));
        assert_eq!(p.at(at(7, 6, 0), utc(), None).source, Source::Default);
        assert_eq!(p.at(at(7, 23, 0), utc(), None).source, Source::Default);
        assert_eq!(p.at(at(6, 3, 0), utc(), None).source, Source::Default);
    }

    #[test]
    fn whole_day_and_midnight_ends() {
        let d = download(vec![
            entry("sun", "00:00", "24:00", Limit::Paused),
            entry("sat", "22:00", "24:00", Limit::Unlimited),
        ]);
        let p = Policy::from_config(&d);
        assert_eq!(p.at(at(8, 23, 59), utc(), None).limit, Limit::Paused);
        assert_eq!(p.at(at(8, 0, 0), utc(), None).limit, Limit::Paused);
        assert_eq!(p.at(at(9, 0, 0), utc(), None).source, Source::Default);
        assert_eq!(p.at(at(7, 23, 30), utc(), None).limit, Limit::Unlimited);
    }

    #[test]
    fn first_entry_wins() {
        let d = download(vec![
            entry("mon-fri", "18:00", "23:00", Limit::Paused),
            entry("daily", "12:00", "20:00", Limit::Unlimited),
        ]);
        let p = Policy::from_config(&d);
        assert_eq!(p.at(at(3, 19, 0), utc(), None).limit, Limit::Paused);
        assert_eq!(p.at(at(3, 13, 0), utc(), None).entry, Some(2));
        assert_eq!(p.at(at(7, 19, 0), utc(), None).entry, Some(2));
    }

    #[test]
    fn overrides() {
        let d = download(vec![entry("daily", "01:00", "07:00", Limit::Rate(1000))]);
        let p = Policy::from_config(&d);
        let pause = Override::for_minutes(OverrideMode::Pause, 60, utc()).unwrap();
        let now = p.at(at(3, 12, 0), utc(), Some(&pause));
        assert_eq!((now.limit, now.source), (Limit::Paused, Source::Pause));
        let later = utc() + Duration::minutes(60);
        assert_eq!(
            p.at(at(3, 13, 0), later, Some(&pause)).source,
            Source::Default
        );
        let next = p.next_change(at(3, 12, 0), utc(), Some(&pause)).unwrap();
        assert_eq!(next.at, at(3, 13, 0));
        assert_eq!(next.current.source, Source::Default);

        // Slow never raises the limit
        let slow = Override::for_minutes(OverrideMode::Slow, 60, utc()).unwrap();
        let day = p.at(at(3, 12, 0), utc(), Some(&slow));
        assert_eq!(
            (day.limit, day.source),
            (Limit::Rate(DEFAULT_SLOW_LIMIT), Source::Slow)
        );
        let night = p.at(at(3, 2, 0), utc(), Some(&slow));
        assert_eq!(
            (night.limit, night.source),
            (Limit::Rate(1000), Source::Schedule)
        );

        assert!(Override::for_minutes(OverrideMode::Pause, 0, utc()).is_err());
        assert!(Override::for_minutes(OverrideMode::Pause, 1441, utc()).is_err());
    }

    #[test]
    fn status_shows_offset_and_next_change() {
        let d = download(vec![entry("daily", "01:00", "07:00", Limit::Unlimited)]);
        let p = Policy::from_config(&d);
        let now = DateTime::parse_from_rfc3339("2026-11-04T08:00:12+01:00").unwrap();
        let s = p.status(now, None);
        assert_eq!(s.local_time, "2026-11-04T08:00:12");
        assert_eq!(s.utc_offset, "+01:00");
        assert_eq!(s.bytes_per_sec, Some(5 * 1_048_576));
        let next = s.next_change.unwrap();
        assert_eq!(next.at, "2026-11-05T01:00");
        assert_eq!(next.limit, Limit::Unlimited);
        let json = serde_json::to_value(p.status(now, None)).unwrap();
        assert_eq!(json["source"], "default");
        assert!(json["override"].is_null());
        assert!(Policy::fixed(None).status(now, None).next_change.is_none());
    }

    #[test]
    fn warnings_find_overlap_paused_week_and_low_limits() {
        let d = download(vec![
            entry("mon-fri", "18:00", "23:00", Limit::Paused),
            entry("daily", "22:00", "02:00", Limit::Rate(1000)),
            entry("sat", "10:00", "11:00", Limit::Unlimited),
        ]);
        let found = warnings(&d);
        let ids: Vec<_> = found.iter().map(|w| w.id).collect();
        assert_eq!(ids, ["bandwidth_overlap", "bandwidth_low"], "{found:?}");
        assert!(found[0].message.contains("entries 1 and 2 overlap"));
        assert!(found[1].message.contains("entry 2"));

        let d = DownloadConfig {
            bandwidth_schedule: vec![entry("daily", "00:00", "24:00", Limit::Paused)],
            ..DownloadConfig::default()
        };
        assert_eq!(warnings(&d)[0].id, "bandwidth_paused_week");
        assert!(warnings(&DownloadConfig::default()).is_empty());
    }
}

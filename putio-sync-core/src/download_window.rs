//! Download windows and the free-space pause (feature 064).
//!
//! A window is hours of the week downloads may run in, written like a bandwidth schedule
//! entry without a limit. With no windows downloads always run; with some, they run only
//! inside one. The free-space pause stops every download below one level of free space
//! and starts them again at a higher one, so it does not flap around a single number.
//!
//! Everything here is pure: callers pass the time and the free space. Times are the
//! daemon's wall clock (local time), so a window follows daylight saving changes the way
//! a clock does: an hour the clock skips is never inside, an hour it repeats is inside
//! both times.

use crate::bandwidth::{self, Limit, ScheduleEntry, Window};
use crate::config::DownloadConfig;
use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// Most windows a config may have
pub const MAX_ENTRIES: usize = 50;

/// One `[[download.windows]]` entry, as written
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowEntry {
    /// `mon-fri`, `sat,sun`, `daily`, `weekdays`, `weekends`
    pub days: String,
    /// `HH:MM`, 00:00 to 23:59
    pub start: String,
    /// `HH:MM`, 00:01 to 24:00; before `start` runs past midnight
    pub end: String,
}

impl WindowEntry {
    /// Parse strictly; the error names the field
    pub fn parse(&self) -> Result<Window, String> {
        ScheduleEntry {
            days: self.days.clone(),
            start: self.start.clone(),
            end: self.end.clone(),
            limit: Limit::Unlimited,
        }
        .parse()
    }
}

/// Whether downloads may run at a moment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Inside a window, or no windows are set
    Open,
    /// Outside every window
    Closed {
        /// The wall-clock time the next window starts, if one does within 8 days
        reopens: Option<NaiveDateTime>,
    },
}

/// The parsed windows of a config
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Windows(Vec<Window>);

impl Windows {
    /// The windows of `entries`. An entry that does not parse is left out; a config
    /// that passed `validate` has none.
    pub fn from_entries(entries: &[WindowEntry]) -> Self {
        Self(entries.iter().filter_map(|e| e.parse().ok()).collect())
    }

    /// No windows: downloads always run
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether downloads may run at wall-clock time `local`
    pub fn open_at(&self, local: NaiveDateTime) -> bool {
        let (wd, m) = (bandwidth::weekday(local), bandwidth::minute(local));
        self.0.is_empty() || self.0.iter().any(|w| w.contains_at(wd, m))
    }

    /// The first wall-clock time after `local`, within 8 days, a window starts at
    pub fn next_open(&self, local: NaiveDateTime) -> Option<NaiveDateTime> {
        let midnight = local.date().and_hms_opt(0, 0, 0)?;
        let mut starts: Vec<NaiveDateTime> = (0..=8)
            .flat_map(|day| {
                self.0.iter().map(move |w| {
                    midnight + Duration::days(day) + Duration::minutes(i64::from(w.start))
                })
            })
            .filter(|t| *t > local)
            .collect();
        starts.sort();
        starts.into_iter().find(|t| self.open_at(*t))
    }

    /// Open or closed at wall-clock time `local`, and when it opens again
    pub fn state(&self, local: NaiveDateTime) -> State {
        if self.open_at(local) {
            State::Open
        } else {
            State::Closed {
                reopens: self.next_open(local),
            }
        }
    }
}

/// The free-space pause after a reading of `free` bytes (`None`: it can't be read):
/// on below `pause`, off at `resume` or more, unchanged in between and when unreadable.
/// `pause` 0 is off.
pub fn space_paused(was_paused: bool, free: Option<u64>, pause: u64, resume: u64) -> bool {
    if pause == 0 {
        return false;
    }
    match free {
        Some(free) if free < pause => true,
        Some(free) if free >= resume.max(pause) => false,
        _ => was_paused,
    }
}

/// Strict checks for `Config::validate`: every window, their count, and the two levels
/// of the free-space pause
pub fn validate(download: &DownloadConfig) -> Result<(), String> {
    if download.windows.len() > MAX_ENTRIES {
        return Err(format!("download.windows: at most {MAX_ENTRIES} entries"));
    }
    for (i, entry) in download.windows.iter().enumerate() {
        entry
            .parse()
            .map_err(|e| format!("download.windows entry {}: {e}", i + 1))?;
    }
    let (pause, resume) = (download.pause_free_space_mb, download.resume_free_space_mb);
    if pause > 0 && resume > 0 && resume <= pause {
        return Err(format!(
            "download.resume_free_space_mb ({resume}) must be above download.pause_free_space_mb ({pause}); 0 means {} MB above it",
            DownloadConfig::RESUME_MARGIN_MB
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate, TimeZone};

    fn entry(days: &str, start: &str, end: &str) -> WindowEntry {
        WindowEntry {
            days: days.into(),
            start: start.into(),
            end: end.into(),
        }
    }

    fn windows(entries: &[WindowEntry]) -> Windows {
        Windows::from_entries(entries)
    }

    /// 2026-11-02 is a Monday
    fn at(day: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    #[test]
    fn no_windows_is_always_open() {
        let w = windows(&[]);
        assert!(w.is_empty());
        assert_eq!(w.state(at(2, 12, 0)), State::Open);
        assert_eq!(w.next_open(at(2, 12, 0)), None);
    }

    #[test]
    fn a_window_is_open_from_its_start_up_to_its_end() {
        let w = windows(&[entry("mon-fri", "01:00", "07:00")]);
        assert!(!w.open_at(at(2, 0, 59)));
        assert!(w.open_at(at(2, 1, 0)));
        assert!(w.open_at(at(2, 6, 59)));
        assert!(!w.open_at(at(2, 7, 0)));
        // Saturday the 7th is not a weekday
        assert!(!w.open_at(at(7, 3, 0)));
    }

    #[test]
    fn closed_names_the_next_start() {
        let w = windows(&[entry("mon-fri", "01:00", "07:00")]);
        assert_eq!(
            w.state(at(2, 7, 0)),
            State::Closed {
                reopens: Some(at(3, 1, 0))
            }
        );
        // Friday evening: the weekend is skipped
        assert_eq!(w.next_open(at(6, 20, 0)), Some(at(9, 1, 0)));
        // Just before today's start
        assert_eq!(w.next_open(at(2, 0, 30)), Some(at(2, 1, 0)));
    }

    #[test]
    fn a_window_past_midnight_belongs_to_the_day_it_starts_on() {
        let w = windows(&[entry("fri", "22:00", "06:00")]);
        assert!(w.open_at(at(6, 23, 0)), "Friday night");
        assert!(w.open_at(at(7, 5, 59)), "Saturday morning, still Friday's");
        assert!(!w.open_at(at(7, 6, 0)));
        assert!(!w.open_at(at(6, 5, 0)), "Friday morning is Thursday's");
        assert_eq!(w.next_open(at(7, 6, 0)), Some(at(13, 22, 0)));
    }

    #[test]
    fn several_windows_add_up() {
        let w = windows(&[
            entry("daily", "01:00", "07:00"),
            entry("sat,sun", "00:00", "24:00"),
        ]);
        assert!(w.open_at(at(7, 15, 0)), "all of Saturday");
        assert!(!w.open_at(at(2, 15, 0)));
        assert_eq!(w.next_open(at(6, 15, 0)), Some(at(7, 0, 0)));
    }

    /// Windows follow the daemon's wall clock whatever its zone: the same instant is
    /// inside in one zone and outside in another
    #[test]
    fn windows_use_the_wall_clock_of_the_zone() {
        let w = windows(&[entry("daily", "01:00", "07:00")]);
        let utc = FixedOffset::east_opt(0).unwrap();
        let instant = utc.with_ymd_and_hms(2026, 11, 2, 23, 30, 0).unwrap();
        let tokyo = FixedOffset::east_opt(9 * 3600).unwrap();
        let new_york = FixedOffset::west_opt(5 * 3600).unwrap();
        assert!(!w.open_at(instant.naive_local()));
        // 08:30 on the 3rd in Tokyo, 18:30 on the 2nd in New York
        assert!(!w.open_at(instant.with_timezone(&tokyo).naive_local()));
        assert!(!w.open_at(instant.with_timezone(&new_york).naive_local()));
        let later = utc.with_ymd_and_hms(2026, 11, 2, 17, 30, 0).unwrap();
        // 02:30 on the 3rd in Tokyo: inside there, outside in UTC
        assert!(w.open_at(later.with_timezone(&tokyo).naive_local()));
        assert!(!w.open_at(later.naive_local()));
    }

    /// Spring forward (Europe, 2026-03-29): the clock goes 01:59 → 03:00, so a window
    /// 02:00–03:00 never opens that night, and one 02:30–04:00 opens when the clock
    /// lands inside it
    #[test]
    fn a_skipped_hour_is_never_inside() {
        let day = |h, m| {
            NaiveDate::from_ymd_opt(2026, 3, 29)
                .unwrap()
                .and_hms_opt(h, m, 0)
                .unwrap()
        };
        let skipped = windows(&[entry("daily", "02:00", "03:00")]);
        assert!(!skipped.open_at(day(1, 59)));
        assert!(
            !skipped.open_at(day(3, 0)),
            "the first minute after the jump"
        );
        // The start it names is a time the clock never shows; the wait ends a day later
        assert_eq!(skipped.next_open(day(1, 59)), Some(day(2, 0)));

        let partly = windows(&[entry("daily", "02:30", "04:00")]);
        assert!(!partly.open_at(day(1, 59)));
        assert!(partly.open_at(day(3, 0)));
    }

    /// Fall back (Europe, 2026-10-25): 02:00–02:59 happens twice and is inside both
    /// times, because only the wall clock is looked at
    #[test]
    fn a_repeated_hour_is_inside_both_times() {
        let w = windows(&[entry("daily", "02:00", "03:00")]);
        let cest = FixedOffset::east_opt(2 * 3600).unwrap();
        let cet = FixedOffset::east_opt(3600).unwrap();
        let first = cest.with_ymd_and_hms(2026, 10, 25, 2, 30, 0).unwrap();
        let second = cet.with_ymd_and_hms(2026, 10, 25, 2, 30, 0).unwrap();
        assert_ne!(first, second);
        assert!(w.open_at(first.naive_local()));
        assert!(w.open_at(second.naive_local()));
    }

    #[test]
    fn the_space_pause_has_two_levels() {
        const GB: u64 = 1 << 30;
        let step = |was, free| space_paused(was, Some(free), 10 * GB, 15 * GB);
        assert!(!step(false, 10 * GB), "at the level is not below it");
        assert!(step(false, 10 * GB - 1));
        assert!(step(true, 12 * GB), "between the levels a pause stays");
        assert!(!step(false, 12 * GB), "and so does no pause");
        assert!(step(true, 15 * GB - 1));
        assert!(!step(true, 15 * GB));
    }

    #[test]
    fn the_space_pause_keeps_its_state_when_unreadable_and_is_off_at_zero() {
        assert!(space_paused(true, None, 10, 20));
        assert!(!space_paused(false, None, 10, 20));
        assert!(!space_paused(true, Some(0), 0, 20));
        // A resume level at or under the pause level acts as the pause level
        assert!(!space_paused(true, Some(10), 10, 5));
    }

    #[test]
    fn validate_names_the_entry_and_the_levels() {
        let mut d = DownloadConfig::default();
        assert_eq!(validate(&d), Ok(()));
        d.windows = vec![
            entry("daily", "01:00", "07:00"),
            entry("someday", "1:00", "2:00"),
        ];
        let e = validate(&d).unwrap_err();
        assert!(
            e.starts_with("download.windows entry 2: days 'someday'"),
            "{e}"
        );
        d.windows = vec![entry("daily", "07:00", "07:00")];
        assert!(validate(&d).unwrap_err().contains("entry 1"));
        d.windows = vec![entry("daily", "00:00", "24:00"); MAX_ENTRIES + 1];
        assert!(validate(&d).unwrap_err().contains("at most 50"));
        d.windows.clear();

        d.pause_free_space_mb = 2048;
        assert_eq!(validate(&d), Ok(()));
        assert_eq!(d.resume_free_space_bytes(), 3072 << 20);
        d.resume_free_space_mb = 2048;
        assert!(validate(&d)
            .unwrap_err()
            .contains("must be above download.pause_free_space_mb"));
        d.resume_free_space_mb = 4096;
        assert_eq!(validate(&d), Ok(()));
        assert_eq!(d.resume_free_space_bytes(), 4096 << 20);
        // A resume level alone is ignored
        d.pause_free_space_mb = 0;
        assert_eq!(validate(&d), Ok(()));
    }
}

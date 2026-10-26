//! The daily empty-folder cleanup
//!
//! A loop checks the local clock every [`TICK`] and runs the cleanup when the configured
//! time of day falls between the previous check and now. Missed runs (the daemon was down)
//! are not caught up, and the settings are read again at every check, so a change from the
//! Settings page applies from the next crossing.

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime};
use putio_sync_core::cleanup::{CleanupError, CleanupTrigger, EmptyFolderCleanup};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// How often the loop looks at the clock
pub const TICK: Duration = Duration::from_secs(30);

/// The date whose run is due when the clock moved from `prev` to `now`: `run_at` on that
/// date lies in `(prev, now]` and that date has not run yet (`last_run`). Times are local
/// and naive, so a DST gap still crosses `run_at`, and a DST overlap that crosses it twice
/// runs once.
pub fn due_date(
    prev: NaiveDateTime,
    now: NaiveDateTime,
    run_at: NaiveTime,
    last_run: Option<NaiveDate>,
) -> Option<NaiveDate> {
    [prev.date(), now.date()].into_iter().find(|&date| {
        let at = date.and_time(run_at);
        prev < at && at <= now && last_run != Some(date)
    })
}

/// Run the cleanup once a day while `[cleanup] enabled`; never returns
pub async fn run_daily(cleanup: Arc<EmptyFolderCleanup>) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The first tick fires at once; starting from now means no catch-up run
    let mut prev = Local::now().naive_local();
    let mut last_run = None;
    loop {
        ticker.tick().await;
        let now = Local::now().naive_local();
        let config = cleanup.config();
        if config.enabled {
            match config.run_time() {
                Ok(run_at) => {
                    if let Some(date) = due_date(prev, now, run_at, last_run) {
                        last_run = Some(date);
                        run_once(&cleanup).await;
                    }
                }
                Err(e) => warn!("Daily empty-folder cleanup skipped: {}", e),
            }
        }
        prev = now;
    }
}

async fn run_once(cleanup: &EmptyFolderCleanup) {
    info!("Daily empty-folder cleanup starting");
    match cleanup.run(CleanupTrigger::Schedule, false).await {
        Ok(_) => {}
        Err(CleanupError::AlreadyRunning) => {
            info!("Daily empty-folder cleanup skipped: a manual run is in progress")
        }
        // Already logged and counted by the service
        Err(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(date: &str, time: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn time(t: &str) -> NaiveTime {
        NaiveTime::parse_from_str(t, "%H:%M").unwrap()
    }

    fn day(d: &str) -> NaiveDate {
        d.parse().unwrap()
    }

    #[test]
    fn due_when_run_time_is_crossed() {
        let due = due_date(
            at("2026-10-26", "02:59:50"),
            at("2026-10-26", "03:00:20"),
            time("03:00"),
            None,
        );
        assert_eq!(due, Some(day("2026-10-26")));
    }

    #[test]
    fn due_exactly_at_run_time_but_not_just_after() {
        let run_at = time("03:00");
        assert_eq!(
            due_date(
                at("2026-10-26", "02:59:30"),
                at("2026-10-26", "03:00:00"),
                run_at,
                None
            ),
            Some(day("2026-10-26"))
        );
        assert_eq!(
            due_date(
                at("2026-10-26", "03:00:00"),
                at("2026-10-26", "03:00:30"),
                run_at,
                None
            ),
            None
        );
    }

    #[test]
    fn due_across_midnight() {
        let due = due_date(
            at("2026-10-26", "23:59:50"),
            at("2026-10-27", "00:00:20"),
            time("00:00"),
            None,
        );
        assert_eq!(due, Some(day("2026-10-27")));

        let due = due_date(
            at("2026-10-26", "23:59:40"),
            at("2026-10-27", "00:00:10"),
            time("23:59"),
            None,
        );
        assert_eq!(due, None);
        let due = due_date(
            at("2026-10-26", "23:58:50"),
            at("2026-10-27", "00:00:10"),
            time("23:59"),
            None,
        );
        assert_eq!(due, Some(day("2026-10-26")));
    }

    #[test]
    fn no_catch_up_after_start() {
        // Started at 10:00: the 03:00 run is not made up
        let due = due_date(
            at("2026-10-26", "10:00:00"),
            at("2026-10-26", "10:00:30"),
            time("03:00"),
            None,
        );
        assert_eq!(due, None);
    }

    #[test]
    fn not_twice_on_one_date() {
        let due = due_date(
            at("2026-10-26", "02:59:50"),
            at("2026-10-26", "03:00:20"),
            time("03:00"),
            Some(day("2026-10-26")),
        );
        assert_eq!(due, None);
    }

    #[test]
    fn dst_gap_still_runs() {
        // Clocks jump from 02:00 to 03:00; 02:30 never shows but is crossed
        let due = due_date(
            at("2026-03-29", "01:59:50"),
            at("2026-03-29", "03:00:20"),
            time("02:30"),
            None,
        );
        assert_eq!(due, Some(day("2026-03-29")));
    }

    #[test]
    fn dst_overlap_runs_once() {
        let run_at = time("02:30");
        let first = due_date(
            at("2026-10-25", "02:29:50"),
            at("2026-10-25", "02:30:20"),
            run_at,
            None,
        );
        assert_eq!(first, Some(day("2026-10-25")));
        // Clocks go back from 03:00 to 02:00, then 02:30 comes round again
        assert_eq!(
            due_date(
                at("2026-10-25", "02:59:50"),
                at("2026-10-25", "02:00:20"),
                run_at,
                first
            ),
            None
        );
        assert_eq!(
            due_date(
                at("2026-10-25", "02:29:50"),
                at("2026-10-25", "02:30:20"),
                run_at,
                first
            ),
            None
        );
    }
}

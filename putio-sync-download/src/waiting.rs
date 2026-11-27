//! Why a row is not downloading (feature 066): one reason per queue row and Active item,
//! as a stable code plus the fields its sentence needs. The web UI turns the code into
//! words and a link to the setting behind it.
//!
//! [`QueueState`] is a snapshot of what `DownloadManager::process_queue` checks before it
//! claims a row, and [`QueueState::for_row`] applies the same checks in the same order,
//! so the reason shown is the one that holds the row back. It is pure, so every rule is
//! tested without a daemon. A feature that adds a new way to hold a row back adds a code
//! in [`code`] and a check here.

use crate::gates::Hold;
use crate::manager::below_free_space_floor;
use putio_sync_core::bandwidth::{self, OverrideMode, Source};
use serde::Serialize;
use std::collections::HashMap;

/// The reason codes. Part of the API: a code is never renamed or reused.
pub mod code {
    /// A timed pause of all downloads is in force (`until`)
    pub const BANDWIDTH_PAUSE: &str = "bandwidth_pause";
    /// A bandwidth schedule entry pauses downloads now (`entry`, `until_local`)
    pub const BANDWIDTH_SCHEDULE: &str = "bandwidth_schedule";
    /// The default bandwidth limit is "paused"
    pub const BANDWIDTH_LIMIT: &str = "bandwidth_limit";
    /// Outside every download window (`until_local`: when the next one starts; feature 064)
    pub const DOWNLOAD_WINDOW: &str = "download_window";
    /// Free space fell below the pause level (`free_bytes`, `min_free_bytes`: the pause
    /// level, `resume_bytes`; feature 064)
    pub const DISK_PAUSE: &str = "disk_pause";
    /// The Pause local downloads switch is on
    pub const DOWNLOADS_PAUSED: &str = "downloads_paused";
    /// Free space is below the floor (`free_bytes`, `min_free_bytes`)
    pub const DISK_SPACE: &str = "disk_space";
    /// The floor is on and the download folder's free space can't be read
    pub const DISK_UNREADABLE: &str = "disk_unreadable";
    /// This file doesn't fit above the floor; smaller ones go first
    pub const FILE_TOO_BIG: &str = "file_too_big";
    /// Every worker is busy (`workers`)
    pub const WAITING_TURN: &str = "waiting_turn";
    /// The row itself is paused
    pub const PAUSED: &str = "paused";
    /// The item was paused while on Put.io, so its download is held
    pub const HELD: &str = "held";
    /// The download failed (`kind`, when this run of the daemon saw it fail)
    pub const FAILED: &str = "failed";
    /// Put.io has not started the transfer yet
    pub const PUTIO_QUEUE: &str = "putio_queue";
    /// Put.io is done; the next sync queues its files (`until`)
    pub const WAITING_FOR_SYNC: &str = "waiting_for_sync";
    /// The transfer was added to wait until its files are picked (feature 070)
    pub const CHOOSE_FILES: &str = "choose_files";
    /// The file was left out by the choice made for its transfer; it stays on Put.io
    /// (feature 070)
    pub const NOT_CHOSEN: &str = "not_chosen";
}

/// The config keys a reason points at
mod setting {
    pub const SCHEDULE: &str = "download.bandwidth_schedule";
    pub const LIMIT: &str = "download.bandwidth_limit_bytes_per_sec";
    pub const MIN_FREE: &str = "download.min_free_space_mb";
    pub const TARGET: &str = "download.target_directory";
    pub const WORKERS: &str = "download.workers";
    pub const WINDOWS: &str = "download.windows";
    pub const PAUSE_FREE: &str = "download.pause_free_space_mb";
}

/// Why a row is not downloading
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Waiting {
    /// One of [`code`]
    pub code: &'static str,
    /// The config key of the setting behind it, when one is
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setting: Option<&'static str>,
    /// When it ends, RFC 3339 UTC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// When it ends on the daemon's wall clock, `YYYY-MM-DDTHH:MM` (schedule entries)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until_local: Option<String>,
    /// The 1-based bandwidth schedule entry
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_free_bytes: Option<u64>,
    /// The free space that ends a `disk_pause`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_bytes: Option<u64>,
    /// `download.workers`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workers: Option<usize>,
    /// What failed: `network`, `http`, `io`, `integrity`, `disk_full` or `other`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
}

impl Waiting {
    /// A reason with no fields
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            ..Self::default()
        }
    }

    fn setting(mut self, key: &'static str) -> Self {
        self.setting = Some(key);
        self
    }
}

/// The reason a paused bandwidth limit gives, or `None` when downloads may run
pub fn bandwidth_hold(status: &bandwidth::Status) -> Option<Waiting> {
    if status.limit != bandwidth::Limit::Paused {
        return None;
    }
    Some(match status.source {
        Source::Pause | Source::Slow => Waiting {
            until: status
                .override_
                .filter(|o| o.mode == OverrideMode::Pause)
                .map(|o| o.until.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            ..Waiting::new(code::BANDWIDTH_PAUSE)
        },
        Source::Schedule => Waiting {
            entry: status.entry,
            until_local: status.next_change.as_ref().map(|c| c.at.clone()),
            ..Waiting::new(code::BANDWIDTH_SCHEDULE).setting(setting::SCHEDULE)
        },
        Source::Default => Waiting::new(code::BANDWIDTH_LIMIT).setting(setting::LIMIT),
    })
}

/// The reason a closed download window or the free-space pause gives (feature 064)
pub fn rule_hold(hold: Hold) -> Waiting {
    match hold {
        Hold::Window { reopens } => Waiting {
            until_local: reopens.map(|t| t.format("%Y-%m-%dT%H:%M").to_string()),
            ..Waiting::new(code::DOWNLOAD_WINDOW).setting(setting::WINDOWS)
        },
        Hold::Space {
            free_bytes,
            pause_bytes,
            resume_bytes,
        } => Waiting {
            free_bytes: Some(free_bytes),
            min_free_bytes: Some(pause_bytes),
            resume_bytes: Some(resume_bytes),
            ..Waiting::new(code::DISK_PAUSE).setting(setting::PAUSE_FREE)
        },
    }
}

/// What the workers check before they claim a row, at one moment
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueState {
    /// [`bandwidth_hold`] of the limit in force
    pub bandwidth: Option<Waiting>,
    /// [`rule_hold`] of the download window or free-space pause in force (feature 064).
    /// It stops running downloads too.
    pub rule: Option<Waiting>,
    /// The Pause local downloads switch
    pub nas_paused: bool,
    /// `download.min_free_space_mb` in bytes; 0 is off
    pub min_free_bytes: u64,
    /// Free bytes of the download folder; `None` when the floor is off or it can't be read
    pub free_bytes: Option<u64>,
    /// Bytes the running downloads have still to write
    pub reserved_bytes: u64,
    /// `download.workers`
    pub workers: usize,
    /// Rows downloading now
    pub running: usize,
    /// What failed, by file id, for rows this run of the daemon saw fail
    pub failures: HashMap<i64, &'static str>,
}

impl QueueState {
    /// What holds every pending row back, in the order `process_queue` checks
    pub fn gate(&self) -> Option<Waiting> {
        if let Some(hold) = self.bandwidth.as_ref().or(self.rule.as_ref()) {
            return Some(hold.clone());
        }
        if self.nas_paused {
            return Some(Waiting::new(code::DOWNLOADS_PAUSED));
        }
        if self.min_free_bytes == 0 {
            return None;
        }
        let Some(free) = self.free_bytes else {
            return Some(Waiting::new(code::DISK_UNREADABLE).setting(setting::TARGET));
        };
        below_free_space_floor(
            free.saturating_sub(self.reserved_bytes),
            self.min_free_bytes,
            0,
        )
        .then(|| self.space(code::DISK_SPACE, free))
    }

    fn space(&self, code: &'static str, free: u64) -> Waiting {
        Waiting {
            free_bytes: Some(free),
            min_free_bytes: Some(self.min_free_bytes),
            ..Waiting::new(code).setting(setting::MIN_FREE)
        }
    }

    /// Why the queue row `file_id` of `size` bytes is not downloading; `None` when it is,
    /// or when nothing holds it back (a worker is about to take it)
    pub fn for_row(&self, status: &str, file_id: i64, size: i64) -> Option<Waiting> {
        match status {
            "paused" => Some(Waiting::new(code::PAUSED)),
            "failed" => Some(self.failed(&[file_id])),
            "pending" => self.gate().or_else(|| self.for_pending(size)),
            "downloading" => self.stopped(),
            _ => None,
        }
    }

    /// Why a row that is downloading gets no bytes: a closed download window or the
    /// free-space pause, which stop running downloads in place (feature 064)
    pub fn stopped(&self) -> Option<Waiting> {
        self.rule.clone()
    }

    /// A failed row or item: the first of `file_ids` with a known failure names the kind
    pub fn failed(&self, file_ids: &[i64]) -> Waiting {
        Waiting {
            kind: file_ids
                .iter()
                .find_map(|id| self.failures.get(id).copied()),
            ..Waiting::new(code::FAILED)
        }
    }

    /// A pending row no gate holds: too big for the room left, or waiting for a worker
    fn for_pending(&self, size: i64) -> Option<Waiting> {
        if let (true, Some(free)) = (self.min_free_bytes > 0, self.free_bytes) {
            let room = free
                .saturating_sub(self.reserved_bytes)
                .saturating_sub(self.min_free_bytes);
            if u64::try_from(size).unwrap_or(0) > room {
                return Some(self.space(code::FILE_TOO_BIG, free));
            }
        }
        (self.running >= self.workers).then(|| Waiting {
            workers: Some(self.workers),
            ..Waiting::new(code::WAITING_TURN).setting(setting::WORKERS)
        })
    }

    /// Why an Active item whose files are all pending is not downloading: the gate, else
    /// what holds its files. `None` as soon as one file is free to start.
    pub fn for_pending_files(&self, sizes: impl IntoIterator<Item = i64>) -> Option<Waiting> {
        if let Some(gate) = self.gate() {
            return Some(gate);
        }
        let mut too_big = None;
        for size in sizes {
            match self.for_pending(size) {
                None => return None,
                Some(w) if w.code == code::FILE_TOO_BIG => too_big = too_big.or(Some(w)),
                Some(w) => return Some(w),
            }
        }
        too_big
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use putio_sync_core::bandwidth::{Limit, NextChange, Override, Status};

    const GB: u64 = 1 << 30;

    fn status(limit: Limit, source: Source) -> Status {
        Status {
            limit,
            bytes_per_sec: None,
            source,
            entry: None,
            override_: None,
            next_change: None,
            local_time: "2026-11-27T20:00:00".to_string(),
            utc_offset: "+00:00".to_string(),
            slow_limit_bytes_per_sec: 1,
        }
    }

    /// Two workers, one busy, nothing in the way
    fn open() -> QueueState {
        QueueState {
            workers: 2,
            running: 1,
            ..QueueState::default()
        }
    }

    #[test]
    fn a_limit_that_lets_bytes_through_is_no_reason() {
        assert_eq!(
            bandwidth_hold(&status(Limit::Unlimited, Source::Default)),
            None
        );
        assert_eq!(
            bandwidth_hold(&status(Limit::Rate(1000), Source::Schedule)),
            None
        );
    }

    #[test]
    fn a_pause_says_when_it_ends() {
        let mut s = status(Limit::Paused, Source::Pause);
        s.override_ = Some(Override {
            mode: OverrideMode::Pause,
            until: Utc.with_ymd_and_hms(2026, 11, 27, 21, 30, 0).unwrap(),
        });
        let hold = bandwidth_hold(&s).unwrap();
        assert_eq!(hold.code, code::BANDWIDTH_PAUSE);
        assert_eq!(hold.until.as_deref(), Some("2026-11-27T21:30:00Z"));
        assert_eq!(hold.setting, None);
    }

    #[test]
    fn a_schedule_entry_names_itself_and_its_end() {
        let mut s = status(Limit::Paused, Source::Schedule);
        s.entry = Some(2);
        s.next_change = Some(NextChange {
            at: "2026-11-28T06:00".to_string(),
            limit: Limit::Unlimited,
            source: Source::Default,
            entry: None,
        });
        let hold = bandwidth_hold(&s).unwrap();
        assert_eq!(hold.code, code::BANDWIDTH_SCHEDULE);
        assert_eq!(hold.entry, Some(2));
        assert_eq!(hold.until_local.as_deref(), Some("2026-11-28T06:00"));
        assert_eq!(hold.setting, Some("download.bandwidth_schedule"));
    }

    #[test]
    fn a_paused_default_limit_points_at_the_limit() {
        let hold = bandwidth_hold(&status(Limit::Paused, Source::Default)).unwrap();
        assert_eq!(hold.code, code::BANDWIDTH_LIMIT);
        assert_eq!(hold.setting, Some("download.bandwidth_limit_bytes_per_sec"));
    }

    #[test]
    fn the_gates_come_in_the_order_the_workers_check_them() {
        let mut q = QueueState {
            bandwidth: Some(Waiting::new(code::BANDWIDTH_PAUSE)),
            nas_paused: true,
            min_free_bytes: 10 * GB,
            free_bytes: Some(GB),
            ..open()
        };
        assert_eq!(q.gate().unwrap().code, code::BANDWIDTH_PAUSE);
        q.bandwidth = None;
        assert_eq!(q.gate().unwrap().code, code::DOWNLOADS_PAUSED);
        q.nas_paused = false;
        let space = q.gate().unwrap();
        assert_eq!(space.code, code::DISK_SPACE);
        assert_eq!(space.free_bytes, Some(GB));
        assert_eq!(space.min_free_bytes, Some(10 * GB));
        assert_eq!(space.setting, Some("download.min_free_space_mb"));
        q.free_bytes = Some(50 * GB);
        assert_eq!(q.gate(), None);
    }

    #[test]
    fn unreadable_free_space_holds_only_with_the_floor_on() {
        let mut q = QueueState {
            min_free_bytes: GB,
            free_bytes: None,
            ..open()
        };
        assert_eq!(q.gate().unwrap().code, code::DISK_UNREADABLE);
        q.min_free_bytes = 0;
        assert_eq!(q.gate(), None);
    }

    #[test]
    fn running_downloads_count_against_free_space() {
        let q = QueueState {
            min_free_bytes: 10 * GB,
            free_bytes: Some(12 * GB),
            reserved_bytes: 3 * GB,
            ..open()
        };
        assert_eq!(q.gate().unwrap().code, code::DISK_SPACE);
    }

    #[test]
    fn a_row_too_big_for_the_room_waits_while_smaller_ones_go() {
        let q = QueueState {
            min_free_bytes: 10 * GB,
            free_bytes: Some(15 * GB),
            ..open()
        };
        let big = q.for_row("pending", 1, 6 * GB as i64).unwrap();
        assert_eq!(big.code, code::FILE_TOO_BIG);
        assert_eq!(big.free_bytes, Some(15 * GB));
        assert_eq!(q.for_row("pending", 2, GB as i64), None);
    }

    #[test]
    fn a_pending_row_waits_its_turn_only_when_every_worker_is_busy() {
        let mut q = open();
        assert_eq!(q.for_row("pending", 1, 10), None);
        q.running = 2;
        let turn = q.for_row("pending", 1, 10).unwrap();
        assert_eq!(turn.code, code::WAITING_TURN);
        assert_eq!(turn.workers, Some(2));
        assert_eq!(turn.setting, Some("download.workers"));
    }

    #[test]
    fn a_gate_wins_over_the_row() {
        let q = QueueState {
            nas_paused: true,
            running: 2,
            ..open()
        };
        assert_eq!(
            q.for_row("pending", 1, 10).unwrap().code,
            code::DOWNLOADS_PAUSED
        );
    }

    #[test]
    fn paused_and_failed_rows_say_so_whatever_the_gate() {
        let mut q = QueueState {
            nas_paused: true,
            ..open()
        };
        q.failures.insert(7, "network");
        assert_eq!(q.for_row("paused", 1, 10).unwrap().code, code::PAUSED);
        let failed = q.for_row("failed", 7, 10).unwrap();
        assert_eq!(failed.code, code::FAILED);
        assert_eq!(failed.kind, Some("network"));
        // A failure from before the restart has no kind
        assert_eq!(q.for_row("failed", 8, 10).unwrap().kind, None);
    }

    #[test]
    fn a_running_row_has_no_reason() {
        let q = QueueState {
            nas_paused: true,
            ..open()
        };
        assert_eq!(q.for_row("downloading", 1, 10), None);
        assert_eq!(q.for_row("completed", 1, 10), None);
    }

    #[test]
    fn an_item_has_a_reason_only_when_none_of_its_files_can_start() {
        let mut q = QueueState {
            min_free_bytes: 10 * GB,
            free_bytes: Some(15 * GB),
            ..open()
        };
        let (big, small) = (6 * GB as i64, GB as i64);
        assert_eq!(
            q.for_pending_files([big, big]).unwrap().code,
            code::FILE_TOO_BIG
        );
        assert_eq!(q.for_pending_files([big, small]), None);
        q.running = 2;
        assert_eq!(
            q.for_pending_files([big, small]).unwrap().code,
            code::WAITING_TURN
        );
        q.nas_paused = true;
        assert_eq!(
            q.for_pending_files([small]).unwrap().code,
            code::DOWNLOADS_PAUSED
        );
    }

    #[test]
    fn only_the_fields_a_reason_has_are_sent() {
        let json = serde_json::to_value(Waiting::new(code::PAUSED)).unwrap();
        assert_eq!(json, serde_json::json!({ "code": "paused" }));
    }

    fn closed_window() -> Waiting {
        rule_hold(Hold::Window {
            reopens: chrono::NaiveDate::from_ymd_opt(2026, 11, 28)
                .unwrap()
                .and_hms_opt(1, 0, 0),
        })
    }

    #[test]
    fn a_closed_window_says_when_it_opens_and_names_its_setting() {
        let hold = closed_window();
        assert_eq!(hold.code, code::DOWNLOAD_WINDOW);
        assert_eq!(hold.until_local.as_deref(), Some("2026-11-28T01:00"));
        assert_eq!(hold.setting, Some("download.windows"));
        let json = serde_json::to_value(&hold).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"code": "download_window", "setting": "download.windows", "until_local": "2026-11-28T01:00"})
        );
    }

    #[test]
    fn the_space_pause_carries_both_levels() {
        let hold = rule_hold(Hold::Space {
            free_bytes: 3 * GB,
            pause_bytes: 5 * GB,
            resume_bytes: 8 * GB,
        });
        assert_eq!(hold.code, code::DISK_PAUSE);
        assert_eq!(hold.setting, Some("download.pause_free_space_mb"));
        assert_eq!(
            (hold.free_bytes, hold.min_free_bytes, hold.resume_bytes),
            (Some(3 * GB), Some(5 * GB), Some(8 * GB))
        );
    }

    #[test]
    fn a_rule_holds_pending_and_running_rows_but_a_bandwidth_pause_is_named_first() {
        let mut state = QueueState {
            rule: Some(closed_window()),
            nas_paused: true,
            ..open()
        };
        let code_of = |s: &QueueState, status: &str| s.for_row(status, 1, 10).map(|w| w.code);
        assert_eq!(code_of(&state, "pending"), Some(code::DOWNLOAD_WINDOW));
        assert_eq!(code_of(&state, "downloading"), Some(code::DOWNLOAD_WINDOW));
        assert_eq!(code_of(&state, "paused"), Some(code::PAUSED));
        assert_eq!(
            state.for_pending_files([10]).map(|w| w.code),
            Some(code::DOWNLOAD_WINDOW)
        );
        state.bandwidth = bandwidth_hold(&status(Limit::Paused, Source::Default));
        assert_eq!(code_of(&state, "pending"), Some(code::BANDWIDTH_LIMIT));
        // A running row is told about the rule only: 066 gives it no bandwidth line
        assert_eq!(code_of(&state, "downloading"), Some(code::DOWNLOAD_WINDOW));
        state.rule = None;
        assert_eq!(code_of(&state, "downloading"), None);
    }
}

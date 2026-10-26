//! In-memory state of the Put.io folder sync, read by the API's status page and
//! "sync now" (feature 004, R5).
//!
//! Times are `SystemTime` so this crate needs no date library; the API converts them.

use crate::manager::SyncStats;
use std::time::{Duration, SystemTime};

/// State of the folder sync
#[derive(Clone, Debug, Default)]
pub struct SyncStatus {
    /// A `sync_downloads` call is running
    pub in_progress: bool,
    /// When the last sync began
    pub last_started_at: Option<SystemTime>,
    /// When the last sync ended
    pub last_finished_at: Option<SystemTime>,
    /// How long the last sync took
    pub last_duration: Option<Duration>,
    /// Result of the last sync; `None` before the first one
    pub last_outcome: Option<SyncOutcome>,
    /// When the loop runs next; `None` when no loop runs
    pub next_due_at: Option<SystemTime>,
    /// The loop interval; `None` when no loop runs
    pub interval: Option<Duration>,
}

/// Result of one sync
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncOutcome {
    /// The sync listed Put.io and updated the queue
    Success {
        /// Files found on Put.io
        found: usize,
        /// Files added to the queue
        queued: usize,
        /// Files already downloaded or queued
        skipped: usize,
        /// Files with a blocked extension
        blocked: usize,
        /// Files that could not be queued
        failed: usize,
    },
    /// The sync failed; `error` is the token-free error chain
    Failure {
        /// What went wrong
        error: String,
    },
}

impl From<&SyncStats> for SyncOutcome {
    fn from(stats: &SyncStats) -> Self {
        SyncOutcome::Success {
            found: stats.total_files,
            queued: stats.files_downloaded,
            skipped: stats.files_skipped,
            blocked: stats.files_blocked,
            failed: stats.files_failed,
        }
    }
}

/// Why a "sync now" request was refused
#[derive(Debug, thiserror::Error)]
pub enum SyncRequestError {
    /// Only one sync runs at a time
    #[error("a sync is already in progress")]
    InProgress,
}

impl SyncStatus {
    /// A sync started
    pub fn begin(&mut self) {
        self.in_progress = true;
        self.last_started_at = Some(SystemTime::now());
    }

    /// A sync ended with `outcome`
    pub fn finish(&mut self, outcome: SyncOutcome) {
        let now = SystemTime::now();
        self.in_progress = false;
        self.last_finished_at = Some(now);
        self.last_duration = self
            .last_started_at
            .and_then(|started| now.duration_since(started).ok());
        self.last_outcome = Some(outcome);
        self.next_due_at = self.interval.map(|interval| now + interval);
    }

    /// A sync loop runs every `interval`
    pub fn schedule(&mut self, interval: Duration) {
        self.interval = Some(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_marks_in_progress() {
        let mut status = SyncStatus::default();
        status.begin();
        assert!(status.in_progress);
        assert!(status.last_started_at.is_some());
        assert!(status.last_outcome.is_none());
    }

    #[test]
    fn finish_success_with_loop_sets_next_due() {
        let mut status = SyncStatus::default();
        status.schedule(Duration::from_secs(60));
        status.begin();
        let stats = SyncStats {
            total_files: 3,
            files_downloaded: 1,
            files_skipped: 1,
            files_failed: 0,
            files_blocked: 1,
        };
        status.finish(SyncOutcome::from(&stats));

        assert!(!status.in_progress);
        assert_eq!(
            status.last_outcome,
            Some(SyncOutcome::Success {
                found: 3,
                queued: 1,
                skipped: 1,
                blocked: 1,
                failed: 0
            })
        );
        let finished = status.last_finished_at.unwrap();
        assert!(status.last_duration.is_some());
        assert_eq!(
            status.next_due_at.unwrap(),
            finished + Duration::from_secs(60)
        );
    }

    #[test]
    fn finish_failure_without_loop_has_no_next_due() {
        let mut status = SyncStatus::default();
        status.begin();
        status.finish(SyncOutcome::Failure {
            error: "boom".to_string(),
        });

        assert!(!status.in_progress);
        assert_eq!(
            status.last_outcome,
            Some(SyncOutcome::Failure {
                error: "boom".to_string()
            })
        );
        assert!(status.next_due_at.is_none());
        assert!(status.interval.is_none());
    }
}

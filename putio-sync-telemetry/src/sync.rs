//! Sync run metrics.

use std::time::Duration;

use crate::{
    global, unix_now, Mp4Request, OrganiseMoveKind, OrganiseResult, RuleDecision, SyncKind,
    SyncResult, Telemetry,
};

impl Telemetry {
    /// Mark a sync run as in progress, until [`Telemetry::sync_run`] records its end.
    pub fn sync_started(&self) {
        self.sync_in_progress.set(1);
    }

    /// Record a finished sync run and the latest per-kind file counts. A success also
    /// sets the last-success timestamp. Either way the run is no longer in progress.
    pub fn sync_run(&self, result: SyncResult, elapsed: Duration, counts: &[(SyncKind, usize)]) {
        self.sync_in_progress.set(0);
        let secs = elapsed.as_secs_f64();
        self.sync_runs.with_label_values(&[result.as_str()]).inc();
        self.sync_duration.observe(secs);
        self.last_sync_duration.set(secs);
        for (kind, n) in counts {
            self.last_sync_files
                .with_label_values(&[kind.as_str()])
                .set(*n as i64);
        }
        if result == SyncResult::Success {
            self.last_success
                .with_label_values::<&str>(&[])
                .set(unix_now());
        }
    }

    /// Count a file blocked or skipped by a rule (call once per file id).
    pub fn sync_file_blocked(&self) {
        self.files_blocked.inc();
    }

    /// Count a folder rule decision.
    pub fn sync_rule_decision(&self, decision: RuleDecision) {
        self.rule_decisions
            .with_label_values(&[decision.as_str()])
            .inc();
    }
}

impl Telemetry {
    /// Count one MP4 request for a stream-mode file (feature 022).
    pub fn sync_stream_mp4_request(&self, result: Mp4Request) {
        self.stream_mp4_requests
            .with_label_values(&[result.as_str()])
            .inc();
    }

    /// Count one organiser move outcome (feature 023).
    pub fn sync_organise_move(&self, kind: OrganiseMoveKind, result: OrganiseResult) {
        self.organise_moves
            .with_label_values(&[kind.as_str(), result.as_str()])
            .inc();
    }
}

/// Mark a sync run as in progress on the global instance.
pub fn started() {
    global().sync_started();
}

/// Record a finished sync run on the global instance.
pub fn record_run(result: SyncResult, elapsed: Duration, counts: &[(SyncKind, usize)]) {
    global().sync_run(result, elapsed, counts);
}

/// Count a blocked file on the global instance.
pub fn file_blocked() {
    global().sync_file_blocked();
}

/// Count a folder rule decision on the global instance.
pub fn rule_decision(decision: RuleDecision) {
    global().sync_rule_decision(decision);
}

/// Count one MP4 request for a stream-mode file on the global instance.
pub fn stream_mp4_request(result: Mp4Request) {
    global().sync_stream_mp4_request(result);
}

/// Count one organiser move outcome on the global instance.
pub fn organise_move(kind: OrganiseMoveKind, result: OrganiseResult) {
    global().sync_organise_move(kind, result);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_progress_is_set_by_start_and_cleared_by_any_result() {
        let t = Telemetry::new();
        let in_progress = || t.sample("putio_sync_sync_in_progress", &[]);
        assert_eq!(in_progress(), Some(0.0));
        for result in SyncResult::ALL {
            t.sync_started();
            assert_eq!(in_progress(), Some(1.0));
            t.sync_run(*result, Duration::from_secs(1), &[]);
            assert_eq!(in_progress(), Some(0.0), "after {:?}", result);
        }
    }
}

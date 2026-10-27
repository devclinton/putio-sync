//! Sync run metrics.

use std::time::Duration;

use crate::{global, unix_now, RuleDecision, SyncKind, SyncResult, Telemetry};

impl Telemetry {
    /// Record a finished sync run and the latest per-kind file counts. A success also
    /// sets the last-success timestamp.
    pub fn sync_run(&self, result: SyncResult, elapsed: Duration, counts: &[(SyncKind, usize)]) {
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

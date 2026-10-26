//! Empty-folder cleanup metrics.

use crate::{global, unix_now, CleanupTrigger, SyncResult, Telemetry};

impl Telemetry {
    /// Record a finished (not dry) cleanup run. A success also sets the last-success
    /// timestamp.
    pub fn empty_folder_run(&self, trigger: CleanupTrigger, result: SyncResult) {
        self.empty_folder_runs
            .with_label_values(&[trigger.as_str(), result.as_str()])
            .inc();
        if result == SyncResult::Success {
            self.empty_folder_last_success
                .with_label_values::<&str>(&[])
                .set(unix_now());
        }
    }

    /// Count `n` empty folders removed.
    pub fn empty_folders_deleted(&self, n: usize) {
        self.empty_folders_deleted.inc_by(n as u64);
    }

    /// Count one failed empty-folder delete.
    pub fn empty_folder_delete_failed(&self) {
        self.empty_folder_delete_failures.inc();
    }
}

/// Record a finished cleanup run on the global instance.
pub fn record_run(trigger: CleanupTrigger, result: SyncResult) {
    global().empty_folder_run(trigger, result);
}

/// Count removed folders on the global instance.
pub fn deleted(n: usize) {
    global().empty_folders_deleted(n);
}

/// Count a failed delete on the global instance.
pub fn delete_failed() {
    global().empty_folder_delete_failed();
}

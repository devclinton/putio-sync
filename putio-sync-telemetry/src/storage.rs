//! Put.io storage metrics (feature 033).

use crate::{global, CleanupTrigger, SyncResult, Telemetry};

impl Telemetry {
    /// Set the Put.io account size and use, in bytes.
    pub fn putio_storage(&self, size: i64, used: i64) {
        self.putio_storage_size.set(size as f64);
        self.putio_storage_used.set(used as f64);
    }

    /// Record a finished free-up run and what it deleted.
    pub fn storage_free_run(
        &self,
        trigger: CleanupTrigger,
        result: SyncResult,
        files: usize,
        bytes: u64,
    ) {
        self.storage_free_runs
            .with_label_values(&[trigger.as_str(), result.as_str()])
            .inc();
        self.storage_freed_files.inc_by(files as u64);
        self.storage_freed_bytes.inc_by(bytes);
    }
}

/// Set the Put.io storage gauges on the global instance.
pub fn set_storage(size: i64, used: i64) {
    global().putio_storage(size, used);
}

/// Record a finished free-up run on the global instance.
pub fn record_run(trigger: CleanupTrigger, result: SyncResult, files: usize, bytes: u64) {
    global().storage_free_run(trigger, result, files, bytes);
}

#[cfg(test)]
mod tests {
    use crate::{CleanupTrigger, SyncResult, Telemetry};

    #[test]
    fn storage_metrics_are_exposed() {
        let t = Telemetry::new();
        t.putio_storage(100, 90);
        t.storage_free_run(CleanupTrigger::Manual, SyncResult::Success, 2, 30);
        let text = t.encode();
        assert!(text.contains("putio_sync_putio_storage_size_bytes 100"));
        assert!(text.contains("putio_sync_putio_storage_used_bytes 90"));
        assert!(text.contains(
            "putio_sync_storage_free_runs_total{result=\"success\",trigger=\"manual\"} 1"
        ));
        assert!(text.contains(
            "putio_sync_storage_free_runs_total{result=\"failure\",trigger=\"schedule\"} 0"
        ));
        assert!(text.contains("putio_sync_storage_freed_files_total 2"));
        assert!(text.contains("putio_sync_storage_freed_bytes_total 30"));
    }
}

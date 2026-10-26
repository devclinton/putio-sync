//! Watch-folder metrics.

use crate::{global, Telemetry};

impl Telemetry {
    /// Count one watch-folder scan.
    pub fn watch_scan(&self) {
        self.watch_scans.inc();
    }

    /// Count one torrent or magnet file uploaded to Put.io.
    pub fn watch_uploaded(&self) {
        self.watch_uploads.inc();
    }

    /// Count one already-uploaded file cleaned up.
    pub fn watch_cleaned_up(&self) {
        self.watch_cleanups.inc();
    }

    /// Count one invalid file (call once per file).
    pub fn watch_invalid_file(&self) {
        self.watch_invalid.inc();
    }

    /// Count one cleanup (delete or archive) failure.
    pub fn watch_cleanup_failed(&self) {
        self.watch_cleanup_failures.inc();
    }
}

/// Count a scan on the global instance.
pub fn scan() {
    global().watch_scan();
}

/// Count an upload on the global instance.
pub fn uploaded() {
    global().watch_uploaded();
}

/// Count a cleanup on the global instance.
pub fn cleaned_up() {
    global().watch_cleaned_up();
}

/// Count an invalid file on the global instance.
pub fn invalid() {
    global().watch_invalid_file();
}

/// Count a cleanup failure on the global instance.
pub fn cleanup_failed() {
    global().watch_cleanup_failed();
}

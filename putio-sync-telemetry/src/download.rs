//! Download and worker metrics.

use std::time::Duration;

use crate::{global, ExistingOutcome, FailReason, Telemetry};

/// Counts one active download for as long as it is held.
#[must_use = "the download is only counted while the guard is held"]
pub struct ActiveGuard<'a> {
    telemetry: &'a Telemetry,
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.telemetry.downloads_active.dec();
    }
}

impl Telemetry {
    /// Count a download as active until the guard is dropped.
    pub fn download_active(&self) -> ActiveGuard<'_> {
        self.downloads_active.inc();
        ActiveGuard { telemetry: self }
    }

    /// Add bytes written to disk.
    pub fn download_bytes(&self, n: u64) {
        self.downloaded_bytes.inc_by(n);
    }

    /// Count a download that was verified and renamed into place.
    pub fn download_completed(&self, elapsed: Duration) {
        self.downloads_completed.inc();
        self.download_duration.observe(elapsed.as_secs_f64());
    }

    /// Count a failed download.
    pub fn download_failed(&self, reason: FailReason) {
        self.downloads_failed
            .with_label_values(&[reason.as_str()])
            .inc();
    }

    /// Count a file that already existed locally, by what was done with it.
    pub fn download_existing(&self, outcome: ExistingOutcome) {
        self.existing_files
            .with_label_values(&[outcome.as_str()])
            .inc();
    }

    /// Mark a worker busy or idle. No-op unless `detailed`.
    pub fn worker_busy(&self, id: usize, busy: bool) {
        if let Some((state, inflight)) = self.worker(id) {
            state.set(i64::from(busy));
            if !busy {
                inflight.set(0);
            }
        }
    }

    /// Set the bytes written so far for a worker's current file. No-op unless `detailed`.
    pub fn worker_inflight(&self, id: usize, bytes: u64) {
        if let Some((_, inflight)) = self.worker(id) {
            inflight.set(bytes as i64);
        }
    }
}

/// Count a download as active on the global instance until the guard is dropped.
pub fn active() -> ActiveGuard<'static> {
    global().download_active()
}

/// Add bytes written to disk on the global instance.
pub fn bytes(n: u64) {
    global().download_bytes(n);
}

/// Count a completed download on the global instance.
pub fn completed(elapsed: Duration) {
    global().download_completed(elapsed);
}

/// Count a failed download on the global instance.
pub fn failed(reason: FailReason) {
    global().download_failed(reason);
}

/// Count a file that already existed locally on the global instance.
pub fn existing(outcome: ExistingOutcome) {
    global().download_existing(outcome);
}

/// Mark a worker busy or idle on the global instance.
pub fn worker_busy(id: usize, busy: bool) {
    global().worker_busy(id, busy);
}

/// Set a worker's in-flight bytes on the global instance.
pub fn worker_inflight(id: usize, bytes: u64) {
    global().worker_inflight(id, bytes);
}

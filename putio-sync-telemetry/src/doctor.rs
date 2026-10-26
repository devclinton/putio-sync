//! Doctor metrics (feature 010).
//!
//! `check` is one of the doctor's fixed check ids, so the label set stays bounded.

use std::time::Duration;

use crate::{global, unix_now, Telemetry};

impl Telemetry {
    /// Record one check's result: `status` is 0 ok, 1 warning, 2 problem, -1 skipped.
    pub fn doctor_check(&self, check: &str, status: f64, items: u64) {
        self.doctor_check_status
            .with_label_values(&[check])
            .set(status);
        self.doctor_check_items
            .with_label_values(&[check])
            .set(items as i64);
    }

    /// Record a finished run.
    pub fn doctor_run(&self, duration: Duration) {
        self.doctor_last_run
            .with_label_values::<&str>(&[])
            .set(unix_now());
        self.doctor_last_duration
            .with_label_values::<&str>(&[])
            .set(duration.as_secs_f64());
    }
}

/// Record one check's result on the global instance.
pub fn record_check(check: &str, status: f64, items: u64) {
    global().doctor_check(check, status, items);
}

/// Record a finished run on the global instance.
pub fn record_run(duration: Duration) {
    global().doctor_run(duration);
}

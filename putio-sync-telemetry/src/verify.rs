//! Download verification metrics (feature 034).

use crate::{global, Telemetry};

/// What started a check
pub const TRIGGERS: [&str; 2] = ["download", "run"];
/// How a file was checked
pub const METHODS: [&str; 2] = ["crc32", "size"];
/// How a check ended
pub const RESULTS: [&str; 2] = ["passed", "failed"];
/// How a verify run classed a file
pub const RUN_RESULTS: [&str; 7] = [
    "verified",
    "mismatch",
    "missing",
    "not_on_putio",
    "busy",
    "unreadable",
    "not_checked",
];

/// One of the known values, else the first, so labels stay bounded
fn known<'a>(value: &str, values: &[&'a str]) -> &'a str {
    values
        .iter()
        .find(|v| **v == value)
        .copied()
        .unwrap_or(values[0])
}

pub(crate) fn zero_init(t: &Telemetry) {
    for trigger in TRIGGERS {
        for method in METHODS {
            for result in RESULTS {
                t.verify_checks
                    .with_label_values(&[trigger, method, result]);
            }
        }
        t.verify_redownloads.with_label_values(&[trigger]);
    }
    for result in RUN_RESULTS {
        t.verify_run_files.with_label_values(&[result]);
    }
}

impl Telemetry {
    /// Count a check: `trigger` `download|run`, `method` `crc32|size`, `passed`.
    pub fn verify_check(&self, trigger: &str, method: &str, passed: bool) {
        let result = if passed { "passed" } else { "failed" };
        self.verify_checks
            .with_label_values(&[known(trigger, &TRIGGERS), known(method, &METHODS), result])
            .inc();
    }

    /// Count a file sent to be downloaded again.
    pub fn verify_redownload(&self, trigger: &str) {
        self.verify_redownloads
            .with_label_values(&[known(trigger, &TRIGGERS)])
            .inc();
    }

    /// Count a file a verify run classed as `result`.
    pub fn verify_run_file(&self, result: &str) {
        if RUN_RESULTS.contains(&result) {
            self.verify_run_files.with_label_values(&[result]).inc();
        }
    }

    /// Count bytes read back from disk to hash.
    pub fn verify_read(&self, bytes: u64) {
        self.verify_read_bytes.inc_by(bytes);
    }

    /// Whether a verify run is going.
    pub fn set_verify_run_active(&self, active: bool) {
        self.verify_run_active.set(i64::from(active));
    }
}

/// [`Telemetry::verify_check`] on the global instance.
pub fn record_check(trigger: &str, method: &str, passed: bool) {
    global().verify_check(trigger, method, passed);
}

/// [`Telemetry::verify_redownload`] on the global instance.
pub fn record_redownload(trigger: &str) {
    global().verify_redownload(trigger);
}

/// [`Telemetry::verify_run_file`] on the global instance.
pub fn record_run_file(result: &str) {
    global().verify_run_file(result);
}

/// [`Telemetry::verify_read`] on the global instance.
pub fn record_read(bytes: u64) {
    global().verify_read(bytes);
}

/// [`Telemetry::set_verify_run_active`] on the global instance.
pub fn set_run_active(active: bool) {
    global().set_verify_run_active(active);
}

#[cfg(test)]
mod tests {
    use crate::Telemetry;

    #[test]
    fn verify_metrics_are_exposed_with_bounded_labels() {
        let t = Telemetry::new();
        t.verify_check("download", "crc32", true);
        t.verify_check("run", "size", false);
        t.verify_check("bogus", "bogus", true);
        t.verify_redownload("download");
        t.verify_run_file("mismatch");
        t.verify_run_file("bogus");
        t.verify_read(4096);
        t.set_verify_run_active(true);
        let text = t.encode();
        assert!(text.contains(
            "putio_sync_verify_total{method=\"crc32\",result=\"passed\",trigger=\"download\"} 2"
        ));
        assert!(text.contains(
            "putio_sync_verify_total{method=\"size\",result=\"failed\",trigger=\"run\"} 1"
        ));
        assert!(text.contains(
            "putio_sync_verify_total{method=\"size\",result=\"passed\",trigger=\"run\"} 0"
        ));
        assert!(text.contains("putio_sync_verify_redownloads_total{trigger=\"download\"} 1"));
        assert!(text.contains("putio_sync_verify_redownloads_total{trigger=\"run\"} 0"));
        assert!(text.contains("putio_sync_verify_run_files_total{result=\"mismatch\"} 1"));
        assert!(text.contains("putio_sync_verify_run_files_total{result=\"busy\"} 0"));
        assert!(!text.contains("bogus"));
        assert!(text.contains("putio_sync_verify_read_bytes_total 4096"));
        assert!(text.contains("putio_sync_verify_run_active 1"));
    }
}

//! Update checker metrics (feature 055).

use crate::{global, Telemetry};

/// How a releases check ended
pub const CHECK_RESULTS: [&str; 4] = ["ok", "not_modified", "error", "rate_limited"];
/// What an update attempt downloaded
pub const ATTEMPT_KINDS: [&str; 3] = ["patch", "full", "installer"];
/// How an update attempt ended
pub const ATTEMPT_RESULTS: [&str; 3] = ["done", "failed", "rolled_back"];
/// Which check refused an update
pub const VERIFY_STAGES: [&str; 3] = ["signature", "download", "binary"];

impl Telemetry {
    /// Count a releases check by result.
    pub fn update_check(&self, result: &str) {
        self.update_checks.with_label_values(&[result]).inc();
    }

    /// 1 while a newer release is known, 0 otherwise.
    pub fn set_update_available(&self, available: bool) {
        self.update_available.set(i64::from(available));
    }

    /// Count an update attempt by kind and result.
    pub fn update_attempt(&self, kind: &str, result: &str) {
        self.update_attempts
            .with_label_values(&[kind, result])
            .inc();
    }

    /// Count an update refused by a signature or hash check.
    pub fn update_verify_failure(&self, stage: &str) {
        self.update_verify_failures
            .with_label_values(&[stage])
            .inc();
    }
}

/// Count a releases check on the global instance.
pub fn record_check(result: &str) {
    global().update_check(result);
}

/// Set the update-available gauge on the global instance.
pub fn set_available(available: bool) {
    global().set_update_available(available);
}

/// Count an update attempt on the global instance.
pub fn record_attempt(kind: &str, result: &str) {
    global().update_attempt(kind, result);
}

/// Count a failed update check on the global instance.
pub fn record_verify_failure(stage: &str) {
    global().update_verify_failure(stage);
}

#[cfg(test)]
mod tests {
    use crate::Telemetry;

    #[test]
    fn update_metrics_are_exposed() {
        let t = Telemetry::new();
        t.update_check("ok");
        t.set_update_available(true);
        t.update_attempt("patch", "done");
        t.update_verify_failure("signature");
        let text = t.encode();
        assert!(text.contains("putio_sync_update_checks_total{result=\"ok\"} 1"));
        assert!(text.contains("putio_sync_update_checks_total{result=\"rate_limited\"} 0"));
        assert!(text.contains("putio_sync_update_available 1"));
        assert!(text.contains("putio_sync_update_attempts_total{kind=\"patch\",result=\"done\"} 1"));
        assert!(text.contains(
            "putio_sync_update_attempts_total{kind=\"installer\",result=\"rolled_back\"} 0"
        ));
        assert!(text.contains("putio_sync_update_verify_failures_total{stage=\"signature\"} 1"));
        assert!(text.contains("putio_sync_update_verify_failures_total{stage=\"binary\"} 0"));
    }
}

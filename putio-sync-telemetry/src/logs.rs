//! Log viewer metrics (feature 047).

use crate::{global, Telemetry};

/// Why a log line never reached the Logs page: `evicted` from the full buffer, or
/// `lagged` when a live tail fell behind and the line had already gone.
pub const DROP_REASONS: [&str; 2] = ["evicted", "lagged"];

impl Telemetry {
    /// Count `n` log lines dropped for `reason`, one of [`DROP_REASONS`].
    pub fn log_lines_dropped(&self, reason: &str, n: u64) {
        self.log_lines_dropped
            .with_label_values(&[reason])
            .inc_by(n);
    }
}

/// Count dropped log lines on the global instance.
pub fn record_dropped(reason: &str, n: u64) {
    global().log_lines_dropped(reason, n);
}

#[cfg(test)]
mod tests {
    use crate::Telemetry;

    #[test]
    fn dropped_lines_are_exposed_from_zero() {
        let t = Telemetry::new();
        let before = t.encode();
        assert!(before.contains("putio_sync_log_lines_dropped_total{reason=\"evicted\"} 0"));
        assert!(before.contains("putio_sync_log_lines_dropped_total{reason=\"lagged\"} 0"));
        t.log_lines_dropped("evicted", 5);
        assert!(t
            .encode()
            .contains("putio_sync_log_lines_dropped_total{reason=\"evicted\"} 5"));
    }
}

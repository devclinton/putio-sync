//! Event and hook metrics (feature 027).
//!
//! The `hook` label is the hook's configured name, which validation limits to
//! `[A-Za-z0-9_-]{1,40}` and the hook count to 32, so it cannot carry a path or URL.

use std::time::Duration;

use crate::{global, Telemetry};

/// Every event type, as on the wire. Kept equal to `putio_sync_core::events::EventType::ALL`
/// by a test in core.
pub const EVENT_TYPES: [&str; 16] = [
    "download.finished",
    "download.failed",
    "transfer.added",
    "transfer.finished",
    "transfer.failed",
    "transfer.stuck",
    "sync.completed",
    "storage.low",
    "verify.passed",
    "verify.failed",
    "verify.redownload",
    "pipeline.step_failed",
    "pipeline.completed",
    "doctor.problem",
    "hook.test",
    "plugin.test",
];
/// How a hook run ended
pub const RESULTS: [&str; 3] = ["success", "failure", "timeout"];

impl Telemetry {
    /// Count one event put on the bus.
    pub fn event_emitted(&self, event_type: &str) {
        self.events.with_label_values(&[event_type]).inc();
    }

    /// Create a configured hook's series at zero, so they show before its first run.
    pub fn hook_configured(&self, hook: &str) {
        self.hook_dropped.with_label_values(&[hook]);
        self.hook_duration.with_label_values(&[hook]);
    }

    /// Record one finished hook run.
    pub fn hook_run(&self, hook: &str, event_type: &str, result: &str, took: Duration) {
        self.hook_runs
            .with_label_values(&[hook, event_type, result])
            .inc();
        self.hook_duration
            .with_label_values(&[hook])
            .observe(took.as_secs_f64());
    }

    /// Count events dropped because the hook's queue was full.
    pub fn hook_dropped(&self, hook: &str, n: u64) {
        self.hook_dropped.with_label_values(&[hook]).inc_by(n);
    }
}

/// Count an event on the global instance.
pub fn record_event(event_type: &str) {
    global().event_emitted(event_type);
}

/// Create a hook's series on the global instance.
pub fn record_configured(hook: &str) {
    global().hook_configured(hook);
}

/// Record a hook run on the global instance.
pub fn record_run(hook: &str, event_type: &str, result: &str, took: Duration) {
    global().hook_run(hook, event_type, result, took);
}

/// Count dropped events on the global instance.
pub fn record_dropped(hook: &str, n: u64) {
    global().hook_dropped(hook, n);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::Telemetry;

    #[test]
    fn hook_metrics_are_exposed() {
        let t = Telemetry::new();
        let before = t.encode();
        assert!(before.contains("putio_sync_events_total{type=\"storage.low\"} 0"));
        t.event_emitted("download.finished");
        t.hook_configured("ha");
        t.hook_run(
            "ha",
            "download.finished",
            "success",
            Duration::from_millis(40),
        );
        t.hook_dropped("ha", 3);
        let text = t.encode();
        assert!(text.contains("putio_sync_events_total{type=\"download.finished\"} 1"));
        assert!(text.contains(
            "putio_sync_hook_runs_total{event=\"download.finished\",hook=\"ha\",result=\"success\"} 1"
        ));
        assert!(text.contains("putio_sync_hook_run_duration_seconds_count{hook=\"ha\"} 1"));
        assert!(text.contains("putio_sync_hook_events_dropped_total{hook=\"ha\"} 3"));
    }
}

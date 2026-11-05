//! Plugin metrics (feature 028).
//!
//! The `plugin` label is the plugin's name, which the manifest rules limit to
//! `[A-Za-z0-9_-]{1,40}` and the plugin count to 32, so it cannot carry a path or URL.
//! The `name` label of `plugin_counter_total` (031) is `[a-z0-9_]{1,40}`, at most 16 per
//! plugin, checked by the plugin host.

use std::time::Duration;

use crate::{global, Telemetry};

/// What a plugin run was for, besides the event types: the tick, the script's `init`,
/// and (031) a settings action or a health check.
pub const RUN_KINDS: [&str; 4] = ["tick", "init", "action", "health"];

impl Telemetry {
    /// Create a loaded plugin's series at zero, so they show before its first run.
    pub fn plugin_configured(&self, plugin: &str) {
        self.plugin_dropped.with_label_values(&[plugin]);
        self.plugin_duration.with_label_values(&[plugin]);
    }

    /// Record one finished plugin call. `event` is an event type or one of [`RUN_KINDS`];
    /// `result` is `success`, `failure` or `timeout`.
    pub fn plugin_run(&self, plugin: &str, event: &str, result: &str, took: Duration) {
        self.plugin_runs
            .with_label_values(&[plugin, event, result])
            .inc();
        self.plugin_duration
            .with_label_values(&[plugin])
            .observe(took.as_secs_f64());
    }

    /// Add `n` to one of a plugin's own counters (`putio.metrics.count`).
    pub fn plugin_counter(&self, plugin: &str, name: &str, n: u64) {
        self.plugin_counter
            .with_label_values(&[plugin, name])
            .inc_by(n);
    }

    /// Count events dropped because the plugin's queue was full.
    pub fn plugin_dropped(&self, plugin: &str, n: u64) {
        self.plugin_dropped.with_label_values(&[plugin]).inc_by(n);
    }
}

/// Create a plugin's series on the global instance.
pub fn record_configured(plugin: &str) {
    global().plugin_configured(plugin);
}

/// Record a plugin run on the global instance.
pub fn record_run(plugin: &str, event: &str, result: &str, took: Duration) {
    global().plugin_run(plugin, event, result, took);
}

/// Add to a plugin counter on the global instance.
pub fn record_counter(plugin: &str, name: &str, n: u64) {
    global().plugin_counter(plugin, name, n);
}

/// Count dropped events on the global instance.
pub fn record_dropped(plugin: &str, n: u64) {
    global().plugin_dropped(plugin, n);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::Telemetry;

    #[test]
    fn plugin_metrics_are_exposed() {
        let t = Telemetry::new();
        t.plugin_configured("ntfy");
        let before = t.encode();
        assert!(before.contains("putio_sync_plugin_events_dropped_total{plugin=\"ntfy\"} 0"));
        t.plugin_run(
            "ntfy",
            "download.finished",
            "success",
            Duration::from_millis(40),
        );
        t.plugin_run("ntfy", "tick", "timeout", Duration::from_secs(10));
        t.plugin_dropped("ntfy", 2);
        t.plugin_counter("plex", "refreshes", 3);
        let text = t.encode();
        assert!(
            text.contains("putio_sync_plugin_counter_total{name=\"refreshes\",plugin=\"plex\"} 3")
        );
        assert!(text.contains(
            "putio_sync_plugin_runs_total{event=\"download.finished\",plugin=\"ntfy\",result=\"success\"} 1"
        ));
        assert!(text.contains(
            "putio_sync_plugin_runs_total{event=\"tick\",plugin=\"ntfy\",result=\"timeout\"} 1"
        ));
        assert!(text.contains("putio_sync_plugin_run_duration_seconds_count{plugin=\"ntfy\"} 2"));
        assert!(text.contains("putio_sync_plugin_events_dropped_total{plugin=\"ntfy\"} 2"));
    }
}

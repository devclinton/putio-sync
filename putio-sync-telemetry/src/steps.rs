//! Processing pipeline metrics (feature 038).
//!
//! The labels are bounded: `stage` is one of the seven stages, `kind` one of five and
//! `result` one of five. Hook names and item keys never become labels.

use std::time::Duration;

use crate::{global, Telemetry};

/// The pipeline stages, as in the API
pub const STAGES: [&str; 7] = [
    "pre_download",
    "downloading",
    "pre_processing",
    "subtitles",
    "translating",
    "post_processing",
    "notifications",
];

/// The step kinds
pub const KINDS: [&str; 5] = ["hook", "download", "notify", "subtitle_scan", "subtitles"];

/// How a step run ended
pub const RESULTS: [&str; 5] = ["done", "failed", "timeout", "skip", "interrupted"];

impl Telemetry {
    /// Record one finished step run.
    pub fn pipeline_step(&self, stage: &str, kind: &str, result: &str, took: Duration) {
        self.pipeline_steps
            .with_label_values(&[stage, kind, result])
            .inc();
        self.pipeline_step_seconds
            .with_label_values(&[stage, kind])
            .observe(took.as_secs_f64());
    }

    /// Set how many items wait for the operator at `stage`.
    pub fn set_pipeline_waiting(&self, stage: &str, n: usize) {
        self.pipeline_items_waiting
            .with_label_values(&[stage])
            .set(n as i64);
    }
}

/// Record a finished step run on the global instance.
pub fn step(stage: &str, kind: &str, result: &str, took: Duration) {
    global().pipeline_step(stage, kind, result, took);
}

/// Set the waiting gauge on the global instance.
pub fn set_waiting(stage: &str, n: usize) {
    global().set_pipeline_waiting(stage, n);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_counted_by_stage_kind_and_result() {
        let t = Telemetry::new();
        t.pipeline_step(
            "post_processing",
            "hook",
            "failed",
            Duration::from_millis(5),
        );
        let text = t.encode();
        assert!(
            text.contains(
                "pipeline_steps_total{kind=\"hook\",result=\"failed\",stage=\"post_processing\"} 1"
            ),
            "{text}"
        );
        assert!(text.contains("pipeline_items_waiting{stage=\"subtitles\"} 0"));
    }
}

//! Subtitle translation metrics (feature 026).
//!
//! Every label is from a fixed set: `kind` is `chat` or `callback`, `result` and `reason`
//! are named below, and `language` is a configured target language or `other` (the caller
//! bounds it). File names, ids and tokens never become labels.

use std::time::Duration;

use crate::{global, Telemetry};

/// How one translation ended
pub const RESULTS: [&str; 4] = ["done", "failed", "timeout", "refused"];

/// How one chat chunk ended
pub const CHUNK_RESULTS: [&str; 3] = ["ok", "split", "failed"];

/// Why a callback was refused
pub const REFUSED_REASONS: [&str; 6] = [
    "unknown",
    "expired",
    "used",
    "bad_token",
    "bad_body",
    "rate_limited",
];

impl Telemetry {
    /// Count one finished translation and its time. `language` must already be bounded.
    pub fn translation_done(&self, kind: &str, result: &str, language: &str, took: Duration) {
        self.translations
            .with_label_values(&[kind, result, language])
            .inc();
        self.translation_seconds
            .with_label_values(&[kind])
            .observe(took.as_secs_f64());
    }

    /// Count one chat chunk by how it ended
    pub fn translation_chunk(&self, result: &str) {
        self.translation_chunks.with_label_values(&[result]).inc();
    }

    /// Count a chat request repeated after a bad reply
    pub fn translation_chunk_retry(&self) {
        self.translation_chunk_retries.inc();
    }

    /// Count an HTTP 429 answer
    pub fn translation_rate_limited(&self) {
        self.translation_rate_limited.inc();
    }

    /// Set how many callback jobs wait
    pub fn set_translation_callbacks_waiting(&self, n: usize) {
        self.translation_callbacks_waiting.set(n as i64);
    }

    /// Count a refused callback
    pub fn translation_callback_refused(&self, reason: &str) {
        self.translation_callbacks_refused
            .with_label_values(&[reason])
            .inc();
    }
}

/// Count one finished translation on the global instance
pub fn done(kind: &str, result: &str, language: &str, took: Duration) {
    global().translation_done(kind, result, language, took);
}

/// Count one chat chunk on the global instance
pub fn chunk(result: &str) {
    global().translation_chunk(result);
}

/// Count a repeated chat request on the global instance
pub fn chunk_retry() {
    global().translation_chunk_retry();
}

/// Count an HTTP 429 on the global instance
pub fn rate_limited() {
    global().translation_rate_limited();
}

/// Set the waiting-callbacks gauge on the global instance
pub fn set_callbacks_waiting(n: usize) {
    global().set_translation_callbacks_waiting(n);
}

/// Count a refused callback on the global instance
pub fn callback_refused(reason: &str) {
    global().translation_callback_refused(reason);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translations_are_counted_by_kind_result_and_language() {
        let t = Telemetry::new();
        t.translation_done("chat", "done", "es", Duration::from_secs(12));
        t.translation_done("chat", "done", "es", Duration::from_secs(3));
        t.translation_done("callback", "timeout", "other", Duration::from_secs(60));
        let text = t.encode();
        assert!(
            text.contains("translations_total{kind=\"chat\",language=\"es\",result=\"done\"} 2"),
            "{text}"
        );
        assert!(text.contains(
            "translations_total{kind=\"callback\",language=\"other\",result=\"timeout\"} 1"
        ));
        assert!(
            text.contains("translation_seconds_count{kind=\"chat\"} 2"),
            "{text}"
        );
    }

    #[test]
    fn chunks_retries_and_rate_limits_are_counted() {
        let t = Telemetry::new();
        t.translation_chunk("ok");
        t.translation_chunk("split");
        t.translation_chunk_retry();
        t.translation_chunk_retry();
        t.translation_rate_limited();
        assert_eq!(
            t.sample(
                "putio_sync_translation_chunks_total",
                &[("result", "split")]
            ),
            Some(1.0)
        );
        assert_eq!(
            t.sample("putio_sync_translation_chunk_retries_total", &[]),
            Some(2.0)
        );
        assert_eq!(
            t.sample("putio_sync_translation_rate_limited_total", &[]),
            Some(1.0)
        );
    }

    #[test]
    fn callbacks_waiting_and_refused_have_named_labels() {
        let t = Telemetry::new();
        t.set_translation_callbacks_waiting(3);
        t.translation_callback_refused("unknown");
        t.translation_callback_refused("unknown");
        assert_eq!(
            t.sample("putio_sync_translation_callbacks_waiting", &[]),
            Some(3.0)
        );
        assert_eq!(
            t.sample(
                "putio_sync_translation_callbacks_refused_total",
                &[("reason", "unknown")]
            ),
            Some(2.0)
        );
    }

    #[test]
    fn step_kinds_include_the_translation_steps() {
        let text = Telemetry::new().encode();
        assert!(text.contains("kind=\"translate_scan\""), "{text}");
        assert!(text.contains("kind=\"translate\""), "{text}");
    }

    #[test]
    fn label_sets_are_fixed() {
        assert_eq!(RESULTS.len(), 4);
        assert_eq!(CHUNK_RESULTS.len(), 3);
        assert_eq!(REFUSED_REASONS.len(), 6);
    }
}

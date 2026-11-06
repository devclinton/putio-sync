//! Subtitle metrics (feature 025).
//!
//! `source` is one of a fixed set of values and `language` a configured language or `other`, so
//! the label set stays bounded. File names and ids never become labels. Feature 050 adds the
//! OpenSubtitles API sources, `subtitles_skipped_total` and `opensubtitles_requests_total`.

use crate::{global, Telemetry};

/// Where a saved subtitle came from, as the `source` label. `opensubtitles` is OpenSubtitles
/// through Put.io's list; `os_hash`, `os_ids` and `os_name` are the API levels (feature 050).
pub const SOURCES: [&str; 7] = [
    "putio_embedded",
    "putio_folder",
    "opensubtitles",
    "os_hash",
    "os_ids",
    "os_name",
    "transcribed",
];

/// Why a match was not saved, as the `reason` label
pub const SKIP_REASONS: [&str; 6] = [
    "below_threshold",
    "quota",
    "no_result",
    "no_ids",
    "no_name",
    "not_configured",
];

/// OpenSubtitles endpoints, as the `endpoint` label
pub const ENDPOINTS: [&str; 3] = ["search", "download", "login"];

/// OpenSubtitles request results, as the `result` label
pub const RESULTS: [&str; 4] = ["ok", "rate_limited", "quota", "error"];

/// `value` when it is one of `allowed`, else `fallback`, so a typo never makes a new series
fn bounded<'a>(value: &'a str, allowed: &[&'a str], fallback: &'a str) -> &'a str {
    allowed
        .iter()
        .copied()
        .find(|a| *a == value)
        .unwrap_or(fallback)
}

impl Telemetry {
    /// Count one subtitle saved. `language` must already be bounded by the caller.
    pub fn subtitle_saved(&self, source: &str, language: &str) {
        self.subtitles_saved
            .with_label_values(&[source, language])
            .inc();
    }

    /// Count one match that was not saved. An unknown `reason` counts as `no_result`.
    pub fn subtitle_skipped(&self, reason: &str) {
        let reason = bounded(reason, &SKIP_REASONS, "no_result");
        self.subtitles_skipped.with_label_values(&[reason]).inc();
    }

    /// Count one OpenSubtitles request. Unknown values count as `search` and `error`.
    pub fn opensubtitles_request(&self, endpoint: &str, result: &str) {
        let endpoint = bounded(endpoint, &ENDPOINTS, "search");
        let result = bounded(result, &RESULTS, "error");
        self.opensubtitles_requests
            .with_label_values(&[endpoint, result])
            .inc();
    }

    /// Add seconds of audio sent to the transcription server.
    pub fn subtitle_transcribed_seconds(&self, seconds: u64) {
        self.subtitles_transcribed_seconds.inc_by(seconds);
    }
}

/// Count one saved subtitle on the global instance.
pub fn saved(source: &str, language: &str) {
    global().subtitle_saved(source, language);
}

/// Count one skipped match on the global instance.
pub fn skipped(reason: &str) {
    global().subtitle_skipped(reason);
}

/// Count one OpenSubtitles request on the global instance.
pub fn opensubtitles_request(endpoint: &str, result: &str) {
    global().opensubtitles_request(endpoint, result);
}

/// Add transcribed audio seconds on the global instance.
pub fn transcribed_seconds(seconds: u64) {
    global().subtitle_transcribed_seconds(seconds);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_subtitles_are_counted_by_source_and_language() {
        let t = Telemetry::new();
        t.subtitle_saved("opensubtitles", "en");
        t.subtitle_saved("opensubtitles", "en");
        t.subtitle_saved("transcribed", "other");
        let text = t.encode();
        assert!(
            text.contains("subtitles_saved_total{language=\"en\",source=\"opensubtitles\"} 2"),
            "{text}"
        );
        assert!(
            text.contains("subtitles_saved_total{language=\"other\",source=\"transcribed\"} 1"),
            "{text}"
        );
    }

    #[test]
    fn transcribed_seconds_accumulate() {
        let t = Telemetry::new();
        t.subtitle_transcribed_seconds(600);
        t.subtitle_transcribed_seconds(30);
        assert!(t
            .encode()
            .contains("subtitles_transcribed_seconds_total 630"));
        assert_eq!(
            t.sample("putio_sync_subtitles_transcribed_seconds_total", &[]),
            Some(630.0)
        );
    }

    #[test]
    fn kinds_include_the_subtitle_steps() {
        let t = Telemetry::new();
        let text = t.encode();
        assert!(text.contains("kind=\"subtitle_scan\""), "{text}");
        assert!(text.contains("kind=\"subtitles\""), "{text}");
    }

    #[test]
    fn api_levels_are_sources() {
        let t = Telemetry::new();
        for s in ["os_hash", "os_ids", "os_name"] {
            assert!(SOURCES.contains(&s));
            t.subtitle_saved(s, "en");
        }
        let text = t.encode();
        assert!(
            text.contains("subtitles_saved_total{language=\"en\",source=\"os_ids\"} 1"),
            "{text}"
        );
    }

    #[test]
    fn skips_and_requests_have_bounded_labels() {
        let t = Telemetry::new();
        t.subtitle_skipped("below_threshold");
        t.subtitle_skipped("below_threshold");
        t.subtitle_skipped("quota");
        t.subtitle_skipped("Show.Name.S01E01.mkv");
        t.opensubtitles_request("search", "ok");
        t.opensubtitles_request("download", "quota");
        t.opensubtitles_request("secret-endpoint", "boom");
        let text = t.encode();
        assert!(
            text.contains("subtitles_skipped_total{reason=\"below_threshold\"} 2"),
            "{text}"
        );
        assert!(
            text.contains("subtitles_skipped_total{reason=\"quota\"} 1"),
            "{text}"
        );
        assert!(
            text.contains("subtitles_skipped_total{reason=\"no_result\"} 1"),
            "{text}"
        );
        assert!(!text.contains("Show.Name"), "{text}");
        assert!(
            text.contains("opensubtitles_requests_total{endpoint=\"download\",result=\"quota\"} 1"),
            "{text}"
        );
        assert!(
            text.contains("opensubtitles_requests_total{endpoint=\"search\",result=\"error\"} 1"),
            "{text}"
        );
        assert!(
            !text.contains("secret-endpoint") && !text.contains("boom"),
            "{text}"
        );
    }
}

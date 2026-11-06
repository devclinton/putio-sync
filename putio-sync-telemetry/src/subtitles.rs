//! Subtitle metrics (feature 025).
//!
//! `source` is one of four values and `language` a configured language or `other`, so the
//! label set stays bounded. File names and ids never become labels.

use crate::{global, Telemetry};

/// Where a saved subtitle came from, as the `source` label
pub const SOURCES: [&str; 4] = [
    "putio_embedded",
    "putio_folder",
    "opensubtitles",
    "transcribed",
];

impl Telemetry {
    /// Count one subtitle saved. `language` must already be bounded by the caller.
    pub fn subtitle_saved(&self, source: &str, language: &str) {
        self.subtitles_saved
            .with_label_values(&[source, language])
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
}

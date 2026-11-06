//! The Whisper-style transcription client (feature 025): one multipart request per audio
//! chunk, the answer turned into cues, and the chunks joined into one timeline.
//!
//! See `specs/025-subtitles/contracts/transcription.md`. The key is sent as a Bearer token
//! only when set and is scrubbed from every message that leaves this module.

use std::time::{Duration, Instant};

use putio_sync_core::subtitles::TranscriptionConfig;
use reqwest::multipart::{Form, Part};
use serde_json::Value;

use crate::srt::{self, Cue};

/// Longest server message kept in an error
const MAX_MESSAGE: usize = 300;

/// Why a chunk could not be transcribed
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranscribeError {
    /// The server answered with a non-2xx status
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },
    /// The server could not be reached
    #[error("unreachable: {0}")]
    Unreachable(String),
    /// The request took longer than `timeout_secs`
    #[error("timeout after {0}s")]
    Timeout(u64),
    /// The answer is neither a transcription JSON nor an SRT
    #[error("unreadable response: {0}")]
    BadResponse(String),
    /// An SRT answer (or JSON without a language) and no language hint to name it by
    #[error("server did not report a language: set transcription.language")]
    NoLanguage,
}

/// What one chunk came to
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// Canonical language code, from the server or the hint
    pub language: Option<String>,
    /// Cues, times from the start of the chunk
    pub cues: Vec<Cue>,
    /// All the text, for the Test button
    pub text: String,
}

/// A server and how to talk to it
#[derive(Clone)]
pub struct Transcriber {
    endpoint: String,
    api_key: String,
    model: String,
    hint: String,
    timeout: Duration,
    timeout_secs: u64,
    client: reqwest::Client,
}

impl std::fmt::Debug for Transcriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transcriber")
            .field("endpoint", &putio_sync_core::redact::url(&self.endpoint))
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl Transcriber {
    /// A client for `config` (the env key override applies)
    pub fn new(config: &TranscriptionConfig) -> Self {
        Self {
            endpoint: config.endpoint(),
            api_key: config.effective_api_key(),
            model: config.model.trim().to_string(),
            hint: putio_sync_core::lang::lookup(&config.language).unwrap_or_default(),
            timeout: Duration::from_secs(config.timeout_secs),
            timeout_secs: config.timeout_secs,
            client: reqwest::Client::new(),
        }
    }

    /// A shorter timeout, for tests (the configured range starts at 10 seconds)
    #[doc(hidden)]
    pub fn with_timeout(mut self, timeout: Duration, reported_secs: u64) -> Self {
        self.timeout = timeout;
        self.timeout_secs = reported_secs;
        self
    }

    /// The configured language hint, canonical, or empty
    pub fn hint(&self) -> &str {
        &self.hint
    }

    /// Send one WAV. `language` is the hint for this chunk: the configured one, or the
    /// language chunk 1 reported.
    pub async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<&str>,
    ) -> Result<Transcript, TranscribeError> {
        let part = Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| TranscribeError::BadResponse(e.to_string()))?;
        let mut form = Form::new()
            .part("file", part)
            .text("response_format", "verbose_json");
        if !self.model.is_empty() {
            form = form.text("model", self.model.clone());
        }
        if let Some(l) = language.filter(|l| !l.is_empty()) {
            form = form.text("language", l.to_string());
        }
        let mut req = self
            .client
            .post(&self.endpoint)
            .timeout(self.timeout)
            .multipart(form);
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        let resp = req.send().await.map_err(|e| self.send_error(e))?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| self.send_error(e))?;
        if !status.is_success() {
            return Err(TranscribeError::Http {
                status: status.as_u16(),
                message: self.scrub(&server_message(&bytes)),
            });
        }
        parse_response(&bytes, language.or(Some(self.hint.as_str())))
    }

    fn send_error(&self, e: reqwest::Error) -> TranscribeError {
        if e.is_timeout() {
            return TranscribeError::Timeout(self.timeout_secs);
        }
        TranscribeError::Unreachable(self.scrub(&e.without_url().to_string()))
    }

    /// The text with the key (and its URL-ish forms) removed, cut to a short message
    pub fn scrub(&self, text: &str) -> String {
        let mut s = text.to_string();
        if !self.api_key.is_empty() {
            s = s.replace(&self.api_key, "********");
        }
        s.chars().take(MAX_MESSAGE).collect()
    }

    /// What the Test button shows: the answer to a short tone
    pub async fn probe(&self, wav: Vec<u8>) -> ServerTest {
        let started = Instant::now();
        let result = self.transcribe(wav, Some(self.hint.as_str())).await;
        let took_ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(t) => ServerTest {
                ok: true,
                status: Some(200),
                took_ms,
                language: t.language,
                text: t.text,
                error: None,
            },
            // A server that answers an SRT or JSON without a language still works
            Err(TranscribeError::NoLanguage) => ServerTest {
                ok: true,
                status: Some(200),
                took_ms,
                language: None,
                text: String::new(),
                error: None,
            },
            Err(TranscribeError::Http { status, message }) => ServerTest {
                ok: false,
                status: Some(status),
                took_ms,
                language: None,
                text: String::new(),
                error: Some(format!("HTTP {status}: {message}")),
            },
            Err(e) => ServerTest {
                ok: false,
                status: None,
                took_ms,
                language: None,
                text: String::new(),
                error: Some(e.to_string()),
            },
        }
    }
}

/// The server half of the Test button's answer
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ServerTest {
    #[serde(skip)]
    pub ok: bool,
    pub status: Option<u16>,
    pub took_ms: u64,
    pub language: Option<String>,
    pub text: String,
    pub error: Option<String>,
}

/// The message in an error body: `{"error": {"message": ...}}`, `{"error": "..."}`,
/// `{"detail": ...}` or the plain text
fn server_message(body: &[u8]) -> String {
    if let Ok(v) = serde_json::from_slice::<Value>(body) {
        let pick = |v: &Value| v.as_str().map(str::to_string);
        if let Some(m) = v
            .pointer("/error/message")
            .and_then(pick)
            .or_else(|| v.get("error").and_then(pick))
            .or_else(|| v.get("message").and_then(pick))
            .or_else(|| v.get("detail").and_then(pick))
        {
            return m;
        }
    }
    let text = String::from_utf8_lossy(body);
    let text = text.trim();
    if text.is_empty() {
        "no message".to_string()
    } else {
        text.to_string()
    }
}

/// Turn an answer into cues: the JSON shape first, an SRT body second
pub fn parse_response(body: &[u8], hint: Option<&str>) -> Result<Transcript, TranscribeError> {
    let hint = hint.filter(|h| !h.is_empty()).map(str::to_string);
    if let Ok(v) = serde_json::from_slice::<Value>(body) {
        if v.is_object() {
            return from_json(&v, hint);
        }
    }
    let text = String::from_utf8_lossy(body).into_owned();
    let cues = srt::parse(&text);
    if cues.is_empty() {
        let head: String = text.chars().take(80).collect();
        return Err(TranscribeError::BadResponse(format!(
            "not JSON and not SRT: {}",
            head.trim()
        )));
    }
    let language = hint.ok_or(TranscribeError::NoLanguage)?;
    let text = cues
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Transcript {
        language: Some(language),
        cues,
        text,
    })
}

fn from_json(v: &Value, hint: Option<String>) -> Result<Transcript, TranscribeError> {
    let reported = v
        .get("language")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .and_then(putio_sync_core::lang::lookup);
    let segments = v.get("segments").and_then(Value::as_array);
    let mut cues = Vec::new();
    for s in segments.into_iter().flatten() {
        let text = s
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        let (Some(start), Some(end)) = (
            s.get("start").and_then(Value::as_f64),
            s.get("end").and_then(Value::as_f64),
        ) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let start_ms = (start.max(0.0) * 1000.0).round() as u64;
        let end_ms = ((end.max(0.0) * 1000.0).round() as u64).max(start_ms + 1);
        cues.push(Cue {
            start_ms,
            end_ms,
            text: text.to_string(),
        });
    }
    let text = v
        .get("text")
        .and_then(Value::as_str)
        .map(|t| t.trim().to_string())
        .unwrap_or_else(|| {
            cues.iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        });
    let language = reported.or(hint);
    if language.is_none() && !cues.is_empty() {
        return Err(TranscribeError::NoLanguage);
    }
    Ok(Transcript {
        language,
        cues,
        text,
    })
}

/// One chunk's cues and where the chunk sits in the video, milliseconds
#[derive(Debug, Clone)]
pub struct ChunkCues {
    pub start_ms: u64,
    pub end_ms: u64,
    pub cues: Vec<Cue>,
}

/// Join chunks into one timeline: times shifted by the chunk start, ends clamped to the chunk
/// end, a start before the previous end moved to it, empty cues dropped, cues renumbered by
/// [`srt::render`]
pub fn join(parts: &[ChunkCues]) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    let mut prev_end = 0u64;
    for part in parts {
        for cue in &part.cues {
            let mut start = part.start_ms + cue.start_ms;
            let end = (part.start_ms + cue.end_ms).min(part.end_ms);
            if start < prev_end {
                start = prev_end;
            }
            if start >= end {
                continue;
            }
            prev_end = end;
            out.push(Cue {
                start_ms: start,
                end_ms: end,
                text: cue.text.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(s: u64, e: u64, t: &str) -> Cue {
        Cue {
            start_ms: s,
            end_ms: e,
            text: t.to_string(),
        }
    }

    #[test]
    fn json_segments_become_cues_and_the_language_is_mapped() {
        let body = br#"{"language":"english","duration":9.0,"text":" Hello there.",
            "segments":[{"start":0.0,"end":4.2,"text":" Hello."},{"start":4.2,"end":5.0,"text":"  "},
                        {"start":5.0,"end":8.5,"text":" There."}]}"#;
        let t = parse_response(body, None).unwrap();
        assert_eq!(t.language.as_deref(), Some("en"));
        assert_eq!(
            t.cues,
            vec![cue(0, 4200, "Hello."), cue(5000, 8500, "There.")]
        );
    }

    #[test]
    fn no_segments_means_no_speech_not_an_error() {
        let t = parse_response(br#"{"language":"en","text":"","segments":[]}"#, None).unwrap();
        assert!(t.cues.is_empty());
        let t = parse_response(br#"{"text":""}"#, None).unwrap();
        assert!(t.cues.is_empty() && t.language.is_none());
    }

    #[test]
    fn an_srt_body_needs_the_hint_for_its_language() {
        let body = b"1\n00:00:01,000 --> 00:00:02,000\nHi\n";
        assert_eq!(parse_response(body, None), Err(TranscribeError::NoLanguage));
        let t = parse_response(body, Some("fr")).unwrap();
        assert_eq!(t.language.as_deref(), Some("fr"));
        assert_eq!(t.cues.len(), 1);
    }

    #[test]
    fn json_with_cues_but_no_language_needs_the_hint() {
        let body = br#"{"segments":[{"start":0,"end":1,"text":"x"}]}"#;
        assert_eq!(parse_response(body, None), Err(TranscribeError::NoLanguage));
        assert!(parse_response(body, Some("de")).is_ok());
    }

    #[test]
    fn garbage_is_a_bad_response() {
        let e = parse_response(b"<html>oops</html>", None).unwrap_err();
        assert!(matches!(e, TranscribeError::BadResponse(_)), "{e}");
    }

    #[test]
    fn error_messages_come_from_the_usual_places() {
        assert_eq!(
            server_message(br#"{"error":{"message":"bad model"}}"#),
            "bad model"
        );
        assert_eq!(server_message(br#"{"detail":"nope"}"#), "nope");
        assert_eq!(server_message(b"plain"), "plain");
        assert_eq!(server_message(b""), "no message");
    }

    #[test]
    fn three_chunks_join_without_gaps_or_overlaps() {
        let parts = vec![
            ChunkCues {
                start_ms: 0,
                end_ms: 600_000,
                cues: vec![cue(1_000, 4_000, "a"), cue(597_000, 603_000, "b")],
            },
            ChunkCues {
                start_ms: 600_000,
                end_ms: 1_200_000,
                cues: vec![cue(0, 2_000, "c"), cue(1_900, 3_000, "d")],
            },
            ChunkCues {
                start_ms: 1_200_000,
                end_ms: 1_500_000,
                cues: vec![cue(10_000, 12_000, "e")],
            },
        ];
        let joined = join(&parts);
        let got: Vec<(u64, u64, &str)> = joined
            .iter()
            .map(|c| (c.start_ms, c.end_ms, c.text.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (1_000, 4_000, "a"),
                (597_000, 600_000, "b"),
                (600_000, 602_000, "c"),
                (602_000, 603_000, "d"),
                (1_210_000, 1_212_000, "e"),
            ]
        );
        for w in joined.windows(2) {
            assert!(w[0].end_ms <= w[1].start_ms, "overlap {w:?}");
        }
        let text = srt::render(&joined);
        assert!(
            text.starts_with("1\n00:00:01,000 --> 00:00:04,000\na\n"),
            "{text}"
        );
        assert!(
            text.contains("\n5\n00:20:10,000 --> 00:20:12,000\ne\n"),
            "{text}"
        );
    }

    #[test]
    fn a_cue_wholly_past_its_chunk_is_dropped() {
        let parts = vec![ChunkCues {
            start_ms: 0,
            end_ms: 60_000,
            cues: vec![cue(61_000, 62_000, "late")],
        }];
        assert!(join(&parts).is_empty());
    }
}

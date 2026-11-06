//! The callback translator's pure parts (research R6): the one-use token and its hash, the
//! callback URL, the classification of the service's reply, and the parse of the body the
//! service sends back.
//!
//! The plain token exists only in memory, inside the URL sent to the service. Only its
//! SHA-256 is stored.

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Characters of a service's error body kept in a message
pub const MAX_ERROR_CHARS: usize = 300;

/// 64 hex characters from two random UUIDs (244 random bits)
pub fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// A fresh job id, which names the job in logs and is not a credential
pub fn new_job_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Hex SHA-256 of a token: what is stored
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex SHA-256 of bytes (a source subtitle)
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `token` could be one of ours: 64 hex characters
pub fn is_well_formed(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `<base>/translate/callback/<token>`; a trailing `/` on the base is ignored and a path
/// prefix is kept
pub fn callback_url(base: &str, token: &str) -> String {
    format!(
        "{}/translate/callback/{token}",
        base.trim().trim_end_matches('/')
    )
}

/// The JSON body of the outbound request (FR-013)
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct JobRequest<'a> {
    /// Names the job in logs
    pub job_id: &'a str,
    /// Canonical code of the subtitle sent
    pub source_lang: &'a str,
    /// Canonical code wanted back
    pub target_lang: &'a str,
    /// The video's file name, no directories
    pub video_name: &'a str,
    /// The subtitle as SRT
    pub srt: &'a str,
    /// Where to POST the result
    pub callback_url: &'a str,
}

/// What a body holds
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// Subtitle text
    Srt(String),
    /// The service says it failed
    Error(String),
}

/// Read a body that carries a subtitle: JSON `{"srt": "..."}` or `{"error": "..."}` when
/// the content type says json or the body starts with `{`, else the text itself. `Err` is a
/// message for a body that is JSON without `srt`, or empty.
pub fn parse_body(content_type: Option<&str>, body: &str) -> Result<Parsed, String> {
    let is_json = content_type
        .map(|c| c.to_ascii_lowercase().contains("json"))
        .unwrap_or(false)
        || body.trim_start().starts_with('{');
    if is_json {
        let value: serde_json::Value =
            serde_json::from_str(body).map_err(|_| "the reply was not valid JSON".to_string())?;
        if let Some(error) = value.get("error") {
            let message = match error {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            return Ok(Parsed::Error(message));
        }
        return match value.get("srt").and_then(|s| s.as_str()) {
            Some(srt) if !srt.trim().is_empty() => Ok(Parsed::Srt(srt.to_string())),
            _ => Err("the reply had no srt".to_string()),
        };
    }
    if body.trim().is_empty() {
        return Err("the reply was empty".to_string());
    }
    Ok(Parsed::Srt(body.to_string()))
}

/// What the service answered to the outbound request
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The translated subtitle, straight away
    Immediate(String),
    /// Accepted: the result will arrive at the callback URL
    Accepted,
    /// Failed, with a message that is safe to show
    Failed(String),
}

/// Classify an HTTP answer (research R6). `scrubbed_body` is the body with secrets removed
/// and cut short; it is used only in failure messages.
pub fn classify_reply(
    status: u16,
    content_type: Option<&str>,
    body: &str,
    scrubbed_body: &str,
) -> Reply {
    match status {
        200 => match parse_body(content_type, body) {
            Ok(Parsed::Srt(srt)) => Reply::Immediate(srt),
            Ok(Parsed::Error(message)) => Reply::Failed(format!("the service said: {message}")),
            Err(message) => Reply::Failed(message),
        },
        202 => Reply::Accepted,
        200..=299 => Reply::Failed(format!("unexpected HTTP {status} (expected 200 or 202)")),
        300..=399 => Reply::Failed(format!(
            "HTTP {status}: redirects are not followed; use the final URL"
        )),
        _ => Reply::Failed(format!("HTTP {status}: {scrubbed_body}")),
    }
}

/// `text` without any of `secrets` or anything `log_redact` knows, cut to `max` characters
pub fn scrub(text: &str, secrets: &[String], max: usize) -> String {
    let mut out = text.to_string();
    for s in secrets {
        if s.chars().count() >= 4 {
            out = out.replace(s.as_str(), "********");
        }
    }
    let out = putio_sync_core::log_redact::redact(&out);
    let out: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > max {
        let cut: String = out.chars().take(max).collect();
        format!("{cut}…")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_64_hex_characters_and_differ() {
        let a = new_token();
        let b = new_token();
        assert_eq!(a.len(), 64);
        assert!(is_well_formed(&a));
        assert_ne!(a, b);
        assert!(!is_well_formed("abc"));
        assert!(!is_well_formed(&"g".repeat(64)));
        assert!(!is_well_formed(&format!("{a}0")));
    }

    #[test]
    fn the_hash_is_sha256_hex() {
        // SHA-256("abc")
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let t = new_token();
        assert_eq!(hash_token(&t).len(), 64);
        assert_ne!(hash_token(&t), t);
        assert_eq!(hash_token(&t), hash_token(&t));
        assert_eq!(sha256_hex(b"abc"), hash_token("abc"));
    }

    #[test]
    fn the_url_keeps_a_path_prefix_and_drops_a_trailing_slash() {
        let t = "t".repeat(64);
        assert_eq!(
            callback_url("http://nas.lan:8088", &t),
            format!("http://nas.lan:8088/translate/callback/{t}")
        );
        assert_eq!(
            callback_url("https://x.example/putio/", &t),
            format!("https://x.example/putio/translate/callback/{t}")
        );
    }

    #[test]
    fn job_requests_serialize_with_the_documented_fields() {
        let body = JobRequest {
            job_id: "j",
            source_lang: "en",
            target_lang: "es",
            video_name: "M.mkv",
            srt: "1\n",
            callback_url: "http://h/translate/callback/t",
        };
        let v: serde_json::Value = serde_json::to_value(&body).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "callback_url",
                "job_id",
                "source_lang",
                "srt",
                "target_lang",
                "video_name"
            ]
        );
    }

    #[test]
    fn bodies_parse_as_text_or_json() {
        assert_eq!(
            parse_body(Some("text/plain"), "1\n00:00:01,000 --> 00:00:02,000\nHi\n"),
            Ok(Parsed::Srt("1\n00:00:01,000 --> 00:00:02,000\nHi\n".into()))
        );
        assert_eq!(
            parse_body(Some("application/x-subrip"), "x"),
            Ok(Parsed::Srt("x".into()))
        );
        assert_eq!(parse_body(None, "x"), Ok(Parsed::Srt("x".into())));
        assert_eq!(
            parse_body(Some("application/json"), r#"{"srt":"1\n"}"#),
            Ok(Parsed::Srt("1\n".into()))
        );
        // JSON without the content type is still recognised by its first character
        assert_eq!(
            parse_body(Some("text/plain"), r#" {"srt":"1\n"}"#),
            Ok(Parsed::Srt("1\n".into()))
        );
        assert_eq!(
            parse_body(Some("application/json"), r#"{"error":"model busy"}"#),
            Ok(Parsed::Error("model busy".into()))
        );
        assert_eq!(
            parse_body(None, r#"{"error":{"code":5}}"#),
            Ok(Parsed::Error(r#"{"code":5}"#.into()))
        );
        assert!(parse_body(Some("application/json"), r#"{"other":1}"#).is_err());
        assert!(parse_body(Some("application/json"), r#"{"srt":""}"#).is_err());
        assert!(parse_body(Some("application/json"), "not json").is_err());
        assert!(parse_body(None, "  \n").is_err());
    }

    #[test]
    fn replies_are_classified_by_status() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nHola\n";
        assert_eq!(
            classify_reply(200, Some("text/plain"), srt, ""),
            Reply::Immediate(srt.into())
        );
        assert_eq!(
            classify_reply(200, Some("application/json"), r#"{"srt":"x"}"#, ""),
            Reply::Immediate("x".into())
        );
        assert_eq!(
            classify_reply(200, Some("application/json"), r#"{"error":"bad"}"#, ""),
            Reply::Failed("the service said: bad".into())
        );
        assert_eq!(
            classify_reply(200, Some("application/json"), r#"{"x":1}"#, ""),
            Reply::Failed("the reply had no srt".into())
        );
        assert_eq!(classify_reply(202, None, "", ""), Reply::Accepted);
        assert_eq!(
            classify_reply(204, None, "", ""),
            Reply::Failed("unexpected HTTP 204 (expected 200 or 202)".into())
        );
        match classify_reply(302, None, "", "") {
            Reply::Failed(m) => assert!(m.contains("redirects are not followed"), "{m}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            classify_reply(503, None, "<html>", "busy now"),
            Reply::Failed("HTTP 503: busy now".into())
        );
        assert_eq!(
            classify_reply(401, None, "", "no"),
            Reply::Failed("HTTP 401: no".into())
        );
    }

    #[test]
    fn scrubbing_removes_secrets_and_cuts_long_text() {
        let out = scrub(
            "bad key sk-secret-1234 in header  Bearer abcdefghij",
            &["sk-secret-1234".to_string()],
            300,
        );
        assert!(!out.contains("sk-secret-1234"), "{out}");
        assert!(!out.contains("abcdefghij"), "{out}");
        assert!(out.contains("********"));
        let long = "x".repeat(1000);
        let out = scrub(&long, &[], 300);
        assert_eq!(out.chars().count(), 301);
        assert!(out.ends_with('…'));
        // whitespace is collapsed so a multi-line body stays on one line
        assert_eq!(scrub("a\n\n b\t c", &[], 300), "a b c");
        // a very short "secret" is not replaced everywhere
        assert_eq!(scrub("a b", &["a".to_string()], 300), "a b");
    }
}

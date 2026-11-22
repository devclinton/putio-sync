//! Shortened forms of values that may carry secrets (feature 027)
//!
//! A webhook URL can hold a token in its path or query string, so logs, errors, the
//! doctor and the API show it through [`url`] only.

use reqwest::Url;

/// A URL cut down to its scheme, host and port, with `/…` when it has a path or query
/// string. The path, query, fragment and user info are dropped, since any of them may
/// hold a token.
///
/// ```
/// use putio_sync_core::redact;
///
/// assert_eq!(redact::url("https://u:pw@h.lan/api/webhook/abc?token=x"), "https://h.lan/…");
/// ```
pub fn url(raw: &str) -> String {
    let Ok(parsed) = Url::parse(raw) else {
        return "<invalid url>".to_string();
    };
    let Some(host) = parsed.host_str() else {
        return "<invalid url>".to_string();
    };
    let mut out = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        out.push_str(&format!(":{}", port));
    }
    // Any part of the path or query can be the secret (Telegram's `/bot<id>:<token>/…`,
    // Home Assistant's `/api/webhook/<id>`), so none of it is kept
    let has_path = parsed
        .path_segments()
        .into_iter()
        .flatten()
        .any(|segment| !segment.is_empty());
    if has_path || parsed.query().is_some() {
        out.push_str("/…");
    }
    out
}

/// `text` with every `http://` or `https://` URL in it shortened by [`url`], for error
/// messages that may quote one (such as `reqwest::Error`'s).
pub fn text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = ["http://", "https://"]
        .iter()
        .filter_map(|scheme| rest.find(scheme))
        .min()
    {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '"' | '\'' | '>'))
            .unwrap_or(tail.len());
        out.push_str(&url(&tail[..end]));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::{text, url};

    #[test]
    fn urls_inside_text_are_shortened() {
        assert_eq!(
            text("error sending request for url (https://cdn.put.io/a/b?sig=xyz): timed out"),
            "error sending request for url (https://cdn.put.io/…): timed out"
        );
        assert_eq!(text("no urls here"), "no urls here");
        assert_eq!(
            text("http://h/x?t=1 and https://g/y"),
            "http://h/… and https://g/…"
        );
    }

    #[test]
    fn keeps_scheme_host_and_port_only() {
        assert_eq!(
            url("https://ha.local:8123/api/webhook/abc"),
            "https://ha.local:8123/…"
        );
        assert_eq!(url("http://10.0.0.2/hook"), "http://10.0.0.2/…");
        let telegram = url("https://api.telegram.org/bot123456:AAEsecretTOKEN/sendMessage");
        assert_eq!(telegram, "https://api.telegram.org/…");
        assert!(!telegram.contains("secret") && !telegram.contains("bot"));
        assert_eq!(url("https://example.com"), "https://example.com");
        assert_eq!(url("https://example.com/"), "https://example.com");
    }

    #[test]
    fn drops_user_info_query_and_fragment() {
        let out = url("https://user:pw@h/a/b?token=x#frag");
        assert_eq!(out, "https://h/…");
        assert!(!out.contains("pw") && !out.contains("token"));
        assert_eq!(url("https://h/hook?token=secret"), "https://h/…");
        assert_eq!(url("https://h?token=secret"), "https://h/…");
    }

    #[test]
    fn invalid_urls_say_so() {
        assert_eq!(url("not a url"), "<invalid url>");
        assert_eq!(url("mailto:someone@example.com"), "<invalid url>");
    }
}

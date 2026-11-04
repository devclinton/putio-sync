//! Shortened forms of values that may carry secrets (feature 027)
//!
//! A webhook URL can hold a token in its path or query string, so logs, errors, the
//! doctor and the API show it through [`url`] only.

use reqwest::Url;

/// A URL cut down to its scheme, host, port and first path segment, with `…` when the
/// path goes on. The query string, fragment and user info are dropped.
///
/// ```
/// use putio_sync_core::redact;
///
/// assert_eq!(redact::url("https://u:pw@h.lan/api/webhook/abc?token=x"), "https://h.lan/api/…");
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
    let mut segments = parsed
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty());
    match segments.next() {
        None => {}
        Some(first) => {
            out.push('/');
            out.push_str(first);
            if segments.next().is_some() {
                out.push_str("/…");
            }
        }
    }
    if parsed.query().is_some() && !out.ends_with('…') {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::url;

    #[test]
    fn keeps_scheme_host_port_and_first_segment() {
        assert_eq!(
            url("https://ha.local:8123/api/webhook/abc"),
            "https://ha.local:8123/api/…"
        );
        assert_eq!(url("http://10.0.0.2/hook"), "http://10.0.0.2/hook");
        assert_eq!(url("https://example.com"), "https://example.com");
        assert_eq!(url("https://example.com/"), "https://example.com");
    }

    #[test]
    fn drops_user_info_query_and_fragment() {
        let out = url("https://user:pw@h/a/b?token=x#frag");
        assert_eq!(out, "https://h/a/…");
        assert!(!out.contains("pw") && !out.contains("token"));
        assert_eq!(url("https://h/hook?token=secret"), "https://h/hook…");
    }

    #[test]
    fn invalid_urls_say_so() {
        assert_eq!(url("not a url"), "<invalid url>");
        assert_eq!(url("mailto:someone@example.com"), "<invalid url>");
    }
}

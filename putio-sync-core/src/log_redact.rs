//! Masking secrets in log lines before the Logs page keeps them (feature 047)
//!
//! Every captured line goes through [`redact`] (the message) and [`redact_field`] (each
//! field). Four passes, in order:
//!
//! 1. Values from the config and environment ([`set_secrets`]) are replaced wherever they
//!    appear.
//! 2. The value after `key=` or `key:` is masked when the key names a secret (`token`,
//!    `password`, `api_key`, `authorization`, …). Quoted values, `Some("…")` and `Bearer x`
//!    are handled.
//! 3. `Bearer x` and `Basic x` lose their credential anywhere else.
//! 4. URLs are shortened by [`crate::redact::text`], which drops query strings and user
//!    info.

use std::sync::RwLock;

/// What a masked value is replaced with, the same as hooks and plugins use
pub const MASK: &str = "********";

/// Shorter values are not matched as known secrets: they would mask ordinary words.
pub const MIN_SECRET: usize = 6;

/// Parts of a field or key name that mark its value as a secret
const SENSITIVE: [&str; 12] = [
    "token",
    "secret",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "api-key",
    "authorization",
    "cookie",
    "signature",
    "credential",
    "private_key",
];

/// Words that come before the credential in an `Authorization` value
const SCHEMES: [&str; 4] = ["bearer", "basic", "token", "digest"];

/// Known secret values, longest first
static SECRETS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Replace the known secret values. Values shorter than [`MIN_SECRET`] are ignored.
pub fn set_secrets(values: Vec<String>) {
    let mut values: Vec<String> = values
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| v.chars().count() >= MIN_SECRET)
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    values.dedup();
    *SECRETS.write().unwrap_or_else(|e| e.into_inner()) = values;
}

/// Whether a key or field name marks its value as a secret
pub fn is_sensitive(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SENSITIVE.iter().any(|s| name.contains(s))
}

/// `text` with every secret masked
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    {
        let secrets = SECRETS.read().unwrap_or_else(|e| e.into_inner());
        for secret in secrets.iter() {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), MASK);
            }
        }
    }
    let out = mask_keyed(&out);
    let out = mask_schemes(&out);
    crate::redact::text(&out)
}

/// A field's value, masked whole when the field's name marks a secret
///
/// `has_token = true` stays readable: a flag says nothing about the value.
pub fn redact_field(name: &str, value: &str) -> String {
    if is_sensitive(name) && !harmless(value) {
        return MASK.to_string();
    }
    redact(value)
}

/// Values that are not secrets even under a secret's name
fn harmless(value: &str) -> bool {
    let v = value.trim().trim_matches('"');
    v.is_empty()
        || v == MASK
        || ["true", "false", "none", "null"]
            .iter()
            .any(|w| v.eq_ignore_ascii_case(w))
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// Mask the value after each `key=` / `key:` whose key names a secret.
fn mask_keyed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while let Some(off) = text[i..].find(['=', ':']) {
        let sep = i + off;
        i = sep + 1;
        // `a::b` is a Rust path, not a key and value
        if text[sep..].starts_with("::") {
            i = sep + 2;
            continue;
        }
        if !is_sensitive(key_before(text, sep)) {
            continue;
        }
        let Some((start, end)) = value_after(text, sep + 1) else {
            continue;
        };
        if harmless(&text[start..end]) {
            i = end.max(i);
            continue;
        }
        out.push_str(&text[copied..start]);
        out.push_str(MASK);
        copied = end;
        i = end;
    }
    out.push_str(&text[copied..]);
    out
}

/// The key that ends just before `sep`, skipping a closing quote and spaces (`"token": x`,
/// `token = x`)
fn key_before(text: &str, sep: usize) -> &str {
    let head = text[..sep].trim_end_matches(' ');
    let head = head
        .strip_suffix('"')
        .or_else(|| head.strip_suffix('\''))
        .unwrap_or(head);
    let start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_key_char(*c))
        .last()
        .map(|(at, _)| at)
        .unwrap_or(head.len());
    &head[start..]
}

/// The span of the value starting at `from`, without its quotes
fn value_after(text: &str, from: usize) -> Option<(usize, usize)> {
    let rest = &text[from..];
    let mut start = from + (rest.len() - rest.trim_start_matches(' ').len());
    if text[start..].starts_with("Some(") {
        start += "Some(".len();
    }
    let rest = &text[start..];
    let first = rest.chars().next()?;
    if first == '"' || first == '\'' {
        let body = start + 1;
        let mut escaped = false;
        for (at, c) in text[body..].char_indices() {
            match c {
                '\\' if !escaped => escaped = true,
                c if c == first && !escaped => return Some((body, body + at)),
                _ => escaped = false,
            }
        }
        return Some((body, text.len()));
    }
    let word_end = |from: usize| {
        text[from..]
            .find(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '&' | ')' | '"' | '\''))
            .map_or(text.len(), |at| from + at)
    };
    let end = word_end(start);
    if end == start {
        return None;
    }
    // `Authorization: Bearer x` masks x too
    if SCHEMES
        .iter()
        .any(|s| text[start..end].eq_ignore_ascii_case(s))
        && text[end..].starts_with(' ')
    {
        let next = end + 1;
        let next_end = word_end(next);
        if next_end > next {
            return Some((start, next_end));
        }
    }
    Some((start, end))
}

/// Mask the word after `Bearer ` or `Basic `, wherever it appears.
fn mask_schemes(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i < text.len() {
        let hit = ["bearer ", "basic "]
            .iter()
            .filter_map(|s| lower[i..].find(s).map(|at| (i + at, s.len())))
            .min();
        let Some((at, len)) = hit else { break };
        let boundary = text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric());
        let start = at + len;
        let end = text[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ')' | '"' | '\''))
            .map_or(text.len(), |e| start + e);
        if boundary && end > start && &text[start..end] != MASK {
            out.push_str(&text[copied..start]);
            out.push_str(MASK);
            copied = end;
        }
        i = end.max(at + 1);
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises tests that set the global secret list
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_secrets<T>(values: &[&str], f: impl FnOnce() -> T) -> T {
        let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_secrets(values.iter().map(|v| v.to_string()).collect());
        let out = f();
        set_secrets(Vec::new());
        out
    }

    fn assert_masked(line: &str, secret: &str) {
        let out = redact(line);
        assert!(!out.contains(secret), "{secret} kept in {out}");
        assert!(out.contains(MASK), "nothing masked in {out}");
    }

    #[test]
    fn known_values_are_masked_anywhere() {
        // Put.io token, API token, app secret, arr password and key, hook header and env
        // values, plugin secret
        let secrets = [
            "putio-token-ABCDEF123",
            "api-token-0123456789",
            "client-secret-XYZ987",
            "arr-password-hunter2",
            "arrkey0123456789abcdef",
            "hook-header-value-77",
            "hook-env-value-88",
            "plugin-secret-99999",
        ];
        with_secrets(&secrets, || {
            for secret in secrets {
                assert_masked(&format!("loaded value {secret} for run"), secret);
                assert_masked(&format!("failed ({secret})"), secret);
            }
        });
    }

    #[test]
    fn short_known_values_are_ignored() {
        with_secrets(&["abc", "info"], || {
            assert_eq!(redact("level info abc"), "level info abc");
        });
    }

    #[test]
    fn webhook_and_hook_urls_lose_their_secret_parts() {
        with_secrets(&["https://ha.lan/api/webhook/abcdef123456"], || {
            assert_masked(
                "posting to https://ha.lan/api/webhook/abcdef123456 failed",
                "abcdef123456",
            );
        });
        // Not configured: the URL is still cut down to its host and first segment
        let out = redact("POST https://hooks.example/run/s3cr3tpath?key=zzz failed");
        assert!(!out.contains("s3cr3tpath") && !out.contains("zzz"), "{out}");
    }

    #[test]
    fn keyed_values_are_masked() {
        for (line, secret) in [
            ("Authorization: Bearer abc.def.ghi", "abc.def.ghi"),
            ("authorization=Basic dXNlcjpwdw==", "dXNlcjpwdw"),
            ("Cookie: session=deadbeef42", "deadbeef42"),
            ("request ?token=qwerty123&x=1", "qwerty123"),
            ("api_key=k3y-value", "k3y-value"),
            ("X-Api-Key: k3y-value", "k3y-value"),
            (
                "AuthConfig { token: \"tok-value\", user: None }",
                "tok-value",
            ),
            ("password: Some(\"pw-value\")", "pw-value"),
            ("{\"authorization\": \"Bearer zz-top\"}", "zz-top"),
            ("PUTIO_CLIENT_SECRET=cs-value", "cs-value"),
            ("client_secret = 'cs-value'", "cs-value"),
        ] {
            assert_masked(line, secret);
        }
    }

    #[test]
    fn bearer_credentials_are_masked_without_a_key() {
        assert_masked("sent header Bearer abcdef0123", "abcdef0123");
        assert_eq!(redact("basically fine"), "basically fine");
    }

    #[test]
    fn ordinary_text_is_left_alone() {
        for line in [
            "putio_sync_core::download finished",
            "has_token = true",
            "token_present=false",
            "at 12:30:05 file_id=42 size=10",
            "Log level changed old=info new=debug",
        ] {
            assert_eq!(redact(line), line);
        }
    }

    #[test]
    fn fields_with_secret_names_are_masked_whole() {
        assert_eq!(redact_field("api_key", "abc 123"), MASK);
        assert_eq!(redact_field("token", "\"x\""), MASK);
        assert_eq!(redact_field("has_token", "true"), "true");
        assert_eq!(redact_field("file_id", "42"), "42");
        assert_eq!(
            redact_field("error", "header authorization: Bearer xyz123"),
            format!("header authorization: {MASK}")
        );
    }
}

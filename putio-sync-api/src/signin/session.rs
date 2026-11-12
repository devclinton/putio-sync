//! Session cookies and CSRF tokens
//!
//! The cookie is `b64url(JSON payload) "." b64url(HMAC-SHA256(key, payload))`. Nothing
//! is stored on the server: the key signs it, and every request checks it against the
//! config again (the account still allowed, the password unchanged).
//!
//! A write sent with the cookie also needs `X-CSRF-Token`, an HMAC of the cookie that the
//! UI reads from `/auth/status`. SameSite=Strict already keeps other sites' forms out;
//! the header covers older browsers and same-site subdomains.

use axum::http::{header::COOKIE, HeaderMap};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// The session cookie
pub const COOKIE_NAME: &str = "putio_sync_session";

/// The OAuth browser-binding cookie, sent only to `/auth/oauth`
pub const FLOW_COOKIE: &str = "putio_sync_oauth";

/// The header a cookie-authenticated write must carry
pub const CSRF_HEADER: &str = "x-csrf-token";

/// A signed-in browser
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Payload version
    pub v: u8,
    /// How they signed in: "password", or a provider ("google", "github", "discord")
    pub how: String,
    /// The username, or the provider account's login (name or email)
    pub login: String,
    /// Display name
    #[serde(default)]
    pub name: String,
    /// Expiry, Unix seconds
    pub exp: u64,
    /// For password sessions, the password's stamp when it was issued
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stamp: String,
}

impl Session {
    /// A session for `how`/`login` lasting `hours`
    pub fn new(how: &str, login: &str, name: &str, hours: u32, stamp: String) -> Session {
        Session {
            v: 1,
            how: how.to_string(),
            login: login.to_string(),
            name: name.to_string(),
            exp: now() + u64::from(hours) * 3600,
            stamp,
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn mac(key: &[u8; 32], parts: &[&[u8]]) -> HmacSha256 {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC takes any key length");
    for part in parts {
        mac.update(part);
    }
    mac
}

/// The cookie value for `session`
pub fn sign(key: &[u8; 32], session: &Session) -> String {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(session).unwrap_or_default());
    let tag = mac(key, &[payload.as_bytes()]).finalize().into_bytes();
    format!("{payload}.{}", URL_SAFE_NO_PAD.encode(tag))
}

/// The session in a cookie value, when the signature matches and it hasn't expired
pub fn verify(key: &[u8; 32], value: &str) -> Option<Session> {
    let (payload, tag) = value.split_once('.')?;
    let tag = URL_SAFE_NO_PAD.decode(tag).ok()?;
    mac(key, &[payload.as_bytes()]).verify_slice(&tag).ok()?;
    let session: Session = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    (session.v == 1 && session.exp > now()).then_some(session)
}

/// The CSRF token for a session cookie value
pub fn csrf(key: &[u8; 32], cookie: &str) -> String {
    let tag = mac(key, &[b"csrf.", cookie.as_bytes()])
        .finalize()
        .into_bytes();
    tag[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `provided` is the CSRF token for `cookie`, compared in constant time
pub fn csrf_matches(key: &[u8; 32], cookie: &str, provided: &str) -> bool {
    let expected = csrf(key, cookie);
    expected.len() == provided.len()
        && expected
            .bytes()
            .zip(provided.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// The value of cookie `name` in the request's `Cookie` headers
pub fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
}

/// Whether the browser reached us over HTTPS (directly or through a proxy that says so)
pub fn is_secure(headers: &HeaderMap, public_url: Option<&str>) -> bool {
    if let Some(url) = public_url {
        return url.starts_with("https://");
    }
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').next().unwrap_or("").trim() == "https")
}

/// A `Set-Cookie` value for the session cookie lasting `hours`
pub fn set_cookie(value: &str, hours: u32, secure: bool) -> String {
    format!(
        "{COOKIE_NAME}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{}",
        u64::from(hours) * 3600,
        if secure { "; Secure" } else { "" }
    )
}

/// A `Set-Cookie` value that removes the session cookie
pub fn clear_cookie(secure: bool) -> String {
    format!(
        "{COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{}",
        if secure { "; Secure" } else { "" }
    )
}

/// A `Set-Cookie` value for the OAuth browser binding (`None` removes it)
pub fn flow_cookie(value: Option<&str>, secure: bool) -> String {
    format!(
        "{FLOW_COOKIE}={}; HttpOnly; SameSite=Lax; Path=/auth/oauth; Max-Age={}{}",
        value.unwrap_or(""),
        if value.is_some() { 600 } else { 0 },
        if secure { "; Secure" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn a_signed_session_verifies() {
        let session = Session::new("password", "admin", "", 1, "abcd".into());
        let value = sign(&KEY, &session);
        assert_eq!(verify(&KEY, &value), Some(session));
        assert_eq!(verify(&[8; 32], &value), None);
    }

    #[test]
    fn tampered_or_expired_sessions_fail() {
        let mut session = Session::new("github", "octocat", "Octo", 1, String::new());
        let value = sign(&KEY, &session);
        let (payload, tag) = value.split_once('.').unwrap();
        let mut forged = session.clone();
        forged.login = "admin".into();
        let forged_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&forged).unwrap());
        assert_eq!(verify(&KEY, &format!("{forged_payload}.{tag}")), None);
        assert_eq!(verify(&KEY, payload), None);
        assert_eq!(verify(&KEY, "garbage"), None);
        session.exp = 1;
        assert_eq!(verify(&KEY, &sign(&KEY, &session)), None);
    }

    #[test]
    fn csrf_tokens_belong_to_their_cookie() {
        let token = csrf(&KEY, "a.b");
        assert_eq!(token.len(), 32);
        assert!(csrf_matches(&KEY, "a.b", &token));
        assert!(!csrf_matches(&KEY, "a.c", &token));
        assert!(!csrf_matches(&KEY, "a.b", ""));
    }

    #[test]
    fn cookies_are_read_from_the_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("other=1; putio_sync_session=abc.def"),
        );
        assert_eq!(cookie(&headers, COOKIE_NAME), Some("abc.def"));
        assert_eq!(cookie(&headers, "missing"), None);
    }

    #[test]
    fn secure_follows_the_public_url_or_the_proxy() {
        let mut headers = HeaderMap::new();
        assert!(!is_secure(&headers, None));
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(is_secure(&headers, None));
        assert!(!is_secure(&headers, Some("http://nas.local")));
        assert!(is_secure(
            &HeaderMap::new(),
            Some("https://sync.example.com")
        ));
        assert!(set_cookie("v", 1, true).ends_with("Max-Age=3600; Secure"));
        assert!(clear_cookie(false).contains("Max-Age=0"));
    }
}

//! Authentication middleware
//!
//! This module provides authentication middleware for API endpoints.
//!
//! Every protected request must include:
//! - Authorization header with format: "Bearer <token>"
//!
//! A token is always required. When none is configured, every protected request is
//! refused with a body that names the setting. Failures are counted per client IP and
//! rate limited (see [`crate::middleware::ratelimit`]).
//!
//! With `[sign_in]` (feature 019), a request is also let in by HTTP Basic with the
//! configured username and password, or by a session cookie from the sign-in page. A
//! cookie-authenticated write needs the session's `X-CSRF-Token` too. The order is the
//! token, Basic, the cookie, then Tailscale.
//!
//! With `[tailscale] identity_sign_in` on, a request without a valid token is also let
//! in when its Tailscale identity is on `allowed_users` (feature 036). The identity is
//! attached to the request as a [`putio_sync_tailscale::Identity`] extension.

use crate::error::ApiError;
use crate::middleware::ratelimit::client_ip;
use crate::signin::{self, session};
use crate::AppState;
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header::AUTHORIZATION, HeaderMap, Method},
    middleware::Next,
    response::Response,
};
use putio_sync_tailscale::{Identity, Lookup};
use std::net::SocketAddr;
use tracing::{info, warn};

/// `tailscale serve`'s header with the user's login
pub const TAILSCALE_USER_LOGIN: &str = "Tailscale-User-Login";
/// `tailscale serve`'s header with the user's display name
pub const TAILSCALE_USER_NAME: &str = "Tailscale-User-Name";

/// Authentication middleware
///
/// This middleware checks for valid authentication tokens in requests.
///
/// - Refuses with 429 while the client IP is rate limited, before comparing anything
/// - Refuses with 401 when `api.auth_token` is not configured
/// - Otherwise validates the Authorization header ("Bearer <token>", or the raw token)
///
/// Only a token that doesn't match counts toward the limiter. A request without one
/// (or when no token is configured) gets its 401 without being counted.
///
/// # Examples
///
/// ```ignore
/// use axum::Router;
/// use axum::middleware;
/// use putio_sync_api::middleware::auth::require_auth;
///
/// let app = Router::new()
///     .layer(middleware::from_fn_with_state(state.clone(), require_auth));
/// ```
pub async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let ip = client_ip(&request);
    if let Err(secs) = state.limiter.check(ip) {
        warn!(ip = %ip, retry_after_seconds = secs, "Authentication rate limited");
        return Err(ApiError::TooManyRequests {
            retry_after_seconds: secs,
        });
    }

    // With no token and no other way in configured there is nothing to guess, so the
    // refusal is not counted
    let (expected_token, sign_in) = {
        let config = state.config.read().await;
        (config.api.auth_token.clone(), config.sign_in.clone())
    };
    if expected_token.is_none() && !sign_in.any() {
        return Err(not_configured());
    }

    // The Authorization header: a token, or Basic with the [sign_in] password (feature
    // 019). `Some(None)` is a header that isn't valid text, which counts as wrong. `None`
    // is no credential at all
    let mut wrong_token = false;
    // Already counted by the reservation above
    let mut counted = false;
    match request
        .headers()
        .get(AUTHORIZATION)
        .map(|h| h.to_str().ok().map(str::to_string))
    {
        None => {}
        Some(None) => wrong_token = true,
        Some(Some(value))
            if sign_in.password.is_set() && signin::basic_credentials(&value).is_some() =>
        {
            // The attempt is counted before the slow hash, so a burst can't all start
            // before the first failure is recorded; a match takes it back
            if let Err(secs) = state.limiter.reserve(ip) {
                warn!(ip = %ip, retry_after_seconds = secs, "Authentication rate limited");
                return Err(ApiError::TooManyRequests {
                    retry_after_seconds: secs,
                });
            }
            match signin::basic_matches(&state.sign_in, &value, &sign_in.password).await {
                signin::BasicCheck::Match => {
                    state.limiter.forgive(ip);
                    if is_write(request.method())
                        && (cross_site(request.headers())
                            || origin_mismatch(request.headers(), sign_in.base_url()))
                    {
                        return Err(ApiError::Forbidden(
                            "Cross-site writes with Basic credentials are refused".to_string(),
                        ));
                    }
                    return Ok(next.run(request).await);
                }
                signin::BasicCheck::Busy => {
                    state.limiter.forgive(ip);
                    warn!(ip = %ip, "Too many password checks at once");
                    return Err(ApiError::TooManyRequests {
                        retry_after_seconds: 1,
                    });
                }
                signin::BasicCheck::NoMatch => {
                    counted = true;
                    wrong_token = true;
                }
            }
        }
        Some(Some(value)) => match (extract_token(&value), &expected_token) {
            ("", _) => {}
            (token, Some(expected)) if token_matches(token, expected) => {
                return Ok(next.run(request).await)
            }
            _ => wrong_token = true,
        },
    }

    // A session cookie from the sign-in page. Writes also need its CSRF token
    if let Some((_, cookie, key)) =
        signin::current_session(&state.sign_in, &sign_in, request.headers())
    {
        if is_write(request.method()) {
            let provided = request
                .headers()
                .get(session::CSRF_HEADER)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if !session::csrf_matches(&key, &cookie, provided) {
                return Err(ApiError::Forbidden(
                    "CSRF token missing or wrong".to_string(),
                ));
            }
        }
        return Ok(next.run(request).await);
    }

    // Tailscale sign-in: after the token, so scripts never wait on tailscaled
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| *addr);
    let listened_on = request
        .extensions()
        .get::<crate::listen::ListenedOn>()
        .map(|l| l.0);
    if let Some(identity) = tailscale_identity(&state, request.headers(), peer, listened_on).await {
        // Like Basic, the identity is ambient: a browser on the tailnet sends it for
        // any site that asks, so a write must not come from another origin
        if is_write(request.method())
            && (cross_site(request.headers())
                || origin_mismatch(request.headers(), sign_in.base_url()))
        {
            return Err(ApiError::Forbidden(
                "Cross-site writes with a Tailscale identity are refused".to_string(),
            ));
        }
        let mut request = request;
        request.extensions_mut().insert(identity);
        return Ok(next.run(request).await);
    }

    // No token at all is not a failed authentication: browsers ask for paths nobody
    // links to (robots.txt, touch icons) and must not lock their operator out
    if wrong_token {
        if counted {
            warn!(ip = %ip, "Authentication failed");
        } else {
            let failures = state.limiter.record_failure(ip);
            warn!(ip = %ip, failures, "Authentication failed");
        }
    }
    Err(ApiError::Unauthorized("Unauthorized".to_string()))
}

/// The Tailscale identity that lets this request in, when identity sign-in is on and
/// the user is on the allow-list. Counted on the metrics; refusals are logged.
///
/// `peer` is the TCP peer from `ConnectInfo`. Without it there is no identity; it is
/// never assumed to be loopback. `listened_on` is the local address the request came in
/// on, when known.
///
/// `Tailscale-User-Login` from a loopback peer is believed only while tailscaled's serve
/// config proxies to this listener's port: a request on a port `tailscale serve` doesn't
/// front can't have come through it.
pub async fn tailscale_identity(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    listened_on: Option<SocketAddr>,
) -> Option<Identity> {
    let ts = state.config.read().await.tailscale.clone();
    if !ts.identity_sign_in {
        return None;
    }
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    // The headers only mean something when `tailscale serve` set them. A reverse proxy
    // on this machine connects from loopback too, and passes the client's own
    // `Tailscale-User-Login` through, so a proxied request has no identity.
    if peer.is_some_and(|p| p.ip().to_canonical().is_loopback()) && proxied(headers) {
        if header(TAILSCALE_USER_LOGIN).is_some() {
            warn_proxied_once();
        }
        return None;
    }
    if peer.is_some_and(|p| p.ip().to_canonical().is_loopback())
        && header(TAILSCALE_USER_LOGIN).is_some()
        && !serve_fronts_this_listener(state, listened_on).await
    {
        warn_not_served_once();
        return None;
    }
    let lookup = state
        .tailscale
        .identity_for(
            peer,
            header(TAILSCALE_USER_LOGIN),
            header(TAILSCALE_USER_NAME),
        )
        .await;
    match lookup {
        Lookup::None => None,
        Lookup::Found(identity) if ts.allows(&identity.login) => {
            putio_sync_telemetry::tailscale::record_identity(identity.via.as_str(), true);
            Some(identity)
        }
        Lookup::Found(identity) => {
            putio_sync_telemetry::tailscale::record_identity(identity.via.as_str(), false);
            info!(
                login = %identity.login,
                via = identity.via.as_str(),
                "Tailscale sign-in refused: not on the allowed users"
            );
            None
        }
        Lookup::Refused { via, reason } => {
            putio_sync_telemetry::tailscale::record_identity(via.as_str(), false);
            info!(via = via.as_str(), %reason, "Tailscale sign-in refused");
            None
        }
    }
}

/// Whether `tailscale serve` is set up to proxy to the port this request arrived on.
/// Without the listener's address, the bound ports (then the configured one) stand in.
async fn serve_fronts_this_listener(state: &AppState, listened_on: Option<SocketAddr>) -> bool {
    let mut ports: Vec<u16> = match listened_on {
        Some(addr) => vec![addr.port()],
        None => state.listening.get().iter().map(SocketAddr::port).collect(),
    };
    if ports.is_empty() {
        let config = state.config.read().await;
        if let Ok(resolved) = config.resolve_api() {
            ports.push(resolved.value.port());
        }
    }
    let status = state.tailscale.status().await;
    ports
        .iter()
        .any(|port| !status.serve_for_port(*port).is_empty())
}

fn warn_not_served_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        warn!(
            "Ignoring Tailscale-User-Login on a loopback connection: tailscaled's serve \
             config does not proxy to this port, so the header could come from anything on \
             this machine. Run `tailscale serve` for this port to use identity sign-in \
             through serve."
        );
    });
}

/// Whether a request carries reverse-proxy forwarding headers that `tailscale serve`
/// does not send. `tailscale serve` may add an `X-Forwarded-For` (and `-Host`/`-Proto`)
/// naming the tailnet client, so those pass; `Forwarded`, `X-Real-IP`, an
/// `X-Forwarded-For` with an address outside the tailnet, or an `X-Forwarded-Host` with
/// no tailnet `X-Forwarded-For` mean another proxy handled the request.
pub fn proxied(headers: &HeaderMap) -> bool {
    if headers.contains_key("forwarded") || headers.contains_key("x-real-ip") {
        return true;
    }
    let mut forwarded_for = headers.get_all("x-forwarded-for").iter().peekable();
    if forwarded_for.peek().is_none() {
        return headers.contains_key("x-forwarded-host");
    }
    !forwarded_for.all(|v| {
        v.to_str().is_ok_and(|v| {
            v.split(',').all(|part| {
                part.trim()
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(putio_sync_tailscale::is_tailnet_ip)
            })
        })
    })
}

fn warn_proxied_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        warn!(
            "Ignoring Tailscale-User-Login on a request that came through a reverse proxy \
             (X-Forwarded-For, X-Forwarded-Host, Forwarded or X-Real-IP is set): the header \
             could be forged by the client. Tailscale identity sign-in works with \
             `tailscale serve` or a direct tailnet connection, not behind another proxy."
        );
    });
}

/// Whether the request has an `Origin` that isn't the host it was sent to: the `Host`
/// header, the proxy's `X-Forwarded-Host`, or the host of `public_url`. Requests without
/// an `Origin` (scripts, curl, same-origin GETs) are never refused for it.
fn origin_mismatch(headers: &HeaderMap, public_url: Option<&str>) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let authority = |url: &str| {
        url.split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(url)
            .split('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let origin = authority(origin);
    let matches = |value: &str| value.trim().to_ascii_lowercase() == origin;
    let hosts = ["host", "x-forwarded-host"].iter().any(|name| {
        headers
            .get_all(*name)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|v| v.split(',').any(matches))
    });
    !(hosts || public_url.is_some_and(|u| authority(u) == origin))
}

/// Whether `method` changes something (anything but GET, HEAD and OPTIONS)
pub fn is_write(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Whether the browser says the request came from another site (`Sec-Fetch-Site`).
/// Scripts don't send the header, so they are never refused for it.
fn cross_site(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "cross-site")
}

/// The 401 for a server with no token configured
pub fn not_configured() -> ApiError {
    ApiError::Unauthorized("API token not configured".to_string())
        .with_details("Set [api] auth_token in the config file or PUTIO_SYNC_API_TOKEN")
}

/// Whether `provided` equals `expected`, compared in constant time (for `/auth/login`)
pub fn token_matches(provided: &str, expected: &str) -> bool {
    constant_time_compare(provided.as_bytes(), expected.as_bytes())
}

/// The token in an Authorization header value: "Bearer <token>", or the raw token for
/// backward compatibility. Empty when the header carries none.
fn extract_token(auth_header: &str) -> &str {
    let auth_header = auth_header.trim();
    if auth_header == "Bearer" {
        return "";
    }
    auth_header
        .strip_prefix("Bearer ")
        .unwrap_or(auth_header)
        .trim()
}

/// Validate an authentication token
///
/// Supports Bearer token format: "Bearer <token>"
/// Performs constant-time comparison to prevent timing attacks
#[cfg(test)]
fn validate_token(auth_header: &str, expected_token: &str) -> bool {
    token_matches(extract_token(auth_header), expected_token)
}

/// Constant-time string comparison to prevent timing attacks
fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (byte_a, byte_b) in a.iter().zip(b.iter()) {
        result |= byte_a ^ byte_b;
    }
    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    #[test]
    fn proxied_detects_forwarding_headers() {
        assert!(!proxied(&headers(&[])));
        assert!(!proxied(&headers(&[("x-forwarded-for", "100.64.1.2")])));
        assert!(!proxied(&headers(&[
            ("x-forwarded-for", "fd7a:115c:a1e0::1"),
            ("x-forwarded-host", "nas.tail1234.ts.net"),
        ])));
        assert!(proxied(&headers(&[("x-forwarded-for", "203.0.113.9")])));
        assert!(proxied(&headers(&[(
            "x-forwarded-for",
            "100.64.1.2, 203.0.113.9"
        )])));
        assert!(proxied(&headers(&[(
            "x-forwarded-host",
            "nas.example.net"
        )])));
        assert!(proxied(&headers(&[("forwarded", "for=203.0.113.9")])));
        assert!(proxied(&headers(&[("x-real-ip", "100.64.1.2")])));
    }

    #[test]
    fn origin_must_match_host() {
        assert!(!origin_mismatch(&headers(&[("host", "nas:8080")]), None));
        let same = headers(&[("host", "nas:8080"), ("origin", "http://nas:8080")]);
        assert!(!origin_mismatch(&same, None));
        let other = headers(&[("host", "nas:8080"), ("origin", "https://evil.example")]);
        assert!(origin_mismatch(&other, None));
        assert!(origin_mismatch(
            &headers(&[("host", "nas"), ("origin", "null")]),
            None
        ));
        // A proxy that rewrites Host: the forwarded host or public_url still match
        let proxied_host = headers(&[
            ("host", "127.0.0.1:8080"),
            ("x-forwarded-host", "sync.example.net"),
            ("origin", "https://sync.example.net"),
        ]);
        assert!(!origin_mismatch(&proxied_host, None));
        let rewritten = headers(&[
            ("host", "127.0.0.1:8080"),
            ("origin", "https://sync.example.net"),
        ]);
        assert!(origin_mismatch(&rewritten, None));
        assert!(!origin_mismatch(
            &rewritten,
            Some("https://sync.example.net/")
        ));
    }

    #[test]
    fn test_validate_token_with_bearer() {
        let expected = "my-secret-token";
        assert!(validate_token("Bearer my-secret-token", expected));
        assert!(validate_token("Bearer  my-secret-token  ", expected)); // with spaces
        assert!(!validate_token("Bearer wrong-token", expected));
    }

    #[test]
    fn test_validate_token_without_bearer() {
        let expected = "my-secret-token";
        assert!(validate_token("my-secret-token", expected));
        assert!(!validate_token("wrong-token", expected));
    }

    #[test]
    fn test_extract_token() {
        assert_eq!(extract_token("Bearer abc"), "abc");
        assert_eq!(extract_token(" abc "), "abc");
        assert_eq!(extract_token("Bearer"), "");
        assert_eq!(extract_token("Bearer   "), "");
        assert_eq!(extract_token(""), "");
    }

    #[test]
    fn test_token_matches() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("abc", "abd"));
        assert!(!token_matches("", "abc"));
    }

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare(b"hello", b"hello"));
        assert!(!constant_time_compare(b"hello", b"world"));
        assert!(!constant_time_compare(b"hello", b"hello!"));
        assert!(!constant_time_compare(b"hello!", b"hello"));
    }
}

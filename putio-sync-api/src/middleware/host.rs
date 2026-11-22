//! Host header allow-list (DNS rebinding guard)
//!
//! A web page can point its own name at this server's address (DNS rebinding) and then
//! talk to the API as if it were same-origin. Such a request still carries the attacker's
//! name in `Host`, and the attacker can only own names with a dot in them. So the API
//! answers only to the names a person really uses:
//!
//! - an IP address, with or without a port
//! - `localhost`, a name with no dot (`nas`), `*.local`, `*.localhost` and `*.ts.net`
//! - LAN-only names: `*.lan`, `*.home`, `*.home.arpa`, `*.internal`, `*.localdomain`
//! - the host of `[sign_in] public_url`
//! - the names in `[api] allowed_hosts`
//!
//! Anything else gets 421. A request with no `Host` at all passes: hyper already refuses
//! an HTTP/1.1 request without one, and a browser always sends it.

use crate::AppState;
use axum::extract::{Request, State};
use axum::http::{header::HOST, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use std::net::IpAddr;

/// Suffixes for LAN-only names that nobody can register in public DNS
const LAN_SUFFIXES: &[&str] = &[".lan", ".home", ".home.arpa", ".internal", ".localdomain"];

/// The names to accept besides the built-in ones
#[derive(Debug, Clone, Default)]
pub struct HostPolicy {
    allowed: Vec<String>,
    public_host: Option<String>,
}

impl HostPolicy {
    /// A policy from `[api] allowed_hosts` and `[sign_in] public_url`
    pub fn new(allowed_hosts: &[String], public_url: Option<&str>) -> Self {
        HostPolicy {
            allowed: allowed_hosts
                .iter()
                .map(|h| host_only(h).to_string())
                .filter(|h| !h.is_empty())
                .collect(),
            public_host: public_url
                .map(|u| u.split_once("://").map_or(u, |(_, rest)| rest))
                .and_then(|u| u.split(['/', '?', '#']).next())
                .map(|authority| host_only(authority.rsplit('@').next().unwrap_or("")).to_string())
                .filter(|h| !h.is_empty()),
        }
    }

    /// Whether a request for `host` (a `Host` header value, port allowed) may be served
    pub fn allows(&self, host: &str) -> bool {
        let host = host_only(host);
        if host.is_empty() {
            return false;
        }
        if host.parse::<IpAddr>().is_ok()
            || host == "localhost"
            || !host.contains('.')
            || host.ends_with(".local")
            || host.ends_with(".localhost")
            || host.ends_with(".ts.net")
            || LAN_SUFFIXES.iter().any(|suffix| host.ends_with(suffix))
        {
            return true;
        }
        if self.public_host.as_deref() == Some(host.as_str()) {
            return true;
        }
        self.allowed
            .iter()
            .any(|entry| match entry.strip_prefix("*.") {
                Some(suffix) => {
                    host.len() > suffix.len() + 1 && host.ends_with(&format!(".{suffix}"))
                }
                None => *entry == host,
            })
    }
}

/// The host part of `value` in lower case: no port, no IPv6 brackets, no trailing dot
fn host_only(value: &str) -> String {
    let value = value.trim();
    let host = if let Some(rest) = value.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else if value.matches(':').count() > 1 {
        // A bare IPv6 address
        value
    } else {
        value.split(':').next().unwrap_or("")
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// The 421 for a name that isn't allowed
pub fn misdirected(host: &str) -> Response {
    let host: String = host.chars().filter(|c| !c.is_control()).take(100).collect();
    (
        StatusCode::MISDIRECTED_REQUEST,
        Json(json!({
            "error": format!(
                "The host name '{host}' isn't allowed. Add it to [api] allowed_hosts in the config file."
            ),
        })),
    )
        .into_response()
}

/// The host a request was sent to: its `Host` header, else the URI's authority (HTTP/2)
fn requested_host(request: &Request) -> Option<String> {
    request
        .headers()
        .get(HOST)
        .map(|v| v.to_str().unwrap_or("").to_string())
        .or_else(|| request.uri().authority().map(|a| a.to_string()))
}

/// Refuse requests for a host name that isn't allowed (main router)
pub async fn require_allowed_host(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(host) = requested_host(&request) {
        let policy = {
            let config = state.config.read().await;
            HostPolicy::new(&config.api.allowed_hosts, config.sign_in.base_url())
        };
        if !policy.allows(&host) {
            tracing::warn!(host = %host, "Refused a request for a host name that is not allowed");
            return misdirected(&host);
        }
    }
    next.run(request).await
}

/// Refuse requests for a host name that isn't allowed, with a fixed policy (setup router)
pub async fn require_allowed_host_fixed(
    State(policy): State<HostPolicy>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(host) = requested_host(&request) {
        if !policy.allows(&host) {
            tracing::warn!(host = %host, "Refused a request for a host name that is not allowed");
            return misdirected(&host);
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> HostPolicy {
        HostPolicy::new(
            &[
                "Sync.Example.Org".to_string(),
                "*.lan.example.org".to_string(),
            ],
            Some("https://public.example.net/ui"),
        )
    }

    #[test]
    fn built_in_names_pass() {
        let p = HostPolicy::default();
        for host in [
            "192.168.1.5",
            "192.168.1.5:15432",
            "[::1]:15432",
            "[fd7a:115c:a1e0::1]",
            "::1",
            "localhost",
            "localhost:15432",
            "nas",
            "nas:15432",
            "NAS.local:15432",
            "nas.tail1234.ts.net",
            "nas.tail1234.ts.net.",
            "app.localhost",
            "nas.lan:15432",
            "nas.home.arpa",
            "nas.internal",
        ] {
            assert!(p.allows(host), "{host}");
        }
    }

    #[test]
    fn rebinding_names_are_refused() {
        let p = HostPolicy::default();
        for host in [
            "evil.example.com",
            "evil.example.com:15432",
            "1.2.3.4.evil.com",
            "nas.local.evil.com",
            "nas.ts.net.evil.com",
            "",
            ":15432",
        ] {
            assert!(!p.allows(host), "{host}");
        }
    }

    #[test]
    fn configured_names_pass() {
        let p = policy();
        assert!(p.allows("sync.example.org"));
        assert!(p.allows("sync.example.org:8443"));
        assert!(p.allows("public.example.net"));
        assert!(p.allows("a.lan.example.org"));
        assert!(!p.allows("lan.example.org"));
        assert!(!p.allows("evil.lan.example.org.evil.com"));
        assert!(!p.allows("other.example.org"));
    }
}

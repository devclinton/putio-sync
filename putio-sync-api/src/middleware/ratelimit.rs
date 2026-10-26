//! Per-IP limiter for failed authentication
//!
//! Counts wrong tokens on protected routes and failed `/auth/login` calls. A request
//! that carries no token at all is refused but not counted.
//! After `limit` failures inside one window, the IP is refused with 429 until the
//! window ends. Successful requests do not reset the window.

use axum::extract::{ConnectInfo, Request};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failures allowed per window
pub const AUTH_FAILURE_LIMIT: u32 = 5;

/// Length of the failure window
pub const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// Failed-authentication counter keyed by client IP
pub struct AuthLimiter {
    limit: u32,
    window: Duration,
    inner: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl AuthLimiter {
    /// Allow `limit` failures per `window` per IP
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// `Err(seconds left in the window)` when `ip` has used up its failures
    pub fn check(&self, ip: IpAddr) -> Result<(), u64> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&ip) {
            Some((start, failures)) if *failures >= self.limit => {
                let elapsed = start.elapsed();
                if elapsed < self.window {
                    // Round up so the client never retries a moment too early
                    let left = self.window - elapsed;
                    let secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
                    Err(secs.max(1))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// Count a failure for `ip` and return its failures in the current window
    pub fn record_failure(&self, ip: IpAddr) -> u32 {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = self.window;
        map.retain(|_, (start, _)| now.duration_since(*start) < window);
        let entry = map.entry(ip).or_insert((now, 0));
        entry.1 += 1;
        entry.1
    }
}

/// The client IP from `ConnectInfo`, or `127.0.0.1` when the router runs without it
/// (as in `oneshot` tests)
pub fn client_ip(request: &Request) -> IpAddr {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, last))
    }

    #[test]
    fn five_failures_allowed_sixth_refused() {
        let limiter = AuthLimiter::new(AUTH_FAILURE_LIMIT, AUTH_FAILURE_WINDOW);
        for n in 1..=5 {
            assert!(limiter.check(ip(1)).is_ok(), "attempt {n} allowed");
            assert_eq!(limiter.record_failure(ip(1)), n);
        }
        let retry_after = limiter.check(ip(1)).unwrap_err();
        assert!((1..=60).contains(&retry_after), "{retry_after}");
    }

    #[test]
    fn new_window_allows_again() {
        let limiter = AuthLimiter::new(2, Duration::from_millis(50));
        limiter.record_failure(ip(1));
        limiter.record_failure(ip(1));
        assert!(limiter.check(ip(1)).is_err());

        std::thread::sleep(Duration::from_millis(60));
        assert!(limiter.check(ip(1)).is_ok());
        assert_eq!(limiter.record_failure(ip(1)), 1, "expired window resets");
        assert!(limiter.check(ip(1)).is_ok());
    }

    #[test]
    fn ips_are_independent() {
        let limiter = AuthLimiter::new(AUTH_FAILURE_LIMIT, AUTH_FAILURE_WINDOW);
        for _ in 0..5 {
            limiter.record_failure(ip(1));
        }
        assert!(limiter.check(ip(1)).is_err());
        assert!(limiter.check(ip(2)).is_ok());
        assert_eq!(limiter.record_failure(ip(2)), 1);
    }

    #[test]
    fn client_ip_falls_back_to_localhost() {
        let request = Request::new(axum::body::Body::empty());
        assert_eq!(client_ip(&request), IpAddr::V4(Ipv4Addr::LOCALHOST));

        let mut request = Request::new(axum::body::Body::empty());
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 7], 5555))));
        assert_eq!(client_ip(&request), IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)));
    }
}

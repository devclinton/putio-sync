//! Outage tracking for the health events (feature 057)
//!
//! An [`Outage`] watches one thing that is called again and again (Put.io, one Radarr or
//! Sonarr server). It says "down" once, after calls have failed for the threshold, and
//! "up" once, when a call works again after that. A short blip inside the threshold says
//! nothing, so a flapping connection sends at most one pair per outage.

use chrono::{DateTime, Utc};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long calls must keep failing before the outage is announced
pub const OUTAGE_THRESHOLD: Duration = Duration::from_secs(60);

/// What a call's result changed
#[derive(Debug, Clone, PartialEq)]
pub enum Edge {
    /// The outage passed the threshold: announce it
    Down {
        /// When the first failed call was made
        since: DateTime<Utc>,
    },
    /// An announced outage ended
    Up {
        /// When the first failed call was made
        since: DateTime<Utc>,
        /// From the first failed call to the first working one
        outage_seconds: f64,
    },
}

#[derive(Debug, Default)]
struct State {
    failing: Option<(Instant, DateTime<Utc>)>,
    announced: bool,
}

/// One watched thing; see the module docs
#[derive(Debug)]
pub struct Outage {
    threshold: Mutex<Duration>,
    state: Mutex<State>,
}

impl Default for Outage {
    fn default() -> Self {
        Self::new(OUTAGE_THRESHOLD)
    }
}

impl Outage {
    /// A tracker that announces after `threshold` of failures
    pub fn new(threshold: Duration) -> Self {
        Self {
            threshold: Mutex::new(threshold),
            state: Mutex::new(State::default()),
        }
    }

    /// Change the threshold; zero announces on the first failed call
    pub fn set_threshold(&self, threshold: Duration) {
        *self.threshold.lock().unwrap_or_else(|e| e.into_inner()) = threshold;
    }

    /// A call failed now
    pub fn failed(&self) -> Option<Edge> {
        self.failed_at(Instant::now())
    }

    /// A call worked now
    pub fn ok(&self) -> Option<Edge> {
        self.ok_at(Instant::now())
    }

    /// [`Self::failed`] with the clock given
    pub fn failed_at(&self, now: Instant) -> Option<Edge> {
        let threshold = *self.threshold.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (first, since) = *state.failing.get_or_insert((now, Utc::now()));
        if state.announced || now.saturating_duration_since(first) < threshold {
            return None;
        }
        state.announced = true;
        Some(Edge::Down { since })
    }

    /// [`Self::ok`] with the clock given
    pub fn ok_at(&self, now: Instant) -> Option<Edge> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (first, since) = state.failing.take()?;
        if !std::mem::take(&mut state.announced) {
            return None;
        }
        Some(Edge::Up {
            since,
            outage_seconds: now.saturating_duration_since(first).as_secs_f64(),
        })
    }
}

/// Says something once until it is reset: the refused token, a low disk
#[derive(Debug, Default)]
pub struct Once(std::sync::atomic::AtomicBool);

impl Once {
    /// True the first time since the last [`Self::reset`]
    pub fn raise(&self) -> bool {
        !self.0.swap(true, std::sync::atomic::Ordering::SeqCst)
    }

    /// True when it was raised: the trouble is over
    pub fn reset(&self) -> bool {
        self.0.swap(false, std::sync::atomic::Ordering::SeqCst)
    }
}

/// Watches a Put.io client's calls and sends `putio.unreachable`, `putio.recovered` and
/// `auth.expired`, each once per change
#[derive(Debug, Default)]
pub struct PutioWatch {
    outage: Outage,
    auth: Once,
}

impl PutioWatch {
    /// A watch that announces an outage after `threshold`
    pub fn new(threshold: Duration) -> Self {
        Self {
            outage: Outage::new(threshold),
            auth: Once::default(),
        }
    }

    /// Follow every attempt `client` makes from now on
    pub fn attach(self, client: &putio_sync_client::PutioApiClient) {
        client.observe_attempts(Box::new(move |outcome, status| {
            self.record(outcome, status);
        }));
    }

    /// One attempt ended: `status` is the HTTP status when Put.io answered
    pub fn record(&self, outcome: putio_sync_telemetry::ApiOutcome, status: Option<u16>) {
        use crate::events::{self, Payload};
        use putio_sync_telemetry::ApiOutcome;
        let edge = match outcome {
            ApiOutcome::NetworkError | ApiOutcome::ServerError => self.outage.failed(),
            _ => self.outage.ok(),
        };
        match edge {
            Some(Edge::Down { since }) => {
                events::emit(Payload::PutioUnreachable(events::PutioUnreachable {
                    since,
                    error: match outcome {
                        ApiOutcome::ServerError => "server_error",
                        _ => "network_error",
                    }
                    .to_string(),
                }));
            }
            Some(Edge::Up {
                since,
                outage_seconds,
            }) => {
                events::emit(Payload::PutioRecovered(events::PutioRecovered {
                    since,
                    outage_seconds,
                }));
            }
            None => {}
        }
        match outcome {
            ApiOutcome::Success => {
                self.auth.reset();
            }
            // 401 only: a 403 can be about one file, not the token
            _ if status == Some(401) && self.auth.raise() => {
                events::emit(Payload::AuthExpired(events::AuthExpired {
                    message: "Put.io refused the token. Sign in again with `putio-sync login` \
                              or on the Settings page."
                        .to_string(),
                }));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outage_is_announced_once_after_the_threshold() {
        let outage = Outage::new(Duration::from_secs(60));
        let t0 = Instant::now();
        assert_eq!(outage.failed_at(t0), None);
        assert_eq!(outage.failed_at(t0 + Duration::from_secs(59)), None);
        let down = outage.failed_at(t0 + Duration::from_secs(60));
        assert!(matches!(down, Some(Edge::Down { .. })));
        // Not again while it lasts
        assert_eq!(outage.failed_at(t0 + Duration::from_secs(120)), None);
        match outage.ok_at(t0 + Duration::from_secs(150)) {
            Some(Edge::Up { outage_seconds, .. }) => assert_eq!(outage_seconds, 150.0),
            other => panic!("expected an Up, got {other:?}"),
        }
        assert_eq!(outage.ok_at(t0 + Duration::from_secs(151)), None);
    }

    #[test]
    fn a_blip_inside_the_threshold_says_nothing() {
        let outage = Outage::new(Duration::from_secs(60));
        let t0 = Instant::now();
        assert_eq!(outage.failed_at(t0), None);
        assert_eq!(outage.ok_at(t0 + Duration::from_secs(10)), None);
        // The next failure starts a new count
        assert_eq!(outage.failed_at(t0 + Duration::from_secs(65)), None);
        assert_eq!(outage.failed_at(t0 + Duration::from_secs(100)), None);
        assert!(outage.failed_at(t0 + Duration::from_secs(125)).is_some());
    }

    #[test]
    fn once_raises_once_until_reset() {
        let once = Once::default();
        assert!(once.raise());
        assert!(!once.raise());
        assert!(once.reset());
        assert!(!once.reset());
        assert!(once.raise());
    }
}

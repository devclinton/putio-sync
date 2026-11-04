//! Bandwidth rate limiting for downloads
//!
//! One limiter is shared by every download (feature 032), so the limit is the total. It
//! follows the bandwidth schedule and any timed pause or slow-down, re-reading the limit
//! at most once a second, so a change reaches running downloads within about a second.

use chrono::{DateTime, FixedOffset, Local, Utc};
use putio_sync_core::bandwidth::{Current, Limit, Override, Policy, Status};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::time::Instant;

/// The daemon's wall clock; tests pass a fixed one
pub type Clock = Arc<dyn Fn() -> DateTime<FixedOffset> + Send + Sync>;

/// How long the limit in force is reused before the schedule is read again
const RECHECK: Duration = Duration::from_secs(1);

/// Shared token-bucket rate limiter; clones share the same bucket
#[derive(Clone)]
pub struct RateLimiter {
    state: Arc<Mutex<State>>,
    clock: Clock,
}

struct State {
    policy: Policy,
    override_: Option<Override>,
    /// The limit in force and when it was read; `None` before the first read
    current: Option<(Current, Instant)>,
    /// Bytes that may be sent now; negative is debt. At most one second's worth.
    tokens: f64,
    last_refill: Instant,
}

impl std::fmt::Debug for RateLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RateLimiter")
            .field("current", &self.current())
            .finish()
    }
}

fn local_now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

impl RateLimiter {
    /// A fixed limit of `bytes_per_sec` for all users of this limiter together
    pub fn new(bytes_per_sec: u64) -> Self {
        Self::from_policy(Policy::fixed(Some(bytes_per_sec)))
    }

    /// No limit until a policy or override says otherwise
    pub fn unlimited() -> Self {
        Self::from_policy(Policy::fixed(None))
    }

    /// Follow `policy` on the daemon's local time
    pub fn from_policy(policy: Policy) -> Self {
        Self::with_clock(policy, Arc::new(local_now))
    }

    /// Follow `policy` on `clock`
    pub fn with_clock(policy: Policy, clock: Clock) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                policy,
                override_: None,
                current: None,
                tokens: 0.0,
                last_refill: Instant::now(),
            })),
            clock,
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Replace the schedule and limits; running downloads follow at once
    pub fn set_policy(&self, policy: Policy) {
        let mut state = self.lock();
        if state.policy != policy {
            state.policy = policy;
            state.refresh(&self.clock, Instant::now(), true);
        }
    }

    /// Start (`Some`) or end (`None`) a timed pause or slow-down
    pub fn set_override(&self, override_: Option<Override>) {
        let mut state = self.lock();
        state.override_ = override_;
        state.refresh(&self.clock, Instant::now(), true);
    }

    /// The pause or slow-down in force, if any
    pub fn override_(&self) -> Option<Override> {
        let now = (self.clock)().with_timezone(&Utc);
        self.lock().override_.filter(|o| o.active(now))
    }

    /// The limit in force now and where it comes from
    pub fn current(&self) -> Current {
        let now = Instant::now();
        self.lock().refresh(&self.clock, now, true)
    }

    /// Whether downloads are paused now (by the schedule or a pause)
    pub fn is_paused(&self) -> bool {
        self.current().limit == Limit::Paused
    }

    /// Everything the Status page and `GET /bandwidth` show
    pub fn status(&self) -> Status {
        let mut state = self.lock();
        state.refresh(&self.clock, Instant::now(), false);
        state
            .policy
            .status((self.clock)(), state.override_.as_ref())
    }

    /// Take `bytes` from the shared bucket, waiting while it is in debt or paused.
    /// Wrap the call in a stop check: a pause can last hours.
    pub async fn consume(&self, bytes: usize) {
        let mut owed = Some(bytes as f64);
        loop {
            let wait = {
                let now = Instant::now();
                let mut state = self.lock();
                match state.refresh(&self.clock, now, false).limit {
                    Limit::Unlimited => return,
                    Limit::Paused => RECHECK,
                    Limit::Rate(rate) => {
                        let rate = rate as f64;
                        state.refill(now, rate);
                        if let Some(n) = owed.take() {
                            state.tokens -= n;
                        }
                        if state.tokens >= 0.0 {
                            return;
                        }
                        Duration::from_secs_f64(-state.tokens / rate).min(RECHECK)
                    }
                }
            };
            tokio::time::sleep(wait).await;
        }
    }
}

impl State {
    /// The limit in force, read again when stale (or `force`); logs and sets the gauges
    /// when it changes
    fn refresh(&mut self, clock: &Clock, now: Instant, force: bool) -> Current {
        if let Some((current, at)) = self.current {
            if !force && now.duration_since(at) < RECHECK {
                return current;
            }
        }
        let local = clock();
        let utc = local.with_timezone(&Utc);
        if self.override_.is_some_and(|o| !o.active(utc)) {
            self.override_ = None;
        }
        let next = self.policy.current(local, self.override_.as_ref());
        let previous = self.current.map(|(c, _)| c);
        if previous != Some(next) {
            if previous.map(|c| c.limit) != Some(next.limit) {
                // Start the bucket over: no burst saved up while paused or unlimited
                self.tokens = 0.0;
                self.last_refill = now;
            }
            tracing::info!(
                limit = %next.limit.describe(),
                source = %next.source.describe(next.entry),
                "Download limit"
            );
            putio_sync_telemetry::download::bandwidth_limit(next.limit.bytes_per_sec());
            putio_sync_telemetry::download::bandwidth_override(self.override_.is_some());
        }
        self.current = Some((next, now));
        next
    }

    fn refill(&mut self, now: Instant, rate: f64) {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate).min(rate);
        self.last_refill = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_core::bandwidth::{OverrideMode, ScheduleEntry, Source};
    use std::sync::atomic::{AtomicI64, Ordering};

    /// A wall clock that tests move by hand, starting Wednesday 2026-11-04 12:00 UTC
    fn clock() -> (Clock, Arc<AtomicI64>) {
        let offset = Arc::new(AtomicI64::new(0));
        let start = DateTime::parse_from_rfc3339("2026-11-04T12:00:00+00:00").unwrap();
        let seconds = offset.clone();
        let clock: Clock =
            Arc::new(move || start + chrono::Duration::seconds(seconds.load(Ordering::SeqCst)));
        (clock, offset)
    }

    #[tokio::test(start_paused = true)]
    async fn unlimited_returns_at_once() {
        let limiter = RateLimiter::unlimited();
        let start = Instant::now();
        limiter.consume(100 * 1024 * 1024).await;
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn two_consumers_share_the_limit() {
        let limiter = RateLimiter::new(1000);
        let start = Instant::now();
        let run = |l: RateLimiter| async move {
            for _ in 0..10 {
                l.consume(500).await;
            }
        };
        tokio::join!(run(limiter.clone()), run(limiter.clone()));
        // 10 000 bytes at 1000 B/s in total, not 1000 B/s each
        let secs = start.elapsed().as_secs_f64();
        assert!((9.5..=10.5).contains(&secs), "took {secs}s");
    }

    #[tokio::test(start_paused = true)]
    async fn a_lower_limit_reaches_a_waiting_consumer() {
        let limiter = RateLimiter::new(1_000_000);
        limiter.consume(1_000_000).await;
        limiter.set_policy(Policy::fixed(Some(100)));
        let start = Instant::now();
        limiter.consume(200).await;
        let secs = start.elapsed().as_secs_f64();
        assert!((1.5..=2.5).contains(&secs), "took {secs}s");
    }

    #[tokio::test(start_paused = true)]
    async fn paused_blocks_until_resumed() {
        let (clock, _) = clock();
        let limiter = RateLimiter::with_clock(Policy::fixed(None), clock);
        let now = Utc::now();
        limiter.set_override(Some(
            Override::for_minutes(OverrideMode::Pause, 60, now + chrono::Duration::days(3650))
                .unwrap(),
        ));
        assert!(limiter.is_paused());
        let waiting = tokio::spawn({
            let l = limiter.clone();
            async move { l.consume(10).await }
        });
        tokio::time::sleep(Duration::from_secs(30)).await;
        assert!(!waiting.is_finished());
        limiter.set_override(None);
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(waiting.is_finished());
        assert!(!limiter.is_paused());
    }

    #[tokio::test(start_paused = true)]
    async fn follows_the_schedule_and_drops_an_expired_override() {
        let (clock, seconds) = clock();
        let policy = Policy::new(
            Some(5000),
            &[ScheduleEntry {
                days: "daily".into(),
                start: "13:00".into(),
                end: "14:00".into(),
                limit: Limit::Paused,
            }],
            1000,
        );
        let limiter = RateLimiter::with_clock(policy, clock.clone());
        assert_eq!(limiter.current().limit, Limit::Rate(5000));
        let now = clock().with_timezone(&Utc);
        limiter.set_override(Some(
            Override::for_minutes(OverrideMode::Slow, 30, now).unwrap(),
        ));
        let slow = limiter.current();
        assert_eq!((slow.limit, slow.source), (Limit::Rate(1000), Source::Slow));
        seconds.store(3600, Ordering::SeqCst);
        let at_one = limiter.current();
        assert_eq!((at_one.limit, at_one.entry), (Limit::Paused, Some(1)));
        assert!(limiter.override_().is_none());
        assert_eq!(limiter.status().source, Source::Schedule);
    }
}

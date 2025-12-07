//! Bandwidth rate limiting for downloads

use std::time::{Duration, Instant};

/// Rate limiter for bandwidth throttling
pub struct RateLimiter {
    /// Maximum bytes per second
    bytes_per_sec: u64,
    /// Last reset time
    last_reset: Instant,
    /// Bytes consumed in current window
    bytes_consumed: u64,
}

impl RateLimiter {
    /// Create a new rate limiter with specified bytes per second
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            bytes_per_sec,
            last_reset: Instant::now(),
            bytes_consumed: 0,
        }
    }

    /// Wait if necessary to maintain the rate limit
    pub async fn consume(&mut self, bytes: usize) {
        let bytes = bytes as u64;
        self.bytes_consumed += bytes;

        let elapsed = self.last_reset.elapsed();
        let expected_duration = Duration::from_secs_f64(self.bytes_consumed as f64 / self.bytes_per_sec as f64);

        if expected_duration > elapsed {
            let sleep_duration = expected_duration - elapsed;
            tokio::time::sleep(sleep_duration).await;
        }

        // Reset counter every second
        if elapsed >= Duration::from_secs(1) {
            self.last_reset = Instant::now();
            self.bytes_consumed = 0;
        }
    }

    /// Update the bandwidth limit
    pub fn update_limit(&mut self, bytes_per_sec: u64) {
        self.bytes_per_sec = bytes_per_sec;
    }
}

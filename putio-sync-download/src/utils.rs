//! Utility functions for downloads

use crate::error::{DownloadError, DownloadResult};
use crate::stream::{unless_stopped, StopSignal};
use std::future::Future;
use std::time::Duration;
use tracing::{error, info, warn};

/// Result of retry operation with attempt count
pub struct RetryResult<T> {
    pub value: T,
    pub attempts: u32,
}

/// Longest wait between two attempts, before jitter
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Wait before a stalled download restarts, before jitter
const STALL_BACKOFF: Duration = Duration::from_secs(3);

/// `base` with up to 25% added or taken off, so workers that failed together (the
/// network dropped) do not all retry at the same moment
fn jittered(base: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // 0.75..=1.25
    base.mul_f64(0.75 + f64::from(nanos % 501) / 1000.0)
}

/// The wait before retry number `attempt` (1-based): `initial_delay` seconds, doubled for
/// each further retry, capped at [`MAX_BACKOFF`], with jitter. A stalled download restarts
/// sooner.
fn backoff(attempt: u32, initial_delay: u64, stalled: bool) -> Duration {
    let base = if stalled {
        STALL_BACKOFF
    } else {
        Duration::from_secs(initial_delay)
            .saturating_mul(1 << (attempt - 1).min(5))
            .min(MAX_BACKOFF)
    };
    jittered(base)
}

/// Retry a future with exponential backoff
///
/// Only a [transient](DownloadError::is_transient) error is retried: a 404, a full
/// disk or a denied path fails at once, since waiting cannot fix it. Waits grow with each
/// attempt up to a cap and are jittered; a stalled download restarts sooner. A stop
/// ends the wait.
///
/// # Arguments
///
/// * `operation` - The async operation to retry
/// * `max_retries` - Maximum number of retry attempts
/// * `initial_delay` - Initial delay in seconds
/// * `operation_name` - Name of the operation for logging
/// * `stop` - Optional stop signal that cuts a wait short with [`DownloadError::Stopped`]
///
/// # Returns
///
/// Returns the result along with the number of attempts made
pub async fn retry_with_stall_detection<F, Fut, T>(
    mut operation: F,
    max_retries: u32,
    initial_delay: u64,
    operation_name: &str,
    stop: Option<&StopSignal>,
) -> DownloadResult<RetryResult<T>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = DownloadResult<T>>,
{
    let mut attempt = 0;

    loop {
        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    info!(
                        "{} succeeded after {} retry attempt(s)",
                        operation_name, attempt
                    );
                }
                return Ok(RetryResult {
                    value: result,
                    attempts: attempt,
                });
            }
            Err(DownloadError::Stopped) => return Err(DownloadError::Stopped),
            Err(e) if !e.is_transient() => {
                error!("{} failed and will not be retried: {}", operation_name, e);
                return Err(e);
            }
            Err(e) => {
                attempt += 1;
                if attempt > max_retries {
                    error!(
                        "{} failed after {} attempts: {}",
                        operation_name, max_retries, e
                    );
                    return Err(e);
                }

                // Check if this is a stalled download error
                let is_stall_error = e.to_string().contains("Download stalled");
                let delay = backoff(attempt, initial_delay, is_stall_error);
                putio_sync_telemetry::download::retried(e.retry_reason());

                if is_stall_error {
                    warn!(
                        "{} stalled (attempt {}/{}): {}. Restarting download in {:.1}s...",
                        operation_name,
                        attempt,
                        max_retries,
                        e,
                        delay.as_secs_f64()
                    );
                } else {
                    warn!(
                        "{} failed (attempt {}/{}): {}. Retrying in {:.1}s...",
                        operation_name,
                        attempt,
                        max_retries,
                        e,
                        delay.as_secs_f64()
                    );
                }

                unless_stopped(stop, tokio::time::sleep(delay)).await?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn transient() -> DownloadError {
        DownloadError::ApiError("HTTP error 503 Service Unavailable: - Response: busy".into())
    }

    #[tokio::test(start_paused = true)]
    async fn transient_errors_are_retried_until_they_pass() {
        let calls = AtomicU32::new(0);
        let result = retry_with_stall_detection(
            || async {
                if calls.fetch_add(1, Ordering::SeqCst) < 2 {
                    Err(transient())
                } else {
                    Ok(7)
                }
            },
            3,
            1,
            "test",
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.value, 7);
        assert_eq!(result.attempts, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn each_retry_is_counted_by_reason() {
        let sample = |reason: &str| {
            putio_sync_telemetry::global()
                .sample("putio_sync_download_retries_total", &[("reason", reason)])
                .unwrap_or(0.0)
        };
        let before = sample("stall");
        let calls = AtomicU32::new(0);
        retry_with_stall_detection(
            || async {
                if calls.fetch_add(1, Ordering::SeqCst) < 2 {
                    Err(DownloadError::ApiError(
                        "Download stalled - no data received for 60 seconds".into(),
                    ))
                } else {
                    Ok(())
                }
            },
            3,
            1,
            "test",
            None,
        )
        .await
        .unwrap();
        // Other tests may retry in parallel, so only a lower bound is stable
        assert!(sample("stall") - before >= 2.0);
    }

    #[tokio::test(start_paused = true)]
    async fn permanent_errors_are_not_retried() {
        for err in [
            || DownloadError::ApiError("HTTP error 404 Not Found: - Response: gone".into()),
            || {
                DownloadError::IoError(std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "no space",
                ))
            },
        ] {
            let calls = AtomicU32::new(0);
            let result = retry_with_stall_detection(
                || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>(err())
                },
                3,
                1,
                "test",
                None,
            )
            .await;
            assert!(result.is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_max_retries() {
        let calls = AtomicU32::new(0);
        let result = retry_with_stall_detection(
            || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(transient())
            },
            2,
            1,
            "test",
            None,
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn backoff_doubles_is_capped_and_jittered() {
        for _ in 0..50 {
            let first = backoff(1, 2, false);
            assert!(first >= Duration::from_millis(1500) && first <= Duration::from_millis(2500));
            let third = backoff(3, 2, false);
            assert!(third >= Duration::from_secs(6) && third <= Duration::from_secs(10));
            let capped = backoff(30, 2, false);
            assert!(capped <= MAX_BACKOFF.mul_f64(1.25));
            let stalled = backoff(5, 2, true);
            assert!(stalled <= STALL_BACKOFF.mul_f64(1.25));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_stop_cuts_the_backoff_wait_short() {
        let stop = StopSignal::default();
        stop.stop();
        let result = retry_with_stall_detection(
            || async { Err::<(), _>(transient()) },
            3,
            30,
            "test",
            Some(&stop),
        )
        .await;
        assert!(matches!(result, Err(DownloadError::Stopped)));
    }
}

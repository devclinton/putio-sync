//! Utility functions for downloads

use crate::error::{DownloadError, DownloadResult};
use std::future::Future;
use std::time::Duration;
use tracing::{error, info, warn};

/// Result of retry operation with attempt count
pub struct RetryResult<T> {
    pub value: T,
    pub attempts: u32,
}

/// Retry a future with special handling for stalled downloads
///
/// This function retries async operations with exponential backoff,
/// but uses shorter delays for stalled downloads to restart quickly.
///
/// # Arguments
///
/// * `operation` - The async operation to retry
/// * `max_retries` - Maximum number of retry attempts
/// * `initial_delay` - Initial delay in seconds
/// * `operation_name` - Name of the operation for logging
///
/// # Returns
///
/// Returns the result along with the number of attempts made
pub async fn retry_with_stall_detection<F, Fut, T>(
    mut operation: F,
    max_retries: u32,
    initial_delay: u64,
    operation_name: &str,
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

                // Use shorter delay for stalled downloads, longer for other errors
                let delay = if is_stall_error {
                    Duration::from_secs(3) // 3 second delay for stalled downloads
                } else {
                    Duration::from_secs(initial_delay * (1 << (attempt - 1).min(4)))
                    // Exponential backoff for other errors
                };

                if is_stall_error {
                    warn!(
                        "{} stalled (attempt {}/{}): {}. Restarting download in {}s...",
                        operation_name,
                        attempt,
                        max_retries,
                        e,
                        delay.as_secs()
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

                tokio::time::sleep(delay).await;
            }
        }
    }
}

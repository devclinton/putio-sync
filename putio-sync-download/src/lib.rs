//! Download management for Put.io sync
//!
//! This crate provides download orchestration, streaming downloads,
//! worker pools, and progress tracking for Put.io file synchronization.

pub mod blocklist;
pub mod error;
pub mod files;
pub mod manager;
pub mod progress;
pub mod rate_limiter;
pub mod stream;
pub mod utils;
pub mod worker;

// Re-export commonly used types
pub use blocklist::{blocked_extension, default_blocked_extensions, is_blocked};
pub use error::{DownloadError, DownloadResult};
pub use files::{
    check_resume, cleanup_partial, create_temp_path, get_original_path, is_partial_download,
};
pub use manager::{
    ActiveDownload, DownloadConfig, DownloadFile, DownloadManager, DownloadOperationResult,
    SyncStats, WorkerStats,
};
pub use progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
pub use rate_limiter::RateLimiter;
pub use stream::{stream_download, StreamDownloadResult, DEFAULT_BUFFER_SIZE};
pub use utils::{retry_with_stall_detection, RetryResult};
pub use worker::{DownloadJob, DownloadWorkerPool, WorkerResult};

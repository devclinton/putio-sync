//! Download management for Put.io sync
//!
//! This crate provides download orchestration, streaming downloads,
//! worker pools, and progress tracking for Put.io file synchronization.

pub mod blocklist;
pub mod dry_run;
pub mod error;
pub mod files;
pub mod finished;
pub mod manager;
pub mod pipeline;
pub mod progress;
pub mod rate_limiter;
pub mod stream;
pub mod sync_status;
pub mod transfer_events;
pub mod utils;
pub mod verify;
pub mod worker;

// Re-export commonly used types
pub use blocklist::{blocked_extension, default_blocked_extensions, is_blocked};
pub use dry_run::{dry_run, DryRunItem, DryRunReport, Tally, DRY_RUN_LIMIT, DRY_RUN_SAMPLES};
pub use error::{DownloadError, DownloadResult};
pub use files::{
    check_resume, cleanup_partial, crc32_prefix, create_temp_path, get_original_path,
    is_partial_download, valid_crc32, Crc32,
};
pub use manager::{
    ActiveDownload, DeferFuture, DownloadConfig, DownloadFile, DownloadManager,
    DownloadOperationResult, PathRedirect, PreviewDownload, PreviewItem, RedirectFuture,
    RemoteDeleteGate, RulePreview, SyncStats, TransfersSnapshot, TreeDecision, TreeFile,
    TreeReport, VerifySettings, WorkerStats, PREVIEW_DOWNLOADS, PREVIEW_ITEMS,
};
pub use progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
pub use rate_limiter::RateLimiter;
pub use stream::{stream_download, StopSignal, StreamDownloadResult, DEFAULT_BUFFER_SIZE};
pub use sync_status::{SyncOutcome, SyncRequestError, SyncStatus};
pub use utils::{retry_with_stall_detection, RetryResult};
pub use verify::{
    Verifier, VerifyControlError, VerifyRequest, VerifyRunner, VerifyStartError, VerifyStatus,
};
pub use worker::{DownloadJob, DownloadWorkerPool, WorkerResult};

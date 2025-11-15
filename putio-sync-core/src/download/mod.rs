//! Download management module
//!
//! This module provides functionality for downloading files from Put.io,
//! including worker pools, streaming, chunking, and progress tracking.

pub mod chunk;
pub mod manager;
pub mod progress;
pub mod stream;
pub mod worker;

pub use chunk::{
    calculate_chunks, download_chunk, supports_range_requests, Chunk, ResumeInfo,
    DEFAULT_CHUNK_SIZE, MIN_CHUNK_FILE_SIZE,
};
pub use manager::{DownloadFile, DownloadManager, DownloadOperationResult};
pub use progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
pub use stream::{stream_download, StreamDownloadResult, DEFAULT_BUFFER_SIZE};
pub use worker::{DownloadJob, DownloadResult, WorkerPool};

//! Download management module
//!
//! This module provides functionality for downloading files from Put.io,
//! including worker pools, streaming, chunking, and progress tracking.

pub mod stream;
pub mod worker;

pub use stream::{stream_download, StreamDownloadResult, DEFAULT_BUFFER_SIZE};
pub use worker::{DownloadJob, DownloadResult, WorkerPool};

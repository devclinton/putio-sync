//! Download management module
//!
//! This module provides functionality for downloading files from Put.io,
//! including worker pools, streaming, chunking, and progress tracking.

pub mod worker;

pub use worker::{DownloadJob, DownloadResult, WorkerPool};

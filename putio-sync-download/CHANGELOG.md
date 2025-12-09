# Changelog

All notable changes to putio-sync-download will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Added `delete_after_download` configuration option to DownloadConfig
- Automatic deletion of files from Put.io after successful download when enabled
- Configurable buffer size for streaming downloads via `buffer_size_mb` parameter

### Changed
- DownloadConfig now includes `delete_after_download` field (default: false)
- `process_queue()` now deletes files from Put.io after download if configured

## [0.2.0] - 2025-12-07

### Added
- Initial release of standalone putio-sync-download crate
- Download management extracted from putio-sync-core
- Modular architecture:
  * `manager` - DownloadManager for orchestration
  * `progress` - ProgressTracker for download progress
  * `stream` - Streaming download with buffering
  * `worker` - DownloadWorkerPool for concurrent downloads
  * `files` - File utilities (check_resume, create_temp_path, cleanup_partial)
  * `rate_limiter` - Bandwidth throttling
  * `utils` - Retry logic with stall detection
- Support for resumable downloads (without chunking)
- Configurable worker pools
- Progress tracking with callbacks
- Rate limiting support

### Changed
- **BREAKING**: Renamed `WorkerPool` to `DownloadWorkerPool`
- Reorganized code into focused modules for better maintainability
- Moved file utilities from manager to dedicated files module
- Moved rate limiting from stream to dedicated rate_limiter module
- Moved retry logic from manager to utils module

### Removed
- **BREAKING**: Removed all chunking functionality
  * Deleted `chunk.rs` module entirely
  * Removed `Chunk`, `ResumeInfo` structs
  * Removed `calculate_chunks()`, `download_chunk()` functions
  * Removed `supports_range_requests()` function
  * Removed chunk-related constants (MIN_CHUNK_FILE_SIZE, DEFAULT_CHUNK_SIZE)
- Streaming downloads no longer use chunking

### Migrated
- Extracted from putio-sync-core 0.1.1 as part of crate restructuring
- All download functionality now in standalone crate

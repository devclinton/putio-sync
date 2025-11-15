# Changelog

All notable changes to putio-sync-core will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- SQLite database for download state tracking
- Download history and statistics storage
- Chunk tracking for resumable downloads
- Automatic database migrations
- Database models for downloads, chunks, history, and stats
- Repository pattern for clean database access
- DownloadRepository for CRUD operations on downloads and chunks
- HistoryRepository with query filtering and pagination
- StatsRepository for aggregation and statistics by extension
- Extended download configuration with new options
  - workers (concurrent download workers, default: 2)
  - target_directory (download location)
  - buffer_size_mb (streaming buffer, default: 16MB)
  - skip_existing (skip already downloaded files)
  - delete_after_download (auto-delete from Put.io)
  - progress_style enum (PerFile, Overall, Individual, Rotating, None)
  - sort_order enum (ByDate, ByName, ByExtension, BySize)
  - reverse_sort (reverse sort order)
- ProgressStyle and SortOrder enums for download configuration
- Updated config builder with new download options
- Backward compatibility with deprecated max_concurrent option
- Cross-platform filename sanitization
  - Handles invalid characters (/, \, :, *, ?, ", <, >, |)
  - Handles Windows reserved names (CON, PRN, AUX, NUL, COM1-9, LPT1-9)
  - Handles path length limits (255 chars for filenames, 260 for paths)
  - Preserves Unicode characters
  - Reproducible transformations
  - Path and filename sanitization functions
- Concurrent download worker pool
  - Configurable worker count
  - Job queue with async task distribution
  - Semaphore-based concurrency control
  - Graceful shutdown handling
  - Error handling and result reporting
- Streaming downloads with buffering
  - Direct-to-disk streaming to minimize memory usage
  - Configurable buffer size (default: 16MB)
  - Intermediate files with .downloading extension and . prefix
  - Atomic file rename on completion
  - File size validation and integrity checks
  - Automatic cleanup on failure
- Progress tracking system
  - Per-file progress with bytes downloaded, percentage, speed, and ETA
  - Overall progress tracking for multiple downloads
  - Moving average speed calculation for accurate estimates
  - Customizable progress callback interface
  - Real-time progress updates during downloads
- Chunked downloads for large files
  - HTTP Range request support for partial downloads
  - Configurable chunk size (default: 10MB)
  - Automatic chunking for files > 50MB
  - Server capability detection for range requests
  - Fallback to streaming for servers without range support
- Resumable downloads
  - Resume detection for partial downloads
  - Chunk completion tracking in database
  - Automatic recovery from interrupted downloads
  - Byte-level resume accuracy
  - Support for resuming multi-chunk downloads
- Remote file deletion handling
  - 404 error detection during downloads
  - Automatic cleanup of partial files on 404
  - Database status updates for failed downloads
  - Graceful handling of files deleted from Put.io
- Download manager orchestration
  - Unified interface for all download operations
  - Automatic selection of download strategy (streaming/chunked/resume)
  - Integration with database for state persistence
  - Support for custom progress callbacks
  - Configurable download options (workers, buffer size, skip existing, etc.)
  - HTTP client access for direct operations
- Repository helper methods
  - find_by_file_id for simpler file lookups
  - update_chunk_status_by_number for chunk tracking
- API client enhancements
  - http_client() method for accessing underlying HTTP client
- Comprehensive unit tests for all modules (158 tests total, all passing)

## [0.1.1] - 2025-11-14

### Fixed
- Updated AccountInfo model to match actual Put.io API response format
  - Changed from flat fields (disk_quota, disk_used, disk_available) to nested DiskInfo structure
  - Matches Put.io's actual response: disk.size, disk.used, disk.avail
- Updated all tests and documentation to use new AccountInfo structure

### Added
- Initial crate structure
- Structured logging using `tracing` crate
- Environment variable support for log level configuration (RUST_LOG)
- Custom error types using `thiserror`
- CoreError enum with variants for different error categories
- CoreResult type alias for ergonomic error handling
- Configuration management with TOML format
- Default config location at ~/.putiosync/config.toml
- Support for custom config paths via environment variable and CLI
- Auto-creation of config directory
- Secure file permissions (0600) for config files on Unix systems
- Configuration validation with comprehensive checks
- Default values for all config options
- Builder pattern for config creation with fluent API
- Put.io API client with reqwest HTTP client
- Multi-method authentication support (OAuth token and app-specific password)
- OAuth2 token authentication via Bearer token
- App-specific password authentication via HTTP Basic Auth
- Flexible authentication configuration with AuthMethod enum
- Dynamic auth header generation based on auth method
- Base64 encoding for HTTP Basic Auth credentials
- Multiple API client constructors (from_auth_config, with_token, with_credentials)
- Backward compatible authentication (existing token auth continues to work)
- Request retry with exponential backoff (up to 3 attempts)
- Request/response logging with tracing
- Mock server tests for API client reliability
- Data models for Put.io API entities
- FileEntry, Transfer, AccountInfo structs with full field coverage
- JSON serialization/deserialization with serde
- ApiResponse<T> wrapper for generic API responses
- Comprehensive tests for all data model parsing
- File listing API endpoint (list_files)
- File details API endpoint (get_file)
- Download URL generation (get_download_url)
- Support for folder navigation via parent_id
- Unit tests for all file/folder operations with mock server
- Account information API endpoint (get_account_info)
- Token verification capability (verify_token)
- Returns complete account details (username, disk usage, quota)
- Unit tests for account operations and token validation
- Path resolution to convert folder paths to IDs
- Case-insensitive folder matching for navigation
- Folder traversal logic with helpful error messages
- Comprehensive tests for path resolution with mock API
- File filtering by name, type, and size
- Glob pattern matching for name filters
- File type detection and filtering (file, folder, video, audio, image, archive)
- Size comparison logic with min/max support
- Human-readable size parsing (KB, MB, GB, TB)
- Sorting by name, size, and date
- Reverse sort order support
- Comprehensive unit tests for all filtering and sorting logic

### CI/CD
- GitHub Actions CI workflow for multi-platform testing (Linux, macOS, Windows)
- Automated formatting checks with rustfmt
- Automated linting with clippy (all warnings as errors)
- Automated test execution across all platforms
- Release build verification in CI pipeline
- Dependency caching for faster CI runs
- Beta Rust version testing (continue-on-error)

### Development
- Makefile with comprehensive development tasks
- Local CI execution scripts for Unix (bash) and Windows (PowerShell)
- Development workflow commands (build, test, fmt, lint, watch, etc.)
- Code coverage support (cargo-tarpaulin integration)
- Security audit support (cargo-audit integration)
- Binary size analysis support (cargo-bloat integration)

### Documentation
- Comprehensive inline documentation for all public APIs
- Module-level documentation for all modules
- Function and struct documentation with usage examples
- docs.rs metadata configuration in Cargo.toml
- Documentation builds with zero warnings
- `#![warn(missing_docs)]` lint enabled

### Testing
- Comprehensive unit test coverage (76 tests, 80%+ coverage goal)
- Unit tests for all config operations (save, load, validate, builder)
- Unit tests for multi-method authentication (token and credentials)
- Unit tests for auth header generation (Bearer and Basic)
- Unit tests for API client with mocked HTTP server (mockito)
- Unit tests for both authentication methods with mock server
- Unit tests for error handling and type conversions
- Unit tests for filtering and sorting logic
- Unit tests for path resolution with mock API responses
- All tests passing with zero warnings
- Mock server tests for retry logic and error handling
- Test coverage for all API data model parsing

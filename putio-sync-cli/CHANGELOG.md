# Changelog

All notable changes to putio-sync-cli will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **BREAKING**: CLI command structure reorganized for better hierarchy
  - `putio-sync list` moved to `putio-sync files list`
  - `putio-sync delete` moved to `putio-sync files delete`
  - `putio-sync create` moved to `putio-sync transfers add`
  - Old commands remain functional but hidden (backward compatible)
- **BREAKING**: `get-token` command renamed to `login`
  - OAuth token acquisition using standard `login` command
  - Token now saves by default (use `--no-save` to prevent saving)
  - Previous behavior was `--save` flag to opt-in to saving

### Removed
- **BREAKING**: Old login command with username/password authentication
  - OAuth token authentication is now the only supported method
  - Use `putio-sync login` to obtain and save OAuth token

### Deprecated
- `putio-sync list` - Use `putio-sync files list` instead (still functional)
- `putio-sync delete` - Use `putio-sync files delete` instead (still functional)
- `putio-sync create` - Use `putio-sync transfers add` instead (still functional)

### Added
- Homebrew formula for macOS and Linux installation
- Local formula testing support
- Homebrew tap repository for easy installation
- Tap testing workflow
- Automated Homebrew formula updates on release
- Version bump workflow for tap repository
- Windows Package Manager (Winget) manifest
- Support for x64 and ARM64 Windows installations
- Automated Winget package submission on release
- PR creation to winget-pkgs repository
- Docker distribution and deployment support
  - Alpine Linux-based Dockerfile with multi-stage build
  - Optimized image size (24MB) with minimal dependencies
  - Non-root user execution for security
  - Volume mounts for /config, /downloads, and /watches directories
  - GitHub Actions workflow for automated Docker builds
  - Multi-platform support (linux/amd64, linux/arm64)
  - Automatic image tagging (branch, tag, semver, sha)
  - Docker layer caching for faster builds
  - Docker Hub integration via GitHub Container Registry
  - .dockerignore file for optimized build context
  - Comprehensive Docker documentation in README
  - Docker Compose examples for daemon and watcher modes
  - RUST_LOG environment variable support in containers
- Create command for adding torrents to Put.io
  - Add torrents from magnet links
  - Upload .torrent files to Put.io
  - Support for multiple sources (files and magnet links)
  - Watch directories for new .torrent files with `--watch` flag
  - Parent folder ID specification with `--parent-id` flag
  - Daemon mode for continuous watching with `--daemon` flag
  - Filesystem watching with notify crate (macOS FSEvents, Linux inotify, Windows ReadDirectoryChangesW)
  - Automatic processing of new .torrent files as they appear
  - Processed file tracking to avoid duplicate uploads
  - Graceful shutdown handling (Ctrl+C support)
  - Configurable check interval with `--interval` flag (default: 60s)
  - Support for multiple watch directories
  - Automatic source type detection (magnet link vs .torrent file)
  - Transfer status display with ID, name, size, and parent folder
  - Human-readable size formatting
  - Comprehensive error handling and validation
  - 4 unit tests for watch module and create command
- History command for viewing download and delete history
  - View download and delete history from SQLite database
  - Action filtering with `--action` flag (downloaded, deleted)
  - Date range filtering with `--since` and `--until` flags
  - Multiple date format support (YYYY-MM-DD, YYYY/MM/DD, MM/DD/YYYY, DD-MMM-YYYY)
  - Pagination with `--limit` and `--offset` flags (default limit: 50)
  - Three output formats: table (default), JSON, CSV
  - Human-readable formatting for sizes, durations, and speeds
  - Pagination information showing entry range and total count
  - Filter display in table output
  - CSV escaping for special characters
  - Comprehensive error messages and guidance
  - 8 unit tests for formatting and validation functions
- Stat command for displaying download statistics
  - Display overall download statistics (count, size, time, speed)
  - Extension-based breakdown with percentages (`--by-extension`)
  - Date range filtering (`--since`, `--until`)
  - Multiple output formats (table, JSON)
  - Human-readable formatting for sizes, durations, and speeds
  - Statistics sourced from SQLite download database
- Delete command for removing files from Put.io
  - Delete specific files by ID
  - Delete all files with `--all` flag
  - Filter files with glob patterns (`--filter`)
  - Date-based deletion with `--before` option
  - Recursive folder deletion with `--recursive` flag
  - Dry-run mode to preview deletions without executing
  - Confirmation prompts for safety (skip with `--yes`)
  - File summary display with size information
  - Progress feedback during deletion
  - Comprehensive error handling and recovery
- Download command with comprehensive options
- Support for filtering, sorting, workers, and chunking
- Daemon mode for continuous syncing
- CLI argument parsing for download configuration
- Support for chunk size and buffer size specification
- Resume control and skip-existing functionality
- Delete-after-download option
- Multiple progress style options (per-file, overall, individual, rotating, none)
- Progress bars with multiple display styles
- Per-file and overall progress tracking
- Speed and ETA display
- Auto-detection of non-interactive terminals
- Integration with core library's progress callback
- Download command execution with file selection logic
- Graceful shutdown handling (Ctrl+C support)
- Download summary display with statistics
- Automatic target directory creation
- Database initialization for download tracking
- Integration with DownloadManager from core library
- Daemon mode for continuous background syncing
- Periodic file listing with configurable interval
- Filter-based automatic downloads in daemon mode
- Cycle-based logging for daemon operations
- Responsive shutdown in daemon mode (checks every 5 seconds)
- Error recovery with automatic retry in daemon mode

## [0.1.1] - 2025-11-14

### Fixed
- OAuth token exchange now uses GET request instead of POST to match Put.io's non-standard implementation
- Updated AccountInfo model to match actual Put.io API response format (nested disk object)
- Fixed token verification and account info retrieval

### Added
- Better error logging for OAuth token exchange failures
- Support for urlencoding in OAuth parameters
- Initial crate structure
- Command-line argument parsing with clap
- Global options: config path, verbosity, quiet mode
- Login command with token and interactive options
- Login command with multi-method authentication support
  - OAuth token authentication (--token flag)
  - App-specific password authentication (--username and --password flags)
  - Interactive mode with authentication method selection
  - Automatic method detection based on provided flags
  - Token verification against Put.io API
  - Credentials verification for both auth methods
  - Automatic config file creation and updates
  - Secure file permissions for config file (0600)
  - Credential masking in logs (tokens and passwords)
  - Security warnings for CLI credential usage
  - Account information display after successful login
  - Integration tests for login functionality
- Get-Token command for easy OAuth token acquisition
  - Step-by-step instructions for obtaining OAuth token
  - Automatic browser opening to Put.io settings page
  - Optional --no-browser flag to skip browser opening
  - Token verification before saving
  - Optional --save flag to automatically save token to config
  - Interactive token input with verification
  - Account information display after verification
- List command for browsing Put.io files and folders
  - Support for listing by ID or path
  - Recursive listing option for deep folder traversal
  - Name pattern filtering with glob support
  - File type filtering (file, folder, video, audio, image, archive)
  - Size filtering with human-readable parsing (KB, MB, GB, TB)
  - Sorting by name, size, and date
  - Reverse sort order support
  - Multiple output formats: table, json, csv, tree
  - Output to file support via --output-file flag
  - Table formatter with column alignment
  - JSON formatter with pretty printing
  - CSV formatter with proper escaping
  - Tree formatter with box-drawing characters
  - Human-readable size and date formatting
  - Comprehensive error handling and helpful messages
  - Unit tests for filtering, sorting, and formatting logic
- Version and help commands
- Verbosity control via -v flags (WARN -> INFO -> DEBUG -> TRACE)
- Quiet mode for minimal output (ERROR only)
- Colored log output support with terminal detection
- Logging integration with tracing

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
- Convenience commands for login and list operations
- Code coverage support (cargo-tarpaulin integration)
- Security audit support (cargo-audit integration)
- Binary size analysis support (cargo-bloat integration)
- Development tool setup automation

### Documentation
- Comprehensive inline documentation for all public APIs
- Module-level documentation for all modules
- Command and function documentation with usage examples
- docs.rs metadata configuration in Cargo.toml
- Documentation builds with zero warnings

### Testing
- Comprehensive unit test coverage (31 tests, 70%+ coverage goal)
- Unit tests for multi-method authentication (token and credentials)
- Unit tests for credential masking logic
- Unit tests for all output formatters (table, json, csv, tree)
- Unit tests for CLI parsing and argument handling
- Unit tests for logging verbosity mapping
- Unit tests for filter and sorting application
- Integration tests for login command workflow
- All tests passing with zero warnings
- Test coverage for edge cases (empty inputs, special characters)

### Dependencies
- Added `open = "5.0"` for browser opening in get-token command

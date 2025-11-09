# Changelog

All notable changes to putio-sync-core will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
- OAuth2 token authentication via Bearer token
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

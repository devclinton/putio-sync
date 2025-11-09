# Changelog

All notable changes to putio-sync-cli will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial crate structure
- Command-line argument parsing with clap
- Global options: config path, verbosity, quiet mode
- Login command with token and interactive options
- Login command for OAuth token management
  - Interactive and non-interactive modes
  - Token verification against Put.io API
  - Automatic config file creation and updates
  - Secure file permissions for config file (0600)
  - Token masking in logs
  - Security warnings for CLI token usage
  - Account information display after successful login
  - Integration tests for login functionality
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

### Testing
- Comprehensive unit test coverage (27 tests, 70%+ coverage goal)
- Unit tests for all output formatters (table, json, csv, tree)
- Unit tests for CLI parsing and argument handling
- Unit tests for logging verbosity mapping
- Unit tests for filter and sorting application
- Integration tests for login command workflow
- All tests passing with zero warnings
- Test coverage for edge cases (empty inputs, special characters)

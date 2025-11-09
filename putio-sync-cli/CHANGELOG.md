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
- List command with filtering, sorting, and output formats
- Version and help commands
- Verbosity control via -v flags (WARN -> INFO -> DEBUG -> TRACE)
- Quiet mode for minimal output (ERROR only)
- Colored log output support with terminal detection
- Logging integration with tracing

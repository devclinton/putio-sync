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

# Changelog

All notable changes to putio-sync-daemon will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2024-12-07

### Added
- Initial release of putio-sync-daemon crate
- PutioDaemon orchestrator for coordinating all sync operations
- ConfigWatcher for hot-reloading configuration changes
- Integration with putio-sync-download for download management
- Integration with putio-sync-file-manager for torrent/magnet watching
- Integration with putio-sync-client for Put.io API communication
- Integration with putio-sync-db for data persistence
- Error handling with DaemonError type
- Configuration file watching with notify

### Changed
- None (initial release)

### Removed
- None (initial release)

### Notes
- This is the initial structure for the daemon crate
- Full daemon orchestration logic to be refined in future updates
- Provides the foundation for unified daemon mode operation

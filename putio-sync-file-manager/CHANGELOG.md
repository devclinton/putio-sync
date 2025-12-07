# Changelog

All notable changes to putio-sync-file-manager will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2024-12-07

### Added
- Initial release of putio-sync-file-manager crate
- TorrentWatcher for monitoring directories for .torrent and .magnet files
- File watcher daemon with notify integration
- Support for both .torrent and .magnet file types
- Automatic file processing with Put.io API integration
- SHA-256 checksumming to prevent duplicate processing
- Database integration for tracking processed files

### Changed
- Merged watch/mod.rs and watch/core.rs into unified watcher.rs
- Enhanced magnet file support throughout the codebase
- Improved error handling with FileManagerError type

### Removed
- None (initial release)

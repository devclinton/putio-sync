# Changelog

All notable changes to putio-sync-db will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2025-12-07

### Added
- Initial release of standalone putio-sync-db crate
- Database layer extracted from putio-sync-core
- Repositories:
  * DownloadRepository - CRUD operations for downloads
  * HistoryRepository - Download history tracking
  * StatsRepository - Statistics aggregation
  * QueueRepository - Download queue management
  * ProcessedTorrentsRepository - Torrent deduplication with SHA-256 checksums
- Database connection pool management (max 5 connections)
- SQLite schema with tables:
  * downloads - Completed downloads
  * download_history - Historical records
  * download_stats - Aggregate statistics
  * download_queue - Download queue
  * processed_torrents - Torrent deduplication
- Optional database mode - system can operate without database

### Removed
- **BREAKING**: Removed all chunking functionality
  * Deleted `ChunkStatus` enum
  * Deleted `DownloadChunk` struct
  * Removed `insert_chunk()` method from DownloadRepository
  * Removed `get_chunks()` method from DownloadRepository
  * Removed `update_chunk_status()` method from DownloadRepository
  * Removed `update_chunk_status_by_number()` method from DownloadRepository
  * Removed `delete_chunks()` method from DownloadRepository
  * Removed `CREATE_DOWNLOAD_CHUNKS_TABLE` from schema
  * Removed all chunk-related indexes

### Migrated
- Extracted from putio-sync-core 0.1.1 as part of crate restructuring
- All database functionality now in standalone crate

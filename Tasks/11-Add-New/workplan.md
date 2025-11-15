# putio-sync Workplan - Phase 2: Download, Delete, Stat, History & Create Commands

## Overview
This workplan covers the implementation of core functionality for the `putio-sync` project:
- Download command with workers, progress tracking, filtering, database tracking, chunking, resuming, and daemon mode
- Delete command for removing files from Put.io
- Stat command for download statistics
- History command for viewing download/delete history
- Create/Add command for uploading torrents
- Test-driven development approach with human testing phases

---

## 1. Database Foundation

### 1.1 SQLite Database Setup
**Feature**: Local state database for tracking downloads and history

**Database Schema**:
```sql
-- Downloads table
CREATE TABLE downloads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    local_path TEXT NOT NULL,
    downloaded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    status TEXT NOT NULL, -- 'completed', 'in_progress', 'failed'
    UNIQUE(file_id, filename, file_size)
);

-- Download chunks table (for resumable downloads)
CREATE TABLE download_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    download_id INTEGER NOT NULL,
    chunk_index INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    status TEXT NOT NULL, -- 'pending', 'downloading', 'completed'
    FOREIGN KEY(download_id) REFERENCES downloads(id),
    UNIQUE(download_id, chunk_index)
);

-- Download history table
CREATE TABLE download_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    extension TEXT,
    action TEXT NOT NULL, -- 'downloaded', 'deleted'
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    duration_seconds INTEGER,
    average_speed INTEGER
);

-- Download statistics table
CREATE TABLE download_stats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    total_downloads INTEGER DEFAULT 0,
    total_bytes INTEGER DEFAULT 0,
    total_time_seconds INTEGER DEFAULT 0,
    last_updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

**Tasks**:
- [ ] Create `putio-sync-core/src/db/mod.rs` module
- [ ] Create `putio-sync-core/src/db/schema.rs` with SQL migrations
- [ ] Create `putio-sync-core/src/db/models.rs` with database models
- [ ] Implement `Database` struct with sqlx
- [ ] Implement `Database::new(path)` to initialize database
- [ ] Implement automatic migrations on startup
- [ ] Add methods for CRUD operations on downloads table
- [ ] Add methods for chunk tracking
- [ ] Add methods for history tracking
- [ ] Add methods for statistics updates
- [ ] Write unit tests for database operations
- [ ] Write integration tests

**Commit**: "feat(core): implement SQLite database for download tracking"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- SQLite database for download state tracking
- Download history and statistics storage
- Chunk tracking for resumable downloads
- Automatic database migrations
```

---

### 1.2 Database Models & Repository Pattern
**Feature**: Domain models and repository pattern for database access

**Tasks**:
- [ ] Create `putio-sync-core/src/db/downloads.rs` repository
- [ ] Create `putio-sync-core/src/db/history.rs` repository
- [ ] Create `putio-sync-core/src/db/stats.rs` repository
- [ ] Implement `DownloadRepository` with CRUD methods
- [ ] Implement `HistoryRepository` with query methods
- [ ] Implement `StatsRepository` with aggregation methods
- [ ] Add filtering and sorting capabilities
- [ ] Add pagination support for large datasets
- [ ] Write unit tests for repositories

**Commit**: "feat(core): add repository pattern for database access"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Repository pattern for clean database access
- Download, history, and stats repositories
- Query filtering and pagination support
```

---

## 2. Download Core Infrastructure

### 2.1 Download Configuration
**Feature**: Extend config to support download options

**Config Updates**:
```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadConfig {
    pub workers: usize,  // Default: 2
    pub target_directory: PathBuf,  // Default: ~/Downloads/putio
    pub chunk_size_mb: Option<usize>,  // Default: None (disabled)
    pub buffer_size_mb: usize,  // Default: 16
    pub resume_enabled: bool,  // Default: true
    pub skip_existing: bool,  // Default: false
    pub delete_after_download: bool,  // Default: false
    pub progress_style: ProgressStyle,  // Default: PerFile
    pub sort_order: SortOrder,  // Default: ByDate
    pub reverse_sort: bool,  // Default: false
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ProgressStyle {
    PerFile,      // One bar per file + overall
    Overall,      // Overall progress only
    Individual,   // Individual files only
    Rotating,     // Rotate between files and overall
    None,         // No progress bars
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum SortOrder {
    ByDate,
    ByName,
    ByExtension,
    BySize,
}
```

**Tasks**:
- [ ] Update `putio-sync-core/src/config.rs` with download config
- [ ] Add defaults for all download options
- [ ] Implement validation for download config
- [ ] Add path expansion for target directory
- [ ] Write unit tests for config validation

**Commit**: "feat(core): extend config for download options"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Download configuration options
- Support for workers, chunking, resume, and progress styles
- Sort order configuration
```

---

### 2.2 File Sanitization
**Feature**: Sanitize filenames for filesystem compatibility

**Tasks**:
- [ ] Create `putio-sync-core/src/fs/mod.rs` module
- [ ] Create `putio-sync-core/src/fs/sanitize.rs`
- [ ] Implement filename sanitization for Windows/Linux/macOS
- [ ] Handle reserved characters (/, \, :, *, ?, ", <, >, |)
- [ ] Handle reserved names (CON, PRN, AUX, NUL, etc. on Windows)
- [ ] Handle path length limits
- [ ] Preserve Unicode characters where safe
- [ ] Make sanitization reproducible (same input = same output)
- [ ] Write comprehensive unit tests for edge cases

**Commit**: "feat(core): implement cross-platform filename sanitization"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Cross-platform filename sanitization
- Support for Windows, Linux, and macOS filesystem restrictions
- Reproducible path transformations
```

---

### 2.3 Download Worker Pool
**Feature**: Concurrent download worker pool

**Tasks**:
- [ ] Create `putio-sync-core/src/download/mod.rs` module
- [ ] Create `putio-sync-core/src/download/worker.rs`
- [ ] Implement `WorkerPool` struct with tokio
- [ ] Implement configurable worker count
- [ ] Implement download queue with priority support
- [ ] Implement work distribution to workers
- [ ] Add graceful shutdown handling
- [ ] Add error handling and retry logic
- [ ] Write unit tests for worker pool
- [ ] Write integration tests

**Commit**: "feat(core): implement concurrent download worker pool"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Concurrent download worker pool
- Configurable worker count
- Download queue with priority support
- Graceful shutdown handling
```

---

### 2.4 Streaming Download with Buffering
**Feature**: Stream downloads to disk with configurable buffer

**Tasks**:
- [ ] Create `putio-sync-core/src/download/stream.rs`
- [ ] Implement streaming download with reqwest
- [ ] Implement configurable buffer size (default 16MB)
- [ ] Stream directly to disk to minimize memory usage
- [ ] Handle intermediate files (`.downloading` extension)
- [ ] Handle file prefix with "." for in-progress files
- [ ] Implement atomic file moves on completion
- [ ] Add integrity checks (file size validation)
- [ ] Write unit tests with mock HTTP server
- [ ] Write integration tests

**Commit**: "feat(core): implement streaming downloads with buffering"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Streaming downloads to minimize memory usage
- Configurable buffer size (default 16MB)
- Intermediate file handling with .downloading extension
- Atomic file completion
```

---

### 2.5 Chunked Downloads
**Feature**: Split large files into chunks for parallel download

**Tasks**:
- [ ] Create `putio-sync-core/src/download/chunked.rs`
- [ ] Implement chunk calculation based on file size and chunk size config
- [ ] Download chunks in order to avoid merge overhead
- [ ] Track chunk completion in database
- [ ] Implement chunk merging on completion
- [ ] Handle partial chunk failures
- [ ] Cleanup temporary chunk files
- [ ] Write unit tests for chunking logic
- [ ] Write integration tests

**Commit**: "feat(core): implement chunked downloads for large files"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Chunked downloads for large files
- Configurable chunk size
- In-order chunk downloads to minimize merging
- Chunk state tracking in database
```

---

### 2.6 Resumable Downloads
**Feature**: Resume interrupted downloads

**Tasks**:
- [ ] Create `putio-sync-core/src/download/resume.rs`
- [ ] Check for existing `.downloading` files on startup
- [ ] Query database for incomplete downloads
- [ ] Implement HTTP Range requests for resuming
- [ ] Validate partial files before resuming
- [ ] Handle cases where remote file changed/deleted
- [ ] Clean up corrupted partial downloads
- [ ] Write unit tests for resume logic
- [ ] Write integration tests

**Commit**: "feat(core): implement resumable downloads"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Resumable downloads with HTTP Range requests
- Automatic detection of incomplete downloads
- Validation of partial files before resuming
- Cleanup of corrupted downloads
```

---

### 2.7 Download Progress Tracking
**Feature**: Track download progress with multiple display styles

**Tasks**:
- [ ] Create `putio-sync-core/src/download/progress.rs`
- [ ] Implement progress tracking per file
- [ ] Implement overall progress tracking
- [ ] Calculate download speed (bytes/sec)
- [ ] Calculate ETA based on current speed
- [ ] Track percentage based on file size (not file count)
- [ ] Provide progress data to CLI layer
- [ ] Write unit tests for progress calculations

**Commit**: "feat(core): implement download progress tracking"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Download progress tracking per file and overall
- Download speed and ETA calculations
- Size-based progress (not file count)
```

---

### 2.8 Handle Remote File Deletion
**Feature**: Handle files deleted on Put.io during download

**Tasks**:
- [ ] Detect 404 errors during download
- [ ] Delete local partial file when remote deleted
- [ ] Update database to mark download as failed
- [ ] Log appropriate error message
- [ ] Write unit tests for deletion handling
- [ ] Write integration tests

**Commit**: "feat(core): handle remote file deletion during download"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Detection of remote file deletion during download
- Automatic cleanup of partial local files
- Database state updates for failed downloads
```

---

### 2.9 Download Manager
**Feature**: High-level download manager orchestrating all download features

**Tasks**:
- [ ] Create `putio-sync-core/src/download/manager.rs`
- [ ] Implement `DownloadManager` struct
- [ ] Coordinate workers, progress, resume, and chunking
- [ ] Implement file sorting (by date, name, extension, size)
- [ ] Implement filtering (glob patterns, --all flag)
- [ ] Check database for previously downloaded files (if skip_existing)
- [ ] Handle delete-after-download if configured
- [ ] Record downloads in history
- [ ] Update statistics
- [ ] Write comprehensive unit tests
- [ ] Write integration tests

**Commit**: "feat(core): implement download manager orchestration"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Download manager for orchestrating all download features
- File sorting and filtering
- Skip existing downloads
- Delete-after-download support
- History and statistics recording
```

---

## 3. Download Command (CLI)

### 3.1 Download Command Interface
**Feature**: CLI interface for download command

**Command Signature**:
```
putio-sync download [OPTIONS] [FILES]...

Arguments:
  [FILES]...                Specific file IDs to download

Options:
  -a, --all                 Download all files
  -f, --filter <PATTERN>    Filter files with glob pattern (e.g., "*.mkv")
  -d, --dir <PATH>          Download directory [default: from config]
  -w, --workers <N>         Number of concurrent workers [default: from config]
  --chunk-size <SIZE>       Chunk size for large files (e.g., 256MB) [default: from config]
  --buffer-size <SIZE>      Buffer size per file (e.g., 16MB) [default: from config]
  --no-resume               Disable resume of partial downloads
  --skip-existing           Skip files already downloaded
  --delete-after            Delete from Put.io after download
  --sort <ORDER>            Sort order [possible values: date, name, extension, size]
  --reverse                 Reverse sort order
  --progress <STYLE>        Progress style [possible values: per-file, overall, individual, rotating, none]
  --daemon                  Run in daemon mode (periodic sync)
  --interval <SECONDS>      Daemon check interval [default: 300]
  -h, --help                Print help
```

**Tasks**:
- [ ] Create `putio-sync-cli/src/commands/download.rs`
- [ ] Define `DownloadArgs` struct with clap
- [ ] Implement all CLI flags and options
- [ ] Add validation for conflicting options (--all vs FILES)
- [ ] Parse human-readable sizes (KB, MB, GB)
- [ ] Implement help text and examples
- [ ] Write unit tests for argument parsing

**Commit**: "feat(cli): add download command interface"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Download command with comprehensive options
- Support for filtering, sorting, workers, and chunking
- Daemon mode for continuous syncing
```

---

### 3.2 Progress Bar Implementation
**Feature**: Visual progress bars for download tracking

**Tasks**:
- [ ] Add `indicatif` dependency for progress bars
- [ ] Create `putio-sync-cli/src/ui/progress.rs`
- [ ] Implement per-file progress bars (filename, %, speed)
- [ ] Implement overall progress bar (total size %, files remaining)
- [ ] Implement ProgressStyle::PerFile (default)
- [ ] Implement ProgressStyle::Overall
- [ ] Implement ProgressStyle::Individual
- [ ] Implement ProgressStyle::Rotating
- [ ] Detect non-interactive terminals and disable progress
- [ ] Write unit tests for progress rendering

**Commit**: "feat(cli): implement progress bars for downloads"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Progress bars with multiple display styles
- Per-file and overall progress tracking
- Speed and ETA display
- Auto-detection of non-interactive terminals
```

---

### 3.3 Download Command Logic
**Feature**: Implement download command execution

**Tasks**:
- [ ] Implement file selection logic (--all, --filter, FILES)
- [ ] Query Put.io API for file list
- [ ] Apply filters and sorting
- [ ] Check database for existing downloads (if --skip-existing)
- [ ] Initialize DownloadManager from core
- [ ] Start download workers
- [ ] Display progress based on --progress style
- [ ] Handle graceful shutdown (Ctrl+C)
- [ ] Display summary on completion (files downloaded, total size, time, avg speed)
- [ ] Handle errors gracefully
- [ ] Write integration tests

**Commit**: "feat(cli): implement download command logic"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Download command execution with file selection
- Progress display integration
- Graceful shutdown handling
- Download summary on completion
```

---

### 3.4 Daemon Mode
**Feature**: Continuous background syncing mode

**Tasks**:
- [ ] Create `putio-sync-cli/src/daemon/mod.rs`
- [ ] Implement periodic file listing with configurable interval
- [ ] Trigger downloads based on filters
- [ ] Handle graceful shutdown with signal handling
- [ ] Add proper logging for daemon operations
- [ ] Prevent duplicate downloads across runs
- [ ] Write daemon-specific tests

**Commit**: "feat(cli): implement daemon mode for continuous syncing"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Daemon mode for continuous background syncing
- Configurable check interval
- Filter-based automatic downloads
- Signal handling for graceful shutdown
```

---

### 3.5 Human Testing Phase: Download Command
**Feature**: Manual testing of download functionality

**Test Cases**:
- [ ] Test download with --all flag
- [ ] Test download with specific file IDs
- [ ] Test download with --filter glob patterns
- [ ] Test download with different worker counts
- [ ] Test chunked downloads with large files
- [ ] Test resume after interruption (Ctrl+C)
- [ ] Test --skip-existing functionality
- [ ] Test --delete-after functionality
- [ ] Test all progress bar styles
- [ ] Test daemon mode with filters
- [ ] Test download to custom directory
- [ ] Test sorting options (date, name, extension, size)
- [ ] Test handling of remote file deletion
- [ ] Test non-interactive terminal (redirect to file)
- [ ] Test edge cases (empty file list, network errors, disk full)

**Commit**: "test(cli): complete human testing of download command"

---

## 4. Delete Command

### 4.1 Delete API Endpoints
**Feature**: Implement Put.io delete API endpoints

**Endpoints**:
- `POST /files/delete` - Delete files by IDs

**Tasks**:
- [ ] Create `putio-sync-core/src/api/delete.rs`
- [ ] Implement `ApiClient::delete_files(file_ids: Vec<i64>)` method
- [ ] Implement `ApiClient::delete_folder_recursive(folder_id: i64)` method
- [ ] Add proper error handling for delete operations
- [ ] Write unit tests with mock server
- [ ] Write integration tests

**Commit**: "feat(core): implement file deletion API endpoints"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- File deletion API endpoints
- Support for single and bulk delete operations
- Recursive folder deletion
```

---

### 4.2 Delete Command Interface
**Feature**: CLI interface for delete command

**Command Signature**:
```
putio-sync delete [OPTIONS] [FILES]...

Arguments:
  [FILES]...                Specific file IDs to delete

Options:
  -a, --all                 Delete all files
  -f, --filter <PATTERN>    Filter files with glob pattern
  -r, --recursive           Delete folders recursively
  --before <DATE>           Delete files before date (e.g., "2026-01")
  --dry-run                 Show what would be deleted without deleting
  -y, --yes                 Skip confirmation prompt
  -h, --help                Print help
```

**Tasks**:
- [ ] Create `putio-sync-cli/src/commands/delete.rs`
- [ ] Define `DeleteArgs` struct with clap
- [ ] Implement all CLI flags and options
- [ ] Parse date formats for --before
- [ ] Add safety confirmation prompts
- [ ] Implement --dry-run mode
- [ ] Write unit tests

**Commit**: "feat(cli): add delete command interface"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Delete command for removing files from Put.io
- Support for filtering and date-based deletion
- Recursive folder deletion
- Dry-run and confirmation prompts for safety
```

---

### 4.3 Delete Command Logic
**Feature**: Implement delete command execution

**Tasks**:
- [ ] Implement file selection logic (--all, --filter, FILES, --before)
- [ ] Query Put.io API for file list
- [ ] Apply filters
- [ ] Display files to be deleted
- [ ] Show confirmation prompt (unless --yes or --dry-run)
- [ ] Execute deletions via API
- [ ] Record deletions in history
- [ ] Display summary (files deleted, total size freed)
- [ ] Handle errors gracefully
- [ ] Write integration tests

**Commit**: "feat(cli): implement delete command logic"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Delete command execution with file selection
- Confirmation prompts for safety
- History recording of deletions
- Delete summary display
```

---

### 4.4 Human Testing Phase: Delete Command
**Feature**: Manual testing of delete functionality

**Test Cases**:
- [ ] Test delete with specific file IDs
- [ ] Test delete with --filter glob patterns
- [ ] Test delete with --all flag (with confirmation)
- [ ] Test delete with --before date
- [ ] Test recursive folder deletion
- [ ] Test --dry-run mode
- [ ] Test --yes flag (skip confirmation)
- [ ] Test confirmation prompt behavior
- [ ] Test error handling (file not found, network errors)
- [ ] Verify history recording

**Commit**: "test(cli): complete human testing of delete command"

---

## 5. Stat Command

### 5.1 Statistics Aggregation
**Feature**: Aggregate download statistics from database

**Tasks**:
- [ ] Create `putio-sync-core/src/stats/mod.rs`
- [ ] Implement total downloads count query
- [ ] Implement total size downloaded query
- [ ] Implement total time spent downloading query
- [ ] Implement average transfer speed calculation
- [ ] Implement stats by extension (normalized to lowercase)
- [ ] Implement date range filtering
- [ ] Write unit tests for statistics queries
- [ ] Write integration tests

**Commit**: "feat(core): implement download statistics aggregation"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Download statistics aggregation from database
- Total downloads, size, time, and speed calculations
- Statistics by file extension
- Date range filtering for stats
```

---

### 5.2 Stat Command Interface
**Feature**: CLI interface for stat command

**Command Signature**:
```
putio-sync stat [OPTIONS]

Options:
  --by-extension            Show breakdown by file extension
  --since <DATE>            Show stats since date (e.g., "2025-01-01")
  --until <DATE>            Show stats until date
  -o, --output <FORMAT>     Output format [possible values: table, json] [default: table]
  -h, --help                Print help
```

**Tasks**:
- [ ] Create `putio-sync-cli/src/commands/stat.rs`
- [ ] Define `StatArgs` struct with clap
- [ ] Implement date range parsing
- [ ] Implement output format options
- [ ] Write unit tests

**Commit**: "feat(cli): add stat command interface"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Stat command for download statistics
- Extension-based breakdown
- Date range filtering
- Table and JSON output formats
```

---

### 5.3 Stat Command Output
**Feature**: Display statistics in user-friendly format

**Output Format (Table)**:
```
Download Statistics
═══════════════════════════════════════════════════════════

Total Downloads:        1,234 files
Total Size:            4.56 TB
Total Time:            12h 34m 56s
Average Speed:         102.3 MB/s

By Extension:
─────────────────────────────────────────────────────────
Extension     Files    Total Size    Percentage
─────────────────────────────────────────────────────────
.mkv           450      3.2 TB        70.2%
.mp4           300      890 GB        19.5%
.pdf           484      470 GB        10.3%
─────────────────────────────────────────────────────────
```

**Tasks**:
- [ ] Implement table formatter for statistics
- [ ] Implement JSON formatter for statistics
- [ ] Format sizes in human-readable format
- [ ] Format durations in human-readable format
- [ ] Format speeds in human-readable format
- [ ] Calculate percentages for extension breakdown
- [ ] Write unit tests for formatters

**Commit**: "feat(cli): implement stat command output formatting"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Statistics display with table format
- JSON output support
- Human-readable size, time, and speed formatting
- Extension breakdown with percentages
```

---

### 5.4 Human Testing Phase: Stat Command
**Feature**: Manual testing of stat functionality

**Test Cases**:
- [ ] Test basic stats display
- [ ] Test --by-extension breakdown
- [ ] Test --since date filtering
- [ ] Test --until date filtering
- [ ] Test combined date range
- [ ] Test JSON output format
- [ ] Test with no downloads in database
- [ ] Test with large dataset
- [ ] Verify calculations are accurate

**Commit**: "test(cli): complete human testing of stat command"

---

## 6. History Command

### 6.1 History Command Interface
**Feature**: CLI interface for history command

**Command Signature**:
```
putio-sync history [OPTIONS]

Options:
  --action <ACTION>         Filter by action [possible values: downloaded, deleted]
  --since <DATE>            Show history since date
  --until <DATE>            Show history until date
  -n, --limit <N>           Limit number of results [default: 50]
  --offset <N>              Offset for pagination [default: 0]
  -o, --output <FORMAT>     Output format [possible values: table, json, csv] [default: table]
  -h, --help                Print help
```

**Tasks**:
- [ ] Create `putio-sync-cli/src/commands/history.rs`
- [ ] Define `HistoryArgs` struct with clap
- [ ] Implement filtering options
- [ ] Implement pagination options
- [ ] Implement output format options
- [ ] Write unit tests

**Commit**: "feat(cli): add history command interface"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- History command for viewing download/delete history
- Action, date range filtering
- Pagination support
- Multiple output formats
```

---

### 6.2 History Command Output
**Feature**: Display history in user-friendly format

**Output Format (Table)**:
```
Download & Delete History
═══════════════════════════════════════════════════════════════════════════════

Timestamp             Action       Filename                Size        Speed
─────────────────────────────────────────────────────────────────────────────
2025-11-14 10:30:45   downloaded   movie.mkv              4.2 GB      98.5 MB/s
2025-11-14 09:15:22   deleted      old_file.mp4           1.5 GB      -
2025-11-14 08:45:10   downloaded   document.pdf           2.3 MB      45.2 MB/s
─────────────────────────────────────────────────────────────────────────────

Showing 3 of 1,234 entries. Use --limit and --offset for pagination.
```

**Tasks**:
- [ ] Query history from database with filters
- [ ] Implement table formatter for history
- [ ] Implement JSON formatter
- [ ] Implement CSV formatter
- [ ] Format timestamps
- [ ] Show pagination info
- [ ] Write unit tests for formatters

**Commit**: "feat(cli): implement history command output"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- History display with table format
- JSON and CSV output support
- Pagination information
- Formatted timestamps and sizes
```

---

### 6.3 Human Testing Phase: History Command
**Feature**: Manual testing of history functionality

**Test Cases**:
- [ ] Test basic history display
- [ ] Test --action filtering (downloaded/deleted)
- [ ] Test --since date filtering
- [ ] Test --until date filtering
- [ ] Test --limit pagination
- [ ] Test --offset pagination
- [ ] Test all output formats (table, json, csv)
- [ ] Test with empty history
- [ ] Test with large history dataset
- [ ] Verify pagination works correctly

**Commit**: "test(cli): complete human testing of history command"

---

## 7. Create/Add Command

### 7.1 Transfer API Endpoints
**Feature**: Implement Put.io transfer (torrent) API endpoints

**Endpoints**:
- `POST /transfers/add` - Add transfer from URL or file
- `GET /transfers/list` - List all transfers
- `GET /transfers/{id}` - Get transfer details

**Tasks**:
- [ ] Create `putio-sync-core/src/api/transfers.rs`
- [ ] Implement `ApiClient::add_transfer_url(url: &str)` for magnet links
- [ ] Implement `ApiClient::add_transfer_file(file_path: &Path)` for .torrent files
- [ ] Implement `ApiClient::list_transfers()` method
- [ ] Implement `ApiClient::get_transfer(id: i64)` method
- [ ] Add Transfer model to models.rs
- [ ] Write unit tests with mock server
- [ ] Write integration tests

**Commit**: "feat(core): implement transfer API endpoints"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Transfer API endpoints for adding torrents
- Support for magnet links and .torrent files
- Transfer listing and status queries
```

---

### 7.2 Create Command Interface
**Feature**: CLI interface for create/add command

**Command Signature**:
```
putio-sync create [OPTIONS] [SOURCES]...

Arguments:
  [SOURCES]...              Torrent files or magnet links to add

Options:
  -w, --watch <PATHS>...    Watch directories for .torrent files
  --parent-id <ID>          Parent folder ID for downloads [default: 0]
  -d, --daemon              Run in daemon mode (continuous watching)
  --interval <SECONDS>      Watch check interval [default: 60]
  -h, --help                Print help
```

**Tasks**:
- [ ] Create `putio-sync-cli/src/commands/create.rs`
- [ ] Define `CreateArgs` struct with clap
- [ ] Implement support for multiple sources
- [ ] Implement file vs URL detection
- [ ] Implement --watch and --daemon options
- [ ] Write unit tests

**Commit**: "feat(cli): add create command interface"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Create command for adding torrents to Put.io
- Support for .torrent files and magnet links
- Watch mode for monitoring directories
- Daemon mode for continuous monitoring
```

---

### 7.3 Create Command Logic
**Feature**: Implement create command execution

**Tasks**:
- [ ] Detect if source is file or magnet link
- [ ] Validate .torrent files
- [ ] Validate magnet link format
- [ ] Upload torrents via API
- [ ] Display transfer details after addition
- [ ] Handle errors gracefully (invalid files, API errors)
- [ ] Write integration tests

**Commit**: "feat(cli): implement create command logic"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Create command execution
- File and magnet link detection
- Transfer confirmation display
```

---

### 7.4 Watch Mode Implementation
**Feature**: Watch directories for new .torrent files

**Tasks**:
- [ ] Add `notify` dependency for filesystem watching
- [ ] Create `putio-sync-cli/src/watch/mod.rs`
- [ ] Implement directory watching for .torrent files
- [ ] Automatically upload new .torrent files
- [ ] Track processed files to avoid duplicates
- [ ] Support multiple watch directories
- [ ] Implement daemon mode for continuous watching
- [ ] Handle graceful shutdown
- [ ] Write integration tests

**Commit**: "feat(cli): implement watch mode for torrent files"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Watch mode for monitoring .torrent directories
- Automatic upload of new torrent files
- Multi-directory support
- Daemon mode for continuous monitoring
```

---

### 7.5 Human Testing Phase: Create Command
**Feature**: Manual testing of create functionality

**Test Cases**:
- [ ] Test adding magnet link
- [ ] Test adding single .torrent file
- [ ] Test adding multiple .torrent files
- [ ] Test adding multiple magnet links
- [ ] Test --watch with single directory
- [ ] Test --watch with multiple directories
- [ ] Test daemon mode (add file to watched directory)
- [ ] Test --parent-id for organizing transfers
- [ ] Test invalid torrent file handling
- [ ] Test invalid magnet link handling
- [ ] Test network error handling
- [ ] Verify transfers appear on Put.io

**Commit**: "test(cli): complete human testing of create command"

---

## 8. Integration & Polish

### 8.1 Update Config Example
**Feature**: Update example config with new options

**Tasks**:
- [ ] Create example config file at `.putiosync/config.example.toml`
- [ ] Document all download options
- [ ] Document database location
- [ ] Add comments explaining each option
- [ ] Include sensible defaults

**Commit**: "docs: add example config with download options"

---

### 8.2 Error Messages & Logging
**Feature**: Improve error messages and logging

**Tasks**:
- [ ] Review all error messages for clarity
- [ ] Add helpful suggestions to error messages
- [ ] Ensure all operations are properly logged
- [ ] Add debug logging for troubleshooting
- [ ] Write tests for error scenarios

**Commit**: "feat: improve error messages and logging"

---

### 8.3 Performance Optimization
**Feature**: Optimize performance for large file lists

**Tasks**:
- [ ] Add database indexes for common queries
- [ ] Optimize file listing for large directories
- [ ] Profile memory usage during downloads
- [ ] Optimize buffer sizes if needed
- [ ] Add benchmarks for critical paths

**Commit**: "perf: optimize database queries and file handling"

---

### 8.4 Update Documentation
**Feature**: Comprehensive user documentation

**Tasks**:
- [ ] Update README with new commands
- [ ] Add download command examples
- [ ] Add delete command examples
- [ ] Add stat command examples
- [ ] Add history command examples
- [ ] Add create command examples
- [ ] Document configuration options
- [ ] Add troubleshooting guide
- [ ] Add FAQ section

**Commit**: "docs: update user documentation for all commands"

---

### 8.5 Update CI/CD
**Feature**: Ensure CI includes new features

**Tasks**:
- [ ] Update CI to run all tests including new commands
- [ ] Add database migration tests
- [ ] Test multi-platform builds
- [ ] Update release workflow if needed

**Commit**: "ci: update workflows for new features"

---

## 9. Optional Enhancement Phases

### 9.1 Optional: Bandwidth Limiting
**Feature**: Limit download bandwidth

**Tasks**:
- [ ] Add bandwidth limit config option
- [ ] Implement rate limiting in download stream
- [ ] Add CLI flag for bandwidth limit
- [ ] Test bandwidth limiting

**Commit**: "feat: add bandwidth limiting for downloads"

---

### 9.2 Optional: Compression Support
**Feature**: Automatically extract compressed files after download

**Tasks**:
- [ ] Add option to auto-extract archives
- [ ] Support .zip, .rar, .7z formats
- [ ] Add option to delete archive after extraction
- [ ] Test extraction functionality

**Commit**: "feat: add automatic archive extraction"

---

### 9.3 Optional: Smart Retry Logic
**Feature**: Exponential backoff for failed downloads

**Tasks**:
- [ ] Implement retry with exponential backoff
- [ ] Add max retry count config
- [ ] Track retry attempts in database
- [ ] Log retry attempts

**Commit**: "feat: add smart retry logic for downloads"

---

### 9.4 Optional: Download Queue Management
**Feature**: Save and manage download queues

**Tasks**:
- [ ] Add queue persistence to database
- [ ] Allow pausing/resuming entire queue
- [ ] Allow reordering queue
- [ ] Add queue command to CLI

**Commit**: "feat: add download queue management"

---

### 9.5 Optional: Webhook/Notification Support
**Feature**: Send notifications on download completion

**Tasks**:
- [ ] Add webhook URL config option
- [ ] Send POST request on download completion
- [ ] Support multiple notification backends (webhook, email, etc.)
- [ ] Test notification delivery

**Commit**: "feat: add webhook notifications"

---

### 9.6 Optional: Mirror Mode
**Feature**: Keep local directory in sync with Put.io

**Tasks**:
- [ ] Implement bidirectional sync detection
- [ ] Delete local files when deleted from Put.io
- [ ] Add conflict resolution strategy
- [ ] Test mirror mode

**Commit**: "feat: add mirror mode for bidirectional sync"

---

### 9.7 Optional: Transfer Management Commands
**Feature**: Commands to manage Put.io transfers

**Tasks**:
- [ ] Add `putio-sync transfers` command to list transfers
- [ ] Add option to cancel transfers
- [ ] Add option to retry failed transfers
- [ ] Show transfer progress

**Commit**: "feat: add transfer management commands"

---

## 10. Implementation Order

### Phase 1: Database Foundation (Commits 1-2)
1. [ ] Implement SQLite database for download tracking
2. [ ] Add repository pattern for database access

### Phase 2: Download Core Infrastructure (Commits 3-9)
3. [ ] Extend config for download options
4. [ ] Implement cross-platform filename sanitization
5. [ ] Implement concurrent download worker pool
6. [ ] Implement streaming downloads with buffering
7. [ ] Implement chunked downloads for large files
8. [ ] Implement resumable downloads
9. [ ] Implement download progress tracking
10. [ ] Handle remote file deletion during download
11. [ ] Implement download manager orchestration

### Phase 3: Download Command - CLI (Commits 10-13)
12. [ ] Add download command interface
13. [ ] Implement progress bars for downloads
14. [ ] Implement download command logic
15. [ ] Implement daemon mode for continuous syncing
16. [ ] **HUMAN TESTING: Download Command**

### Phase 4: Delete Command (Commits 14-16)
17. [ ] Implement file deletion API endpoints
18. [ ] Add delete command interface
19. [ ] Implement delete command logic
20. [ ] **HUMAN TESTING: Delete Command**

### Phase 5: Stat Command (Commits 17-19)
21. [ ] Implement download statistics aggregation
22. [ ] Add stat command interface
23. [ ] Implement stat command output formatting
24. [ ] **HUMAN TESTING: Stat Command**

### Phase 6: History Command (Commits 20-21)
25. [ ] Add history command interface
26. [ ] Implement history command output
27. [ ] **HUMAN TESTING: History Command**

### Phase 7: Create/Add Command (Commits 22-26)
28. [ ] Implement transfer API endpoints
29. [ ] Add create command interface
30. [ ] Implement create command logic
31. [ ] Implement watch mode for torrent files
32. [ ] **HUMAN TESTING: Create Command**

### Phase 8: Integration & Polish (Commits 27-31)
33. [ ] Add example config with download options
34. [ ] Improve error messages and logging
35. [ ] Optimize database queries and file handling
36. [ ] Update user documentation for all commands
37. [ ] Update workflows for new features

### Phase 9: Optional Enhancements (As needed)
38. [ ] Optional features as time/requirements permit

---

## 11. Command Reference Summary

### Download Command
```bash
# Download all files
putio-sync download --all

# Download specific files
putio-sync download 12345 67890

# Download with filter
putio-sync download --all --filter "*.mkv"

# Download with custom workers and directory
putio-sync download --all --workers 4 --dir ~/Movies

# Download with chunking
putio-sync download --all --chunk-size 256MB

# Daemon mode - auto-download new video files
putio-sync download --daemon --filter "*.mkv" --interval 300

# Skip already downloaded files
putio-sync download --all --skip-existing

# Delete from Put.io after download
putio-sync download --all --delete-after

# Custom progress style
putio-sync download --all --progress overall

# Sort by date (oldest first)
putio-sync download --all --sort date --reverse
```

### Delete Command
```bash
# Delete specific files
putio-sync delete 12345 67890

# Delete with filter
putio-sync delete --filter "*.txt"

# Delete all files before date
putio-sync delete --all --before "2026-01"

# Delete folder recursively
putio-sync delete 12345 --recursive

# Dry run (show what would be deleted)
putio-sync delete --all --dry-run

# Delete without confirmation
putio-sync delete 12345 --yes
```

### Stat Command
```bash
# Show overall statistics
putio-sync stat

# Show statistics by extension
putio-sync stat --by-extension

# Show statistics for date range
putio-sync stat --since "2025-01-01" --until "2025-12-31"

# Output as JSON
putio-sync stat --output json
```

### History Command
```bash
# Show recent history
putio-sync history

# Show only downloads
putio-sync history --action downloaded

# Show only deletions
putio-sync history --action deleted

# Show history since date
putio-sync history --since "2025-11-01"

# Show with pagination
putio-sync history --limit 100 --offset 50

# Output as CSV
putio-sync history --output csv
```

### Create Command
```bash
# Add magnet link
putio-sync create "magnet:?xt=urn:btih:..."

# Add torrent file
putio-sync create ~/Downloads/file.torrent

# Add multiple torrents
putio-sync create file1.torrent file2.torrent

# Watch directory for torrents
putio-sync create --watch ~/Downloads/torrents

# Watch in daemon mode
putio-sync create --watch ~/Downloads/torrents --daemon

# Add to specific folder
putio-sync create file.torrent --parent-id 12345
```

---

## 12. Success Criteria

### Download Command
- [ ] Downloads files successfully with default settings
- [ ] Worker pool parallelizes downloads correctly
- [ ] Progress bars display accurately
- [ ] Chunked downloads work for large files
- [ ] Resume works after interruption
- [ ] Skip existing files works correctly
- [ ] Delete after download works correctly
- [ ] Daemon mode continuously syncs
- [ ] Filters and sorting work as expected
- [ ] All progress styles work correctly
- [ ] Graceful shutdown preserves partial downloads
- [ ] Database tracking is accurate

### Delete Command
- [ ] Deletes files successfully
- [ ] Filters work correctly
- [ ] Date-based deletion works
- [ ] Recursive deletion works
- [ ] Dry-run shows correct files
- [ ] Confirmation prompts work
- [ ] History records deletions
- [ ] Error handling is robust

### Stat Command
- [ ] Displays accurate statistics
- [ ] Extension breakdown is correct
- [ ] Date filtering works
- [ ] All output formats work
- [ ] Calculations are accurate

### History Command
- [ ] Displays history correctly
- [ ] Filtering works
- [ ] Pagination works
- [ ] All output formats work
- [ ] Timestamps are formatted correctly

### Create Command
- [ ] Adds magnet links successfully
- [ ] Uploads torrent files successfully
- [ ] Watch mode detects new files
- [ ] Daemon mode runs continuously
- [ ] Multi-directory watching works
- [ ] Error handling is robust

### Overall
- [ ] All tests pass on CI
- [ ] Code passes clippy
- [ ] Code is formatted with rustfmt
- [ ] Documentation is complete
- [ ] Database migrations work
- [ ] Performance is acceptable

---

## 13. Testing Strategy

### Unit Tests
- [ ] Test database operations (CRUD, queries, migrations)
- [ ] Test filename sanitization
- [ ] Test worker pool logic
- [ ] Test download streaming
- [ ] Test chunking logic
- [ ] Test resume logic
- [ ] Test progress calculations
- [ ] Test filtering and sorting
- [ ] Test statistics aggregation
- [ ] Test all formatters (table, json, csv)

### Integration Tests
- [ ] Test full download flow
- [ ] Test download with interruption and resume
- [ ] Test delete flow
- [ ] Test stat queries with real database
- [ ] Test history queries
- [ ] Test create/add flow
- [ ] Test watch mode
- [ ] Test daemon modes

### Manual Testing (Human)
- [ ] Test each command with various options
- [ ] Test error scenarios
- [ ] Test on different platforms (Linux, macOS, Windows)
- [ ] Test with large files
- [ ] Test with many files
- [ ] Test network interruptions
- [ ] Test disk space issues

---

## 14. Notes

- All commits should follow conventional commit format
- Each feature should include tests before commit
- Human testing phases are critical for validation
- Update changelogs with each commit
- Break down large features if they become too complex
- Optional enhancements can be implemented in any order
- Database migrations should be tested thoroughly
- Performance testing should be done with realistic datasets
- Security considerations for daemon modes (file permissions, input validation)

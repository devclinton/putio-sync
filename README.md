# putio-sync

A high-performance Rust-based CLI tool for Put.io with support for downloads, transfers, and file management.

## Features

- **Fast Downloads**: Concurrent downloads with configurable workers
- **Resumable Downloads**: Automatic resume for interrupted downloads using range requests
- **Streaming Downloads**: Efficient buffered streaming with minimal memory usage
- **Smart Progress Logging**: Size-based progress intervals with single-threshold logging (every 50% for 500MB+, 33% for 1GB+, 20% for 5GB+, 10% for 10GB+, 7% for 15GB+, 5% for 20GB+, 1% for 50GB+)
- **Empty Directory Cleanup**: Enhanced detection and tracking with 5-scan threshold for automatic deletion of empty folders
- **Smart Filtering**: Whitelist/blacklist patterns with glob support
- **Executable Guard**: Executables and installers on Put.io (`.exe`, `.msi`, `.bat`, ...) are never downloaded; configurable with `blocked_extensions`
- **Transfer Management**: Add torrents from files or magnet links
- **Watch Mode**: Automatically upload .torrent and .magnet files from watched directories with smart deduplication
- **Post-Processing**: Archive or delete processed torrent files after upload
- **Advanced Daemon**: Unified daemon with hot-reload configuration and intelligent worker scaling
- **Statistics & History**: Track download stats and view history
- **File Management**: List, download, and delete files on Put.io
- **SQLite Database**: Local state tracking for downloads and history
- **Progress Tracking**: Multiple progress bar styles with speed and ETA

## Architecture

This project is organized as a Cargo workspace with specialized crates following separation of concerns:

### Core Libraries

- **putio-sync-client**: Put.io API client
  - OAuth and app-specific password authentication
  - File listing, download, and transfer management
  - Asynchronous HTTP client with retry logic

- **putio-sync-db**: SQLite database layer
  - Download tracking and history
  - Transfer and queue management
  - Statistics repositories

- **putio-sync-download**: Download manager
  - Streaming downloads with buffered I/O
  - Resume support with range requests
  - Progress tracking and callbacks
  - Concurrent download orchestration

- **putio-sync-file-manager**: File system utilities
  - Path sanitization and validation
  - Empty directory cleanup
  - Cross-platform path handling

- **putio-sync-daemon**: Background daemon service
  - Periodic download synchronization
  - Torrent file watching and upload
  - Hot-reload configuration support
  - Worker pool management

- **putio-sync-core**: Shared utilities
  - Configuration management
  - File filtering and sorting
  - Path utilities
  - Logging and notifications

### Applications

- **putio-sync-cli**: Command-line interface
  - `login`: Authenticate with Put.io via OAuth
  - `list`: Browse files and folders with filtering
  - `download`: Download files with workers and resume support
  - `delete`: Remove files from Put.io
  - `create`: Add torrents from files or magnet links
  - `daemon`: Unified daemon for downloads and torrent watching
  - `stat`: View download statistics
  - `history`: View download and delete history

- **putio-sync-api**: Web API server
  - REST API for file and transfer management
  - JSON response format
  - Authentication middleware

## Installation

### macOS

**Homebrew** (recommended):
```bash
brew tap devclinton/putio-sync
brew install putio-sync
```

### Windows

**Winget** (recommended):
```powershell
winget install putio.putio-sync
```

Or download the latest release from [GitHub Releases](https://github.com/devclinton/putio-sync/releases).

### Linux

**Snap** (all distributions):
```bash
sudo snap install putio-sync
```

**Flatpak** (all distributions):
```bash
flatpak install flathub com.putio.PutioSync
```

**Debian/Ubuntu** (.deb):
```bash
# Download from releases
wget https://github.com/devclinton/putio-sync/releases/latest/download/putio-sync_VERSION_amd64.deb
sudo dpkg -i putio-sync_VERSION_amd64.deb
```

### From Source

**Requirements**: Rust 1.70+

```bash
cargo install --path putio-sync-cli
# Or for the latest release
cargo install putio-sync-cli
```

See [INSTALLATION.md](INSTALLATION.md) for detailed installation instructions.

## Quick Start

The binary will be available at `./target/release/putio-sync`.

### Authentication

First, authenticate with Put.io:

```bash
putio-sync login
```

This will open your browser for OAuth authentication and save your token to `~/.putiosync/config.toml`.

Alternatively, you can manually set your token in the config file:

```bash
cp config.example.toml ~/.putiosync/config.toml
# Edit ~/.putiosync/config.toml and add your OAuth token
```

### Basic Usage

```bash
# List files in root folder
putio-sync list

# List files in a specific folder
putio-sync list --parent-id 123456

# Download all files
putio-sync download --all

# Download specific files
putio-sync download 123456 789012

# Download with filtering
putio-sync download --all --filter "*.mkv"

# Download with 4 workers and skip existing
putio-sync download --all --workers 4 --skip-existing

# Add torrent from magnet link
putio-sync create "magnet:?xt=urn:btih:..."

# Add torrent from file
putio-sync create movie.torrent

# Watch directory for torrents
putio-sync create --watch ~/Downloads/torrents

# View statistics
putio-sync stat

# View download history
putio-sync history --limit 20

# Delete files
putio-sync delete 123456 --yes
```

## Commands

### `login`

Authenticate with Put.io using OAuth.

```bash
putio-sync login
```

Opens your browser for authentication and saves the token to your config file.

### `list`

List files and folders on Put.io.

```bash
putio-sync list [OPTIONS]

Options:
  -p, --parent-id <ID>      Parent folder ID (0 for root)
  -f, --filter <PATTERN>    Filter by filename pattern
  -o, --output <FORMAT>     Output format: table, json [default: table]
```

### `download`

Download files from Put.io with support for workers, filtering, and resuming.

```bash
putio-sync download [OPTIONS] [FILES]...

Arguments:
  [FILES]...                Specific file IDs to download

Options:
  -a, --all                 Download all files
  -f, --filter <PATTERN>    Filter files with glob pattern (e.g., "*.mkv")
  -d, --dir <PATH>          Download directory [default: from config]
  -w, --workers <N>         Number of concurrent workers [default: from config]
  --buffer-size <SIZE>      Buffer size for streaming (e.g., 16MB) [default: from config]
  --no-resume               Disable resume of partial downloads
  --skip-existing           Skip files already downloaded
  --delete-after            Delete from Put.io after download
  --sort <ORDER>            Sort order: by-date, by-name, by-extension, by-size
  --reverse                 Reverse sort order
  --progress <STYLE>        Progress style: per-file, overall, individual, rotating, none
  --daemon                  Run in daemon mode (periodic sync)
  --interval <SECONDS>      Daemon check interval [default: 300]
```

Examples:

```bash
# Download all files with 4 workers
putio-sync download --all --workers 4

# Download only video files
putio-sync download --all --filter "*.mkv,*.mp4"

# Download and delete from Put.io
putio-sync download --all --delete-after

# Download with custom buffer size for streaming
putio-sync download --all --buffer-size 32MB

# Run in daemon mode, checking every 5 minutes
putio-sync download --all --daemon --interval 300
```

### `delete`

Delete files from Put.io.

```bash
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
```

Examples:

```bash
# Delete specific files
putio-sync delete 123456 789012

# Delete files matching pattern (with confirmation)
putio-sync delete --filter "*.txt"

# Delete all files before a date
putio-sync delete --all --before "2024-01-01" --yes

# Dry run to see what would be deleted
putio-sync delete --all --filter "*.log" --dry-run
```

### `create`

Add torrents to Put.io from files or magnet links.

```bash
putio-sync create [OPTIONS] [SOURCES]...

Arguments:
  [SOURCES]...              Torrent files or magnet links to add

Options:
  -w, --watch <PATHS>...    Watch directories for .torrent and .magnet files
  --parent-id <ID>          Parent folder ID for downloads [default: 0]
  -d, --daemon              Run in daemon mode (continuous watching)
  --interval <SECONDS>      Watch check interval [default: 60]
```

Examples:

```bash
# Add torrent from magnet link
putio-sync create "magnet:?xt=urn:btih:..."

# Add torrent from file
putio-sync create movie.torrent

# Add magnet from .magnet file
putio-sync create movie.magnet

# Add multiple torrents and magnet files
putio-sync create file1.torrent file2.magnet file3.torrent

# Watch directory for torrents and magnet files
putio-sync create --watch ~/Downloads/torrents

# Watch multiple directories in daemon mode
putio-sync create --watch ~/Downloads --watch /media/torrents --daemon
```

### `stat`

View download statistics.

```bash
putio-sync stat [OPTIONS]

Options:
  --by-extension            Show breakdown by file extension
  --since <DATE>            Show stats since date (e.g., "2025-01-01")
  --until <DATE>            Show stats until date
  -o, --output <FORMAT>     Output format: table, json [default: table]
```

Examples:

```bash
# View overall statistics
putio-sync stat

# View statistics by file extension
putio-sync stat --by-extension

# View statistics for a date range
putio-sync stat --since "2025-01-01" --until "2025-12-31"
```

### `history`

View download and delete history.

```bash
putio-sync history [OPTIONS]

Options:
  --action <ACTION>         Filter by action: downloaded, deleted
  --since <DATE>            Show history since date
  --until <DATE>            Show history until date
  -n, --limit <N>           Limit number of results [default: 50]
  --offset <N>              Offset for pagination [default: 0]
  -o, --output <FORMAT>     Output format: table, json, csv [default: table]
```

Examples:

```bash
# View recent history
putio-sync history

# View only downloads
putio-sync history --action downloaded

# View history for a specific date range
putio-sync history --since "2025-01-01" --limit 100

# Export history to CSV
putio-sync history --output csv > history.csv
```

## Configuration

Configuration is stored at `~/.putiosync/config.toml`. See `config.example.toml` for a complete example with all options documented.

### Path Handling

putio-sync supports flexible path formats for all directory configurations:

- **Absolute paths**: `/home/user/Downloads` (exact path)
- **Relative paths**: `./downloads`, `downloads` (relative to current directory)  
- **Home directory**: `~/Downloads` (expands to user home)
- **Current directory**: `.` (current working directory)

Examples:
```toml
[download]
target_directory = "~/Downloads/putio"      # Expands to /home/user/Downloads/putio
# target_directory = "/mnt/downloads"       # Absolute path
# target_directory = "./downloads"          # Relative to current directory

[watch] 
paths = [
    "~/Downloads/torrents",                 # Home relative
    "/var/watch/torrents",                  # Absolute path
    "./torrent-watch"                       # Current directory relative
]
```

### Download Configuration

Configure download behavior including streaming buffer size and resume support:

- **Streaming**: Files are downloaded using efficient buffered streaming
- **Buffer Size**: Configurable buffer for reading/writing (default: 16MB)
- **Resume Support**: Automatic resume using HTTP range requests for interrupted downloads
- **Recommended**: 16-32MB buffer size for optimal performance

```toml
[download]
buffer_size_mb = 16        # Stream buffer size for downloads
resume_enabled = true      # Enable automatic resume
skip_existing = false      # Skip files that already exist locally
```

### Basic Configuration

```toml
[auth]
auth_method = "Token"
token = "YOUR_OAUTH_TOKEN"

[download]
workers = 2
target_directory = "~/Downloads/putio"  # Supports ~, absolute, and relative paths
buffer_size_mb = 16                     # Stream buffer size for downloads
resume_enabled = true                   # Enable automatic resume
skip_existing = false                   # Skip files that already exist
delete_after_download = true            # Clean up Put.io after successful downloads
progress_style = "per-file"             # Progress bar style
sort_order = "by-date"                  # File sorting order
max_retries = 3                         # Retry failed downloads
bandwidth_limit_bytes_per_sec = 1048576 # Optional: 1 MB/s limit

[watch]
refresh_cycle = 60
paths = []
```

### Environment Variables

- `PUTIO_SYNC_CONFIG`: Custom config file path
- `RUST_LOG`: Log level (error, warn, info, debug, trace). When set, it replaces
  `[logging] level` from the config file. The startup line `Log level level=... source=Env`
  (or `source=Config`) shows which one is in effect.

## Metrics

Both `daemon` and `server` modes serve Prometheus metrics at
`http://0.0.0.0:9742/metrics` by default, in the text exposition format. The listener
starts even when no Put.io token is set. If the port can't be bound, one error is logged
and putio-sync keeps running without metrics.

```toml
[metrics]
enabled = true          # PUTIO_SYNC_METRICS_ENABLED
bind = "0.0.0.0:9742"   # PUTIO_SYNC_METRICS_BIND
path = "/metrics"       # PUTIO_SYNC_METRICS_PATH
detail = "basic"        # PUTIO_SYNC_METRICS_DETAIL: basic | detailed
```

> **No auth.** Anything that can reach the port can read the metrics. They never contain
> tokens, file names, ids, paths or URLs, but keep the port on a trusted network.

`detailed` adds per-operation API latency and per-worker series. Counters reset when the
process restarts, so query them with `increase()` or `rate()`. In Docker, publish the
port (`-p 9742:9742`).

### Metric families

H = histogram.

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `putio_sync_api_requests_total` | counter | `operation`, `outcome` | Each HTTP attempt to Put.io, retries included |
| `putio_sync_api_retries_total` | counter | `reason` | Each retry scheduled by the retry policy |
| `putio_sync_api_request_duration_seconds` | H | — | Latency of each attempt |
| `putio_sync_api_operation_duration_seconds` | H | `operation` | **detailed only.** Latency per operation |
| `putio_sync_sync_runs_total` | counter | `result` | Completed sync runs |
| `putio_sync_sync_duration_seconds` | H | — | Duration of each sync run |
| `putio_sync_last_sync_duration_seconds` | gauge | — | Duration of the latest run |
| `putio_sync_last_success_timestamp_seconds` | gauge | — | Unix time the last successful run finished; absent until the first one |
| `putio_sync_last_sync_files` | gauge | `kind` | Counts from the latest run |
| `putio_sync_files_blocked_total` | counter | — | Files skipped by the blocked-extension list (warned once per file id) |
| `putio_sync_queue_items` | gauge | `state` | Queue rows by state, refreshed every 15 s and at startup |
| `putio_sync_downloads_active` | gauge | — | Downloads in progress |
| `putio_sync_download_workers` | gauge | — | Configured worker count |
| `putio_sync_downloads_completed_total` | counter | — | Downloads verified and renamed into place |
| `putio_sync_downloads_failed_total` | counter | `reason` | Downloads that ended in failure |
| `putio_sync_downloaded_bytes_total` | counter | — | Bytes written to disk; `rate()` gives throughput |
| `putio_sync_download_duration_seconds` | H | — | Time per completed download |
| `putio_sync_worker_state` | gauge | `worker` | **detailed only.** 0 idle, 1 downloading |
| `putio_sync_worker_inflight_bytes` | gauge | `worker` | **detailed only.** Bytes written so far for the current file |
| `putio_sync_watch_scans_total` | counter | — | Watch-folder scans |
| `putio_sync_watch_uploads_total` | counter | — | Torrent/magnet files uploaded to Put.io |
| `putio_sync_watch_cleanups_total` | counter | — | Already-uploaded files cleaned up |
| `putio_sync_watch_invalid_total` | counter | — | Invalid files (warned once) |
| `putio_sync_watch_cleanup_failures_total` | counter | — | Cleanup (delete/archive) failures |
| `putio_sync_db_errors_total` | counter | `operation` | Database errors |
| `putio_sync_disk_free_bytes` | gauge | — | Free space for the download folder; absent if unreadable |
| `putio_sync_disk_total_bytes` | gauge | — | Size of the download folder's filesystem; absent if unreadable |
| `putio_sync_start_time_seconds` | gauge | — | Process start, Unix seconds |
| `putio_sync_build_info` | gauge (=1) | `version`, `commit` | Build identity |

Label values are fixed sets, listed in
[specs/003-telemetry-prometheus/data-model.md](specs/003-telemetry-prometheus/data-model.md#closed-label-sets).

### Useful queries

| Question | Query |
|---|---|
| Syncs failed last week | `increase(putio_sync_sync_runs_total{result="failure"}[7d])` |
| Retries and why | `sum by (reason) (increase(putio_sync_api_retries_total[1d]))` |
| Pending and moving? | `putio_sync_queue_items{state="pending"}` and `increase(putio_sync_downloads_completed_total[1h])` |
| Executables blocked | `increase(putio_sync_files_blocked_total[7d])` |
| Watch cleanups | `increase(putio_sync_watch_cleanups_total[7d])` |
| DB errors | `increase(putio_sync_db_errors_total[1d])` |

## Advanced Features

### Streaming Downloads

All downloads use efficient buffered streaming with automatic resume support:

```bash
# Download with custom buffer size
putio-sync download --all --buffer-size 32MB --workers 4

# Resume is enabled by default - interrupted downloads automatically continue
putio-sync download --all
```

### Filtering

Use glob patterns to filter files:

```bash
# Whitelist: only download video files
putio-sync download --all --filter "*.mkv,*.mp4,*.avi"

# Config file approach
[download]
whitelist_patterns = "*.mkv,*.mp4"
blacklist_patterns = "*.exe,*.bat"
```

#### Executable guard

Fake releases are often executables. Files whose final extension is in
`[download] blocked_extensions` are never queued or downloaded. They stay on Put.io, and
each one is logged once as `WARN Blocked file not downloaded file_id=... extension=exe`.
The sync summary counts them as `Blocked: N`.

```toml
[download]
# Omit to use the default list; set [] to disable the guard.
blocked_extensions = ["exe", "msi", "bat", "cmd", "com", "scr", "pif", "vbs",
                      "js", "jar", "ps1", "lnk", "apk", "dmg", "pkg"]
```

Matching is case-insensitive and uses only the last extension (`a.mkv.exe` is blocked,
`a.exe.mkv` is not). The setting can also be changed through `PUT /settings/download`.

### Progress Styles

Choose from multiple progress bar styles:

- `per-file`: Show progress for each file + overall (default)
- `overall`: Only show overall progress
- `individual`: Only show individual file progress
- `rotating`: Rotate between files and overall
- `none`: No progress bars (useful for logging)

```bash
putio-sync download --all --progress overall
```

### Daemon Mode

Run downloads continuously in the background:

```bash
# Check for new files every 5 minutes
putio-sync download --all --daemon --interval 300 --filter "*.mkv"
```

## Running Periodic Sync and Watch Simultaneously

You can run both periodic downloads and torrent watching at the same time using multiple terminal sessions, systemd services, or Docker containers. Here are several approaches:

### Method 1: New Unified Daemon Command with Hot Reloading (Recommended)

Use the new `daemon` command that combines both download sync and torrent watching in a single process with **automatic configuration hot reloading**:

```bash
# Run both download sync and torrent watching together
putio-sync daemon --download-interval 300 --watch-interval 60 --watch ~/Downloads/torrents --skip-existing --delete-after

# Download-only mode (no torrent watching)  
putio-sync daemon --download-only --download-interval 300 --skip-existing --delete-after

# Watch-only mode (no download sync)
putio-sync daemon --watch-only --watch-interval 60 --watch ~/Downloads/torrents ~/Watch/putio

# With filtering for specific file types
putio-sync daemon --filter "*.mkv,*.mp4,*.avi" --skip-existing --delete-after

# Archive torrent/magnet files after processing (move to ~/processed)
putio-sync daemon --watch-only --watch ~/Downloads/torrents --archive-dir ~/processed

# Delete torrent/magnet files after processing 
putio-sync daemon --watch-only --watch ~/Downloads/torrents --delete-torrents
```

> **Daemon flag support:** `--download-interval` sets how often Put.io is synced (default
> 300 s), and `--watch-interval` sets how often watch folders are scanned and the queue is
> worked. `--filter`, `--skip-existing`, `--delete-after`, `--delete-empty-folders`,
> `--watch-only`, `--download-only`, `--archive-dir` and `--delete-torrents` are currently
> ignored in daemon mode, with a warning at startup; use `config.toml` instead
> (`watch.delete_after_upload`, `download.delete_after_download`). The effective settings
> are logged after the config loads, under `Daemon configuration:`.
>
> Watch files that were already uploaded (for example after a restart) get the configured
> post-upload action instead of staying in the watch folder. Scans with nothing to do log
> at `debug`, so an idle daemon is quiet at `info`.

**Improved Daemon Logging**: The daemon now provides clear, structured logs:
```bash
# Download activity
🔽 Starting download: Movie.mkv (1.2 GB)
✅ Download completed: Movie.mkv (1.2 GB downloaded)

# Torrent activity  
🔼 Processing new .torrent file: Movie.torrent
✅ Torrent uploaded successfully: Movie (ID: 1234567)

# Magnet file activity
🔼 Processing new .magnet file: Movie.magnet
✅ Magnet link uploaded successfully: Movie (ID: 1234568)

# Delete activity (works for both .torrent and .magnet files)
INFO Deleted uploaded watch file path=/watch/Movie.torrent
INFO Removed already-uploaded watch file path=/watch/Series.magnet

# Periodic health summaries
📊 Download sync completed in 2.1s - found: 3, downloaded: 2, failed: 0, total: 2.1 GB
📊 Daemon Health - Uptime: 2h15m, Downloads: 1/2 active, 3 queued, Watch: 2 dirs, 47 processed

# File cleanup after download
🗑️  Successfully deleted Movie.mkv from Put.io (delete_after_download enabled)
```

#### 🔥 Hot Reloading Features

The daemon command supports intelligent **hot reloading** of configuration changes:

- **Config File Watching**: Automatically detects changes to `~/.putiosync/config.toml`
- **Intelligent Worker Scaling**: 
  - **Scaling Up**: Adds new workers seamlessly
  - **Scaling Down**: Preserves downloads ≥75% complete, interrupts downloads <75%
  - **Download Preservation**: Interrupted downloads are moved to the top of the queue with higher priority
- **Live Settings Updates**: Changes to download settings apply to new downloads immediately
- **Zero Downtime**: Configuration changes happen without stopping the daemon

#### Configuration Hot Reload Examples

```bash
# Start daemon with 2 workers
putio-sync daemon --download-only

# Edit config.toml to change workers to 4 - automatically applied!
# Downloads continue seamlessly

# Edit config.toml to change workers to 1 - downloads <75% are intelligently interrupted
# and prioritized for restart after active downloads complete

# Change bandwidth limits, chunk sizes, etc. - applied to new downloads immediately
```

#### Worker Scaling Intelligence

When scaling down workers:
1. ✅ **Preserve** downloads ≥75% complete (let them finish)
2. ⚠️ **Interrupt** downloads <75% complete 
3. 🔄 **Prioritize** interrupted downloads (moved to front of queue with +1000 priority)
4. ⏭️ **Resume** interrupted downloads after current ones complete

#### Smart Torrent Processing & Deduplication

The daemon includes advanced torrent processing features:

- **Multi-format Support**: Processes both `.torrent` files (binary) and `.magnet` files (text-based magnet links)
- **Checksum-based Deduplication**: Prevents reprocessing identical files using SHA-256 checksums
  - For `.torrent` files: checksums the binary content
  - For `.magnet` files: checksums the magnet link content
- **SQLite Database Tracking**: Maintains persistent record of processed files (when archive/delete not enabled)
- **Post-processing Options**:
  - **Archive**: Move files to a specified directory after successful upload
  - **Delete**: Remove files after successful upload
  - **Keep**: Leave files in place with database tracking (default)

```bash
# Enable database tracking (default when no archive/delete specified)
putio-sync daemon --watch-only --watch ~/Downloads/torrents

# Archive files after processing
putio-sync daemon --watch-only --watch ~/Downloads/torrents --archive-dir ~/processed

# Delete files after processing
putio-sync daemon --watch-only --watch ~/Downloads/torrents --delete-torrents
```

### Method 2: Multiple Terminal Sessions (Alternative)

Run each daemon in separate terminal sessions:

```bash
# Terminal 1: Periodic download sync (checks every 5 minutes)
putio-sync download --all --daemon --interval 300 --skip-existing --progress none

# Terminal 2: Torrent watcher (checks every minute)
putio-sync create --watch ~/Downloads/torrents --daemon --interval 60
```

### Method 3: Configuration File Setup

Configure everything in `~/.putiosync/config.toml` for consistent behavior:

```toml
[auth]
auth_method = "Token"
token = "YOUR_OAUTH_TOKEN"

[download]
workers = 4
target_directory = "~/Downloads/putio"
skip_existing = true
delete_after_download = true
progress_style = "none"  # Better for daemon mode
whitelist_patterns = "*.mkv,*.mp4,*.avi,*.mp3,*.flac"
blacklist_patterns = "*.exe,*.bat,*.msi"

[watch]
refresh_cycle = 60
paths = ["~/Downloads/torrents", "~/Watch/putio", "/media/torrents"]
# archive_directory = "~/processed"        # Move processed files here (optional)
# delete_after_upload = true               # Delete files after upload (optional)

[notifications]
enabled = true
backend = "webhook"
webhook_url = "https://your-domain.com/webhook/putio"
```

Then run the daemons with simplified commands:

```bash
# Download daemon (uses config.toml settings)
putio-sync download --all --daemon --interval 300

# Watch daemon (uses config.toml watch.paths)
putio-sync create --daemon --interval 60 --watch ~/Downloads/torrents ~/Watch/putio
```
```
docker run --env PUTIO_SYNC_CONFIG=/config/config.toml --volume "/mnt/archive/sync_folders/local/putio-sync:/config" --volume "/mnt/archive/sync_folders/torrents/downloaded:/downloads" --volume /mnt/archive/sync_folders/torrents/blackhole:/watches/torrents devclinton/putio-sync:latest
```
### Method 4: Systemd Services (Linux)

Create systemd service files for automatic startup:

**Download Service** (`~/.config/systemd/user/putio-sync-download.service`):
```ini
[Unit]
Description=Put.io Download Sync Daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/putio-sync download --all --daemon --interval 300 --skip-existing --progress none
Restart=always
RestartSec=30
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
```

**Watch Service** (`~/.config/systemd/user/putio-sync-watch.service`):
```ini
[Unit]
Description=Put.io Torrent Watch Daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/putio-sync create --daemon --interval 60 --watch %h/Downloads/torrents %h/Watch/putio
Restart=always
RestartSec=30
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
```

Enable and start the services:
```bash
systemctl --user daemon-reload
systemctl --user enable putio-sync-download.service
systemctl --user enable putio-sync-watch.service
systemctl --user start putio-sync-download.service
systemctl --user start putio-sync-watch.service

# Check status
systemctl --user status putio-sync-download.service
systemctl --user status putio-sync-watch.service
```

### Method 5: Docker Compose

Create a `docker-compose.yml` for both services:

```yaml
version: '3.8'

services:
  # New unified daemon approach (recommended)
  putio-sync-daemon:
    image: ghcr.io/devclinton/putio-sync:latest
    container_name: putio-sync-daemon
    restart: unless-stopped
    volumes:
      - ~/.putiosync:/config
      - ~/Downloads/putio:/downloads
      - ~/Downloads/torrents:/torrents
      - ~/Watch/putio:/watch
    environment:
      - RUST_LOG=info
    command: >
      putio-sync daemon 
      --download-interval 300 
      --watch-interval 60 
      --watch /torrents /watch
      --skip-existing 
      --delete-after
      --filter "*.mkv,*.mp4,*.avi,*.mp3,*.flac"
    healthcheck:
      test: ["CMD", "putio-sync", "--version"]
      interval: 30s
      timeout: 10s
      retries: 3

  # Alternative: Separate services (if you prefer the old approach)
  putio-sync-download:
    image: ghcr.io/devclinton/putio-sync:latest
    container_name: putio-sync-download
    restart: unless-stopped
    profiles: ["separate"]  # Only runs when 'separate' profile is used
    volumes:
      - ~/.putiosync:/config
      - ~/Downloads/putio:/downloads
    environment:
      - RUST_LOG=info
    command: >
      putio-sync download --all --daemon --interval 300 
      --skip-existing --progress none --delete-after

  putio-sync-watch:
    image: ghcr.io/devclinton/putio-sync:latest
    container_name: putio-sync-watch
    restart: unless-stopped
    profiles: ["separate"]  # Only runs when 'separate' profile is used
    volumes:
      - ~/.putiosync:/config
      - ~/Downloads/torrents:/torrents
      - ~/Watch/putio:/watch
    environment:
      - RUST_LOG=info
    command: >
      putio-sync create --daemon --interval 60 
      --watch /torrents /watch
```

Start with Docker Compose:
```bash
# Start unified daemon (recommended)
docker-compose up -d putio-sync-daemon

# Or start separate services (old approach)  
docker-compose --profile separate up -d

# View logs
docker-compose logs -f putio-sync-daemon
# OR for separate services:
# docker-compose logs -f putio-sync-download putio-sync-watch

# Check status
docker-compose ps
```

### Method 6: Process Management with PM2 (Node.js)

Install PM2 and create ecosystem file `ecosystem.config.js`:

```javascript
module.exports = {
  apps: [
    {
      name: 'putio-sync-download',
      script: 'putio-sync',
      args: 'download --all --daemon --interval 300 --skip-existing --progress none',
      instances: 1,
      autorestart: true,
      watch: false,
      max_memory_restart: '1G',
      env: {
        RUST_LOG: 'info'
      }
    },
    {
      name: 'putio-sync-watch',
      script: 'putio-sync',
      args: 'create --daemon --interval 60 --watch ~/Downloads/torrents ~/Watch/putio',
      instances: 1,
      autorestart: true,
      watch: false,
      max_memory_restart: '512M',
      env: {
        RUST_LOG: 'info'
      }
    }
  ]
};
```

```bash
# Install PM2
npm install -g pm2

# Start services
pm2 start ecosystem.config.js

# Monitor
pm2 monit
pm2 logs putio-sync-download
pm2 logs putio-sync-watch

# Auto-startup on boot
pm2 startup
pm2 save
```

### Configuration Tips

1. **Avoid Conflicts**: Use different `--interval` values to prevent both processes from hitting the API simultaneously
2. **Resource Management**: Set appropriate `workers` limits based on your bandwidth
3. **Logging**: Use `RUST_LOG=info` for production, `debug` for troubleshooting
4. **Storage**: Ensure sufficient disk space for downloads
5. **Monitoring**: Use `--progress none` for daemon mode to reduce log noise

### Watch Directory Structure

Organize your watch directories by content type:

```
~/Downloads/
├── torrents/          # Main torrent drop folder
│   ├── movies/        # Movie torrents
│   ├── tv/           # TV show torrents
│   └── music/        # Music torrents
├── putio/            # Downloaded files destination
│   ├── movies/
│   ├── tv/
│   └── music/
└── temp/             # Temporary processing
```

### Database Location

Download tracking and history are stored in `~/.putiosync/putio-sync.db`

## Development

### Prerequisites

- Rust 1.70 or later
- SQLite 3

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running the CLI

```bash
cargo run --bin putio-sync -- --help
```

## Docker

### Pull from GitHub Container Registry

```bash
docker pull ghcr.io/devclinton/putio-sync:latest
```

### Build from Source

#### Basic Build
```bash
docker build -t putio-sync .
```

#### Advanced Build with Caching

For faster builds, especially during development, use the enhanced build targets with BuildKit caching:

```bash
# Optimized inline cache (recommended - works everywhere)
make docker-build-fast

# Basic BuildKit with inline cache  
make docker-build

# Local filesystem cache (fastest but requires buildx setup)
make docker-build-local-cache

# Registry-based cache (best for CI/CD and team collaboration)
make docker-build-cache
```

#### Build Performance Features

The enhanced Docker build includes several optimizations:

- **🚀 Dependency Caching**: Dependencies are built in a separate layer that only rebuilds when `Cargo.toml`/`Cargo.lock` change
- **🔄 BuildKit Mount Cache**: Uses `--mount=type=cache` for Cargo registry and build artifacts
- **📦 Multi-Stage Optimization**: Separates dependency compilation from source compilation
- **⚡ Cache Strategies**: Multiple caching options for different use cases

**Cache Performance Comparison:**
- **No cache**: 5-10 minutes (full rebuild)
- **Basic cache**: 2-4 minutes (partial dependency reuse)  
- **Local cache**: 30-90 seconds (full dependency reuse)
- **Registry cache**: 1-3 minutes (shared across machines)

#### Cache Management

```bash
# View cache information
make docker-cache-info

# Clean local cache
make docker-clean-cache

# Remove old images
make docker-clean
```

### Running with Docker

Create a config directory and add your configuration:

```bash
mkdir -p ~/.putiosync
cp config.example.toml ~/.putiosync/config.toml
# Edit ~/.putiosync/config.toml and add your OAuth token
```

Run the container:

```bash
docker run -d \
  --name putio-sync \
  -v ~/.putiosync:/config \
  -v ~/Downloads/putio:/downloads \
  -v ~/Downloads/torrents:/watches \
  ghcr.io/devclinton/putio-sync:latest \
  putio-sync download --all --daemon --interval 300
```

### Volume Mounts

The Docker image uses the following directories:

- `/config` - Configuration directory (mount your config.toml here)
- `/downloads` - Default download directory
- `/watches` - Directory for watching .torrent and .magnet files (optional)

### Environment Variables

- `RUST_LOG` - Set log level (error, warn, info, debug, trace)

### Example: Download Daemon

Run as a daemon that checks for new files every 5 minutes:

```bash
docker run -d \
  --name putio-sync-daemon \
  --restart unless-stopped \
  -v ~/.putiosync:/config \
  -v ~/Downloads/putio:/downloads \
  -e RUST_LOG=info \
  ghcr.io/devclinton/putio-sync:latest \
  putio-sync download --all --daemon --interval 300
```

### Example: Torrent Watcher

Watch a directory for .torrent and .magnet files and automatically upload them:

```bash
docker run -d \
  --name putio-sync-watcher \
  --restart unless-stopped \
  -v ~/.putiosync:/config \
  -v ~/Downloads/torrents:/watches \
  -e RUST_LOG=info \
  ghcr.io/devclinton/putio-sync:latest \
  putio-sync create --watch /watches --daemon
```

### Docker Compose

Example `docker-compose.yml`:

```yaml
version: '3.8'

services:
  putio-sync-daemon:
    image: ghcr.io/devclinton/putio-sync:latest
    container_name: putio-sync-daemon
    restart: unless-stopped
    volumes:
      - ~/.putiosync:/config
      - ~/Downloads/putio:/downloads
    environment:
      - RUST_LOG=info
    command: putio-sync download --all --daemon --interval 300

  putio-sync-watcher:
    image: ghcr.io/devclinton/putio-sync:latest
    container_name: putio-sync-watcher
    restart: unless-stopped
    volumes:
      - ~/.putiosync:/config
      - ~/Downloads/torrents:/watches
    environment:
      - RUST_LOG=info
    command: putio-sync create --watch /watches --daemon
```

Run with:

```bash
docker-compose up -d
```

## File Formats & Processing

### Torrent Files (.torrent)

Traditional BitTorrent files containing metadata and tracker information. These are uploaded directly to Put.io using the file upload API.

```bash
# Process .torrent file
putio-sync create movie.torrent
```

### Magnet Files (.magnet) 

Text files containing magnet links for BitTorrent downloads. The content should be a single magnet URI starting with `magnet:`.

Example `.magnet` file content:
```
magnet:?xt=urn:btih:d9e4b7b8c8c8f8f8f8f8f8f8f8f8f8f8f8f8f8f8&dn=Movie-2024&tr=udp://tracker.example.com:80
```

```bash
# Process .magnet file
putio-sync create movie.magnet
```

### Database & Deduplication

When neither archive nor delete options are specified, putio-sync maintains a SQLite database (`.putiosync-torrents.db`) to track processed files and prevent reprocessing:

- **Checksum-based**: Uses SHA-256 hashes to identify duplicate content
  - `.torrent` files: checksums binary file content
  - `.magnet` files: checksums magnet link string content  
- **Persistent**: Survives daemon restarts and system reboots
- **Efficient**: Only processes truly new or changed files
- **Cleanup**: Supports automatic cleanup of old records

Database location: `<target_directory>/.putiosync-torrents.db`

## Distribution

- Compiled binaries for macOS, Linux, and Windows
- Docker images via GitHub Container Registry

## Development Principles

- **Test-Driven Development**: Write tests first
- **Documentation**: Document as we go
- **Structured Logging**: Using tracing from day one
- **Robust Error Handling**: Comprehensive error types
- **Commit Frequency**: Commit after completing each feature

## License

MIT

# putio-sync

A high-performance Rust-based CLI tool for Put.io with support for downloads, transfers, and file management.

## Features

- **Fast Downloads**: Concurrent downloads with configurable workers
- **Resumable Downloads**: Automatic resume for interrupted downloads using range requests
- **Streaming Downloads**: Efficient buffered streaming with minimal memory usage
- **Smart Progress Logging**: Size-based progress intervals with single-threshold logging (every 50% for 500MB+, 33% for 1GB+, 20% for 5GB+, 10% for 10GB+, 7% for 15GB+, 5% for 20GB+, 1% for 50GB+)
- **Empty Directory Cleanup**: Enhanced detection and tracking with 5-scan threshold for automatic deletion of empty folders
- **Put.io Empty-Folder Cleanup**: Remove empty folders from your Put.io account once a day or on demand, from the CLI, API or web UI ([details](#putio-empty-folder-cleanup))
- **Smart Filtering**: Whitelist/blacklist patterns with glob support
- **Executable Guard**: Executables and installers on Put.io (`.exe`, `.msi`, `.bat`, ...) are never downloaded; configurable with `blocked_extensions`
- **Transfer Management**: Add torrents from files or magnet links
- **Watch Mode**: Automatically upload .torrent and .magnet files from watched directories with smart deduplication
- **Post-Processing**: Archive or delete processed torrent files after upload
- **Advanced Daemon**: Unified daemon with live log-level changes and graceful worker scaling
- **Prometheus Metrics**: Sync, download, watch-folder, API and database metrics on `:9742/metrics`
- **Web UI**: status, queue, Put.io browser, transfers, history, stats and settings in the browser on `:15432`, behind a token ([Web UI](#web-ui))
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
  - Worker scaling on settings changes from the API
  - Worker pool management

- **putio-sync-doctor**: Health checks and safe fixes
  - Read-only checks of the Put.io account, the sync state, this machine and the connection
  - Fixes that check again before acting; the ones that delete need confirmation
  - Shared by the `doctor` command, the API and the daemon's scheduled runs

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

- **putio-sync-api**: Web API and web UI
  - REST API for status, queue, file, transfer, history and settings management
  - Embedded browser UI (plain HTML, CSS and ES modules, no build step)
  - Required bearer token, with a failed-attempt lockout
  - Served by `daemon` (on `[api] bind`) and by `server`

## Installation

### macOS

**Homebrew** (recommended):
```bash
brew tap clintoncollins/putio-sync
brew install putio-sync
```

### Windows

**Winget** (recommended):
```powershell
winget install putio.putio-sync
```

Or download the latest release from [GitHub Releases](https://github.com/clintoncollins/putio-sync/releases).

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
wget https://github.com/clintoncollins/putio-sync/releases/latest/download/putio-sync_VERSION_amd64.deb
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

### `files clean-empty`

Remove empty folders from your Put.io account. See
[Put.io Empty-Folder Cleanup](#putio-empty-folder-cleanup) for what counts as empty and
what is never removed.

```bash
putio-sync files clean-empty [OPTIONS]

Options:
  --dry-run                 List the folders that would be removed; delete nothing
  -y, --yes                 Skip the confirmation prompt
```

It prints each folder (id, path, age) and a summary line, and asks before deleting unless
`--yes`. It talks to Put.io directly, so the daemon need not be running. The exit code is
non-zero when the run fails or any delete fails.

### `doctor`

Run the health checks and print a report. See [Doctor](#doctor) for what is checked.

```bash
putio-sync doctor [OPTIONS]

Options:
  --json                    Print the report (or the fix result) as JSON on stdout
  --fix <ID>                Preview and apply one fix, e.g. dead_queue_rows.remove
  --dry-run                 With --fix: show what it would act on; change nothing
  -y, --yes                 With --fix: skip the confirmation prompt
```

It needs no running daemon and never creates a database. Logs go to stderr, so `--json`
output is clean. Exit codes: 0 ok, 1 warning, 2 problem, 3 the doctor could not run. A
fix that deletes asks first, and with `--json` it needs `--yes` or `--dry-run`.

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
skip_existing = true       # Skip files that already exist locally
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
skip_existing = true                    # Skip files that already exist
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
- `PUTIO_SYNC_API_TOKEN`, `PUTIO_SYNC_API_LISTEN`, `PUTIO_SYNC_API_BIND`: web UI and API
  listener, see [Web UI](#web-ui).

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

### Verbosity

Logging and metrics detail are set in `config.toml` or by environment variables; the
variable wins when both are set.

| Variable | Setting | Allowed | Applies |
|---|---|---|---|
| `RUST_LOG` | `logging.level` + `module_filters` | any tracing `EnvFilter` directive | restart; while set, config level changes are ignored |
| `PUTIO_SYNC_LOG_FORMAT` | `logging.console_format` | `compact`, `pretty`, `json` | restart |
| `PUTIO_SYNC_METRICS_ENABLED` | `metrics.enabled` | `true`, `false`, `1`, `0` | restart |
| `PUTIO_SYNC_METRICS_BIND` | `metrics.bind` | `host:port` | restart |
| `PUTIO_SYNC_METRICS_PATH` | `metrics.path` | starts with `/` | restart |
| `PUTIO_SYNC_METRICS_DETAIL` | `metrics.detail` | `basic`, `detailed` | restart |

`logging.level` and `logging.module_filters` in the config file are live: putio-sync checks
the file every 10 s and logs `Log level changed old=info new=debug source=Config`. An invalid
level logs `Log level not changed` once and keeps the current one. An invalid value at
startup, in the file or the environment, stops putio-sync with
`invalid <setting> '<value>': expected one of <allowed>`.

The effective settings are logged at startup:

```text
Log level level=info source=Config format=compact format_source=Config
Metrics enabled bind=0.0.0.0:9742 path=/metrics detail=basic source=Config
```

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
| `putio_sync_empty_folder_runs_total` | counter | `trigger`, `result` | Finished Put.io empty-folder cleanup runs; dry runs not counted |
| `putio_sync_empty_folders_deleted_total` | counter | — | Empty folders removed from Put.io, nested ones included |
| `putio_sync_empty_folder_delete_failures_total` | counter | — | Empty-folder deletes that failed |
| `putio_sync_empty_folder_last_success_timestamp_seconds` | gauge | — | Unix time of the last successful cleanup run; absent until the first |
| `putio_sync_db_errors_total` | counter | `operation` | Database errors |
| `putio_sync_disk_free_bytes` | gauge | — | Free space for the download folder; absent if unreadable |
| `putio_sync_disk_total_bytes` | gauge | — | Size of the download folder's filesystem; absent if unreadable |
| `putio_sync_transfers_stalled` | gauge | — | Put.io transfers with no new bytes for `[transfers] stall_threshold_hours` (`daemon` only) |
| `putio_sync_doctor_check_status` | gauge | `check` | Doctor check result: 0 ok, 1 warning, 2 problem, -1 skipped (`daemon` only) |
| `putio_sync_doctor_check_items` | gauge | `check` | Items the check found at its last run |
| `putio_sync_doctor_last_run_timestamp_seconds` | gauge | — | Unix time the last doctor run finished; absent until the first |
| `putio_sync_doctor_last_run_duration_seconds` | gauge | — | Duration of the last doctor run |
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
| Transfers stuck | `putio_sync_transfers_stalled > 0` |
| Doctor problems | `putio_sync_doctor_check_status == 2` |

## Web UI

`daemon` serves a browser UI and the JSON API on one port, by default
`http://<nas>:15432/` (for example `http://192.168.1.34:15432/`). It works on a phone
as well as a desktop. The pages:

- **Status**: version, uptime, last and next sync, queue counts, active downloads,
  workers, watch folders, disk space and Put.io reachability. **Sync now** starts a
  folder sync at once; a second click while one runs says it is already in progress.
- **Queue**: pause, resume, skip, reorder (up, down, first, last), clear completed,
  cancel a running download. Higher priority downloads first; a move renumbers the
  whole queue.
- **Put.io**: browse folders, filter by name, queue a file or a whole folder
  ("found, queued, skipped, blocked"), delete from Put.io with confirmation. Delete is
  refused for files that are queued, downloading or paused. Tick rows (or **Select
  all**) to download or delete several at once, with one confirmation for the lot;
  queued files can't be ticked.
- **Transfers**: add a magnet link, URL or `.torrent` file; retry a failed transfer;
  remove a transfer (Put.io cancels it). Put.io has no pause or resume for transfers.
  A transfer still downloading with no new bytes for 6 hours gets a **stalled** badge
  and the likely reason (availability below 100%, no peers, or no progress); see
  [Stalled transfers](#stalled-transfers). **Copy magnet** copies its magnet link,
  and **Re-add** cancels it and adds the same magnet again to the same folder.
  Tick rows (or **Select all**) to retry or remove several at once. **Clear
  completed** (completed and seeding), **Clear failed** and **Clear all finished**
  remove those transfers from the Put.io transfer list after a confirm with the count;
  their files stay in the account.
- **History**: downloads and deletions, filtered by action and by name.
- **Stats**: charts of what was downloaded and when (see [Stats page](#stats-page)).
- **Doctor**: the latest health report with **Run now** and one-click fixes; a fix that
  deletes lists what it will touch and asks first (see [Doctor](#doctor)).
- **Settings**: download, watcher, logging and notification settings (see
  [Settings from the UI](#settings-from-the-ui)). The API token and Put.io
  credentials are never shown.

The UI is built into the binary and the Docker image; there is nothing else to install
or mount.

### Stats page

`#/stats` shows, for the last 24 hours, 7, 30 or 90 days, a year or everything:

- totals: files, bytes, time spent downloading and average speed;
- **Bandwidth**: bytes per hour, six hours, day or week, spread over each download's
  duration; hover, tap or use the arrow keys for a bar's details;
- **When**: an hour × weekday heatmap in your local time, by bytes or by files;
- **File types**: video, audio, archive, image, document, subtitle and other, with the
  top extensions in each;
- **Origin**: how each download reached Put.io (a watch folder, added in putio-sync,
  added on Put.io) and its top-level Put.io folder;
- **Sources**: torrent, URL or other; the tracker host; private or public.

Filters for type, source, tracker, privacy, origin, watch folder and Put.io folder sit
above the charts, and clicking a row in a breakdown sets that filter. Filters are kept
in the page address, so a reload or a bookmark keeps them.

The numbers come from putio-sync's own download history, not from Prometheus. Sources
and origins are recorded when a file is queued, starting with this version; older
downloads show as "unknown". Only the tracker's host name is stored, never the tracker
URL or its passkey. Put.io doesn't report DHT peers, so there is no DHT breakdown;
private torrents don't use DHT anyway. The page reads `GET /stats/summary` (token
required), which takes the same filters as query parameters.

### Token (required)

> **Breaking change.** The API no longer has an open mode. Without a token every request
> except `/health` is refused with 401 `API token not configured`, and the UI shows a
> "set a token and restart" notice instead of the sign-in form.

Set a token in `config.toml` or, preferably, in the environment:

```toml
[api]
auth_token = "a-long-random-string"   # or PUTIO_SYNC_API_TOKEN
```

The token must be non-empty and contain no whitespace; `auth_token = ""` stops
putio-sync at startup. Sign in with it once per browser; the browser keeps it until
you sign out. API clients send
`Authorization: Bearer <token>`. The token is never logged or returned; startup logs
only `token=set` or `token=unset`.

Five wrong tokens from one IP within 60 s lock that IP out until 60 s after its first
failure: further requests get 429 with `Retry-After`, even with the right token. Requests with
no token at all are refused but not counted.

> **Plain HTTP.** The UI and API have no TLS. The token crosses the network in clear
> text, so keep the port on a trusted LAN and don't forward it to the internet.

### Settings

| Variable | Setting | Allowed | Default |
|---|---|---|---|
| `PUTIO_SYNC_API_TOKEN` | `api.auth_token` | non-empty, no whitespace | unset (everything refused) |
| `PUTIO_SYNC_API_LISTEN` | `api.listen` | `true`, `false`, `1`, `0` | `true` |
| `PUTIO_SYNC_API_BIND` | `api.bind` | `host:port` | `0.0.0.0:15432` |

The variable wins over `config.toml`. All three need a restart. `listen` and `bind` are
read by `daemon` only; `server` keeps its `--bind`/`--port` flags (default
`0.0.0.0:15432`) and reads only the token. Set `PUTIO_SYNC_API_LISTEN=0` to turn the
listener off. If the port can't be bound, the daemon logs one
`API listener failed to start` error and keeps syncing. With `[api] enabled = true`
(downloads delegated to a remote API server) the daemon doesn't start its own listener.

Startup lines:

```text
API listener enabled bind=0.0.0.0:15432 token=set source=Config
API token not configured (set [api] auth_token or PUTIO_SYNC_API_TOKEN); all API and UI requests will be refused
```

In Docker, publish the port: `-p 15432:15432`, or add a port mapping for container
port 15432 in the Unraid template.

### Settings from the UI

A save edits only the settings that changed in `config.toml`; comments, blank lines,
ordering and keys it doesn't know stay as they are. The credentials are never written:
`[auth]` and `api.auth_token` stay as they are in the file, so a token set only through
`PUTIO_SYNC_API_TOKEN` never reaches it. The file is replaced atomically and keeps its
mode; a file that doesn't parse is left alone and the save fails. The response
names each changed setting as applying now or needing a restart; the UI shows it as is,
and a save that changed nothing says so. Under `daemon`, the UI and API share the
daemon's download manager and watcher:

| Group | Applies live | Needs a restart |
|---|---|---|
| Logging | `level`, `module_filters`, within 10 s (ignored while `RUST_LOG` is set) | `console_format`, `console_enabled`, `file_*` |
| Download | `workers` (the pool scales), `bandwidth_limit_bytes_per_sec`, `buffer_size_mb`, `max_retries`, `retry_initial_delay_secs`, `skip_existing`, `delete_after_download`, `blocked_extensions`, for the next download | `target_directory` (new downloads go there at once, but the queue database and disk figures stay on the startup folder), and the rest of the group |
| Watcher | `paths`, `parent_folder_id`, `archive_directory`, at the next scan, if the daemon started with at least one watch folder | `enabled` (also the watcher Start/Stop endpoints, which only save the flag), `delete_after_upload`; with no watch folder at startup, everything. `refresh_cycle` is unused: the daemon scans on `--watch-interval` |
| Cleanup | `enabled`, `run_at`, `min_age_hours`, `nested`, from the next run or schedule check | — |
| Notifications | — | everything |

Under `server`, the logging rows are the same (when the config file existed at
startup). Download changes reach only the API's own download manager, not the
`--download` background thread, and watcher and notification changes need a restart.
The daily cleanup runs only under `daemon`; under `server`, `min_age_hours` and `nested`
apply to manual runs.

Invalid values are refused with 400 and a JSON `error`, and nothing is saved. A
watch folder must exist before the watcher can be enabled.

### HTTP API

All routes except `/health`, `/`, `/assets/*`, `/auth/status` and `/auth/login` need
the token. Errors are `{"error": "...", "details": "..."}`.

| Route | Purpose |
|---|---|
| `GET /health` | Liveness, no token |
| `GET /auth/status` | `{"token_configured": true}`, no token |
| `POST /auth/login` | `{"token": "..."}` → 204, 401 or 429; counted by the lockout |
| `GET /status` | Everything on the status page in one response |
| `POST /local/sync` | Start a folder sync now: 202, 409 while one runs, 503 without a database |
| `GET /local/queue`, `/local/queue/stats` | Queue rows and counts |
| `POST /local/queue` | `{"file_id": 123}`: queue a file or every file under a folder → 202 with found/queued/skipped/blocked/failed, 404 if unknown |
| `POST /local/queue/{pause,resume,clear}` | Pause all, resume all, remove completed |
| `POST /local/queue/{file_id}/{move_up,move_down,move_first,move_last,skip}` | Reorder or skip one item |
| `GET /local/downloads`, `DELETE /local/downloads/{id}` | Active downloads; cancel one (404 if unknown) |
| `GET /putio/files?parent_id=`, `GET /putio/files/{id}` | Browse Put.io |
| `DELETE /putio/files/{id}` | Delete from Put.io; 409 when the file is queued, downloading or paused |
| `POST /putio/files/delete` | `{"file_ids": [1, 2]}`: delete several with one Put.io call; 409 (nothing deleted, names in `details`) when any is queued, downloading or paused; 400 for an empty list |
| `GET`/`POST /putio/transfers`, `DELETE /putio/transfers/{id}` | List, add (`{"url"}` or `{"file_data"}`), remove transfers |
| `POST /putio/transfers/{id}/retry` | Retry a failed transfer; answers with the transfer |
| `POST /putio/transfers/remove` | `{"transfer_ids": [1, 2]}`: remove several with one Put.io cancel |
| `POST /putio/transfers/retry` | `{"transfer_ids": [1, 2]}`: retry each → `{"retried": [ids], "failed": [{"id", "error"}]}` |
| `POST /putio/transfers/clear` | `{"status": "completed" \| "failed" \| "finished", "dry_run": true}`: the transfers in that group → `{"count", "transfers"}`; with `false`, removes them from the transfer list (one Put.io clean, then one cancel for any it left) |
| `POST /putio/cleanup/empty-folders` | `{"dry_run": true}`: find (or, with `false`, remove) empty Put.io folders → report with `removed`, `removed_count`, `kept_protected`, `kept_too_new`, `failures`; 409 while a run is going |
| `POST /putio/transfers/{id}/readd` | Cancel a transfer and add its magnet again to the same folder; answers `{"cancelled_id", "transfer"}`. 400 without a magnet link (nothing cancelled); 500 with the magnet in `details` if the add fails after the cancel |
| `GET /history?action=&name=&limit=&offset=` | History; `name` is a case-insensitive substring |
| `GET /history/stats`, `GET /stats/{downloads,transfers,disk}` | Totals |
| `GET`/`PUT /settings/{download,watcher,cleanup,logging,notifications}` | Settings (see above) |
| `GET /local/watcher/status`, `GET /local/workers` | Watcher and worker state |
| `GET /doctor` | The latest doctor report; 404 before the first run |
| `POST /doctor/run` | Run every check now and answer with the report (a run already going is joined, not repeated) |
| `POST /doctor/fixes/{fix}/preview` | Check again and list the items the fix would act on; changes nothing |
| `POST /doctor/fixes/{fix}/apply` | `{"items": ["key", ...], "confirm": true}` → `{"done", "skipped", "failed", "check"}`. Only items that still qualify are touched. 400 without `confirm` for a fix that deletes, 404 for an unknown fix, 409 while an empty-folder cleanup runs |

The legacy `/api/*` placeholder routes are removed, and no CORS headers are sent: the
UI is served from the same origin, and non-browser clients are unaffected.

## Advanced Features

### Put.io Empty-Folder Cleanup

Removes empty folders from your Put.io account. A folder counts as empty when it holds
nothing, or only folders that are themselves empty, so a whole empty tree goes in one run
(deepest first). Set `nested = false` to remove only folders with nothing at all in
them; their parents then go on later runs. It never removes:

- the root;
- `watch.parent_folder_id` and `sync.remote_parent_id`;
- shared folders (they also keep their parent);
- folders that unfinished transfers save to;
- folders younger than `min_age_hours` (default 24), so a folder a new transfer just
  created survives.

A folder that is kept for any reason keeps its parents too. Run it by hand with
`putio-sync files clean-empty` (`--dry-run` to preview), `POST /putio/cleanup/empty-folders`,
or **Remove empty folders** on the web UI's Put.io page, which previews the list and asks
before deleting. Turn on the daily run in `[cleanup]` or on the Settings page:

```toml
[cleanup]
enabled = true      # daily run; daemon only, off by default
run_at = "03:00"    # local time, 24-hour HH:MM
min_age_hours = 24
nested = true       # also remove folders whose sub-folders are all empty
```

The daily run deletes (it has no dry run) and is not caught up if the daemon was down at
`run_at`. "Local" is the process's time zone: in Docker set `TZ` (e.g.
`TZ=Europe/London`), otherwise it is UTC. Only one run happens at a time; each removed
folder is logged with its id, path and age.

### Doctor

The doctor runs read-only health checks and reports each one as OK, warning, problem or
skipped, in plain English with a suggestion. Run it with `putio-sync doctor`, on the web
UI's **Doctor** page (`#/doctor`, with **Run now**), or with `POST /doctor/run`. The
daemon also runs it 2 minutes after it starts and then every `interval_minutes`.

| Area | Check | Warning / problem when |
|---|---|---|
| Put.io | Put.io sign-in | The token is rejected (problem) |
| | Put.io storage | 90% used (warning), 98% (problem) |
| | Transfers | A transfer is stalled (warning; needs the daemon) or failed (problem) |
| | Empty folders | The empty-folder cleanup would remove something |
| | Duplicate files | Files of 1 MB or more with the same name and size |
| | Leftover .torrent files | A .torrent file in the account matches no transfer |
| | Watch folder | A watched file waited over an hour without becoming a transfer |
| Sync | Queue entries for deleted files | A queue row points at a file Put.io no longer has |
| | Failing downloads | A download failed 3 or more times |
| | Leftover partial downloads | A `.downloading` file older than an hour that no download owns |
| This machine | Download folder | The folder is missing or not writable (problem) |
| | Disk space | Under 10% free, or less than the queue needs (warning); under 2% (problem) |
| | Database | SQLite's integrity check fails (problem) |
| | Config file | Unknown keys, or settings that are probably a mistake |
| Connection | Put.io reachability | Put.io can't be reached (problem), errors or is slow (warning) |
| | Rate limit | Under 10% of requests left, or Put.io is limiting now |
| | Clock | This machine's clock is over 60 s off Put.io's (warning), 300 s (problem) |

Fixes, on the check's line in the UI or with `putio-sync doctor --fix <id>`:

| Fix | Does | Asks first |
|---|---|---|
| `transfers.retry` | Retries failed transfers | No |
| `transfers.readd` | Cancels stalled transfers and adds their magnet again | Yes |
| `empty_folders.clean` | Runs the empty-folder cleanup, after a dry run shows the same folders | Yes |
| `dead_queue_rows.remove` | Removes queue rows for files deleted from Put.io | Yes |
| `failing_downloads.requeue` | Puts failing downloads back in the queue | No |
| `orphan_partials.delete` | Deletes leftover partial files inside the download folder | Yes |

Every fix checks again first and acts only on the listed items that still qualify; the
rest are reported as skipped. The report never includes the token or magnet links.

```toml
[doctor]
enabled = true          # scheduled runs in the daemon; manual runs always work
interval_minutes = 60   # values below 5 count as 5
```

The daemon publishes each check's result as `putio_sync_doctor_check_status`. An example
alert:

```yaml
- alert: PutioSyncDoctorProblem
  expr: putio_sync_doctor_check_status == 2
  for: 2h
  annotations:
    summary: "putio-sync doctor: {{ $labels.check }} has a problem"
```

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

### Method 1: Unified Daemon Command (Recommended)

Use the `daemon` command, which runs download sync and torrent watching in a single process:

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
> (`watch.archive_directory`, `watch.delete_after_upload`, `download.delete_after_download`). The effective settings
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

#### Live configuration changes

Only `[logging] level` and `module_filters` are re-read from `config.toml` while running
(see [Verbosity](#verbosity)). Everything else in the file needs a restart. Some download
and watcher settings can also be changed at runtime from the web UI or the API
(`PUT /settings/download`, `PUT /settings/watcher`); a new `workers` value there scales
the pool as described below. `PUT /settings/logging` saves to `config.toml`, so its level
change is picked up the same way. [Settings from the UI](#settings-from-the-ui) lists what
applies live and what needs a restart.

At startup, `daemon` and `server` (its API and its `--download` thread) give the download
manager every `[download]` setting it has: `target_directory`, `workers`,
`buffer_size_mb`, `bandwidth_limit_bytes_per_sec`, `max_retries`,
`retry_initial_delay_secs`, `skip_existing`, `delete_after_download` and
`blocked_extensions`. The other `[download]` keys (`resume_enabled`, `chunk_size_mb`,
`progress_style`, sorting, glob patterns, `delete_empty_folders`) are read by the
one-off `download` command only. Before this, `daemon` ignored the bandwidth limit and
retry settings and always skipped files that already existed; it now follows
`skip_existing`, which defaults to `true` (the old behaviour). An explicit
`skip_existing = false` now really re-downloads and overwrites existing files, so remove
that line or set it to `true` unless you want that.

#### Stalled transfers

Put.io transfers sometimes stop at 97% or more because no peer has the missing pieces.
Put.io's API gives no access to a transfer's partial data, so putio-sync cannot rescue
what was downloaded; it can tell you which transfers are stuck and restart them.

`daemon` lists the transfers every `stall_check_interval_secs` and remembers when each
`DOWNLOADING` transfer's byte count last changed. One unchanged for
`stall_threshold_hours` is stalled: the Transfers page badges it, `GET /putio/transfers`
adds `stall: {since, seconds, reason}`, and `putio_sync_transfers_stalled` counts it. The
record is in memory, so after a restart the clock starts again. `server` does not check.

```toml
[transfers]
stall_threshold_hours = 6        # no new bytes for this long = stalled
stall_check_interval_secs = 600  # how often the daemon lists transfers
```

**Re-add** usually starts again from 0%. If the swarm still lacks the pieces it will
stall again; **Copy magnet** lets you try the link in another client instead.

#### Worker Scaling

When `workers` is raised through the API, new workers start straight away. When it is
lowered, the excess workers are told to stop:

1. A worker that is downloading finishes its current file, then exits. No download is
   interrupted.
2. Idle workers exit at once. The pool is at the new size once the busy ones finish.
3. Queue order is unchanged; the remaining workers keep taking the highest-priority item.

A partial file, for example one left by a restart, resumes from the bytes already on
disk whenever a worker picks it. It gets no priority boost; move it up the queue to
resume it sooner.

If the process stops mid-download (crash, kill, container restart), its queue row stays
`downloading`. At startup, before any worker starts, `daemon` and `server` put such rows
back to `pending` with their priority and position unchanged, and log
`Requeued interrupted downloads left in downloading state count=N` when there were any.
Paused and failed rows are left alone.

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
    image: ghcr.io/clintoncollins/putio-sync:latest
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
    image: ghcr.io/clintoncollins/putio-sync:latest
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
    image: ghcr.io/clintoncollins/putio-sync:latest
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
docker pull ghcr.io/clintoncollins/putio-sync:latest
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
  ghcr.io/clintoncollins/putio-sync:latest \
  putio-sync download --all --daemon --interval 300
```

### Volume Mounts

The Docker image uses the following directories:

- `/config` - Configuration directory (mount your config.toml here)
- `/downloads` - Default download directory
- `/watches` - Directory for watching .torrent and .magnet files (optional)

### Environment Variables

- `RUST_LOG` - Set log level (error, warn, info, debug, trace)
- `PUTIO_SYNC_API_TOKEN` - Token for the web UI and API (required to use them)
- `PUTIO_SYNC_API_LISTEN`, `PUTIO_SYNC_API_BIND` - see [Web UI](#web-ui)
- `PUTIO_SYNC_METRICS_*` - see [Verbosity](#verbosity)
- `TZ` - Time zone for the daily empty-folder cleanup's `run_at` (UTC if unset)

### Ports

The image exposes 9742 (Prometheus metrics) and 15432 (web UI and API). Publish the ones
you use, for example `-p 15432:15432 -p 9742:9742`, or add the port mappings in the
Unraid template. The UI is embedded in the binary; no extra volume or file is needed.

```bash
# The image's default command runs `daemon`, which serves the UI on 15432
export PUTIO_SYNC_API_TOKEN="$(openssl rand -hex 24)"   # save it: you sign in with it
docker run -d \
  --name putio-sync \
  --restart unless-stopped \
  -p 15432:15432 -p 9742:9742 \
  -e PUTIO_SYNC_API_TOKEN \
  -v ~/.putiosync:/config \
  -v ~/Downloads/putio:/downloads \
  ghcr.io/clintoncollins/putio-sync:latest
```

On Unraid, put the token in the template's `PUTIO_SYNC_API_TOKEN` variable instead.

### Example: Download Daemon

Run as a daemon that checks for new files every 5 minutes:

```bash
docker run -d \
  --name putio-sync-daemon \
  --restart unless-stopped \
  -v ~/.putiosync:/config \
  -v ~/Downloads/putio:/downloads \
  -e RUST_LOG=info \
  ghcr.io/clintoncollins/putio-sync:latest \
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
  ghcr.io/clintoncollins/putio-sync:latest \
  putio-sync create --watch /watches --daemon
```

### Docker Compose

Example `docker-compose.yml`:

```yaml
version: '3.8'

services:
  putio-sync-daemon:
    image: ghcr.io/clintoncollins/putio-sync:latest
    container_name: putio-sync-daemon
    restart: unless-stopped
    volumes:
      - ~/.putiosync:/config
      - ~/Downloads/putio:/downloads
    environment:
      - RUST_LOG=info
    command: putio-sync download --all --daemon --interval 300

  putio-sync-watcher:
    image: ghcr.io/clintoncollins/putio-sync:latest
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

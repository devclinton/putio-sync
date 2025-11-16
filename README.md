# putio-sync

A high-performance Rust-based CLI tool for Put.io with support for downloads, transfers, and file management.

## Features

- **Fast Downloads**: Concurrent downloads with configurable workers
- **Resumable Downloads**: Automatic resume for interrupted downloads
- **Chunked Downloads**: Split large files into chunks for parallel downloading
- **Smart Filtering**: Whitelist/blacklist patterns with glob support
- **Transfer Management**: Add torrents from files or magnet links
- **Watch Mode**: Automatically upload .torrent files from watched directories
- **Statistics & History**: Track download stats and view history
- **File Management**: List, download, and delete files on Put.io
- **SQLite Database**: Local state tracking for downloads and history
- **Progress Tracking**: Multiple progress bar styles with speed and ETA

## Architecture

This project is organized as a Cargo workspace with the following crates:

- **putio-sync-core**: Core library containing:
  - Put.io API client
  - Sync planner
  - Local state management (SQLite)
  - Configuration and authentication handling
  - Download logic with chunked and resume support

- **putio-sync-cli**: Command-line interface providing commands:
  - `login`: Authenticate with Put.io via OAuth
  - `list`: Browse files and folders with filtering
  - `download`: Download files with workers, resume, and chunking support
  - `delete`: Remove files from Put.io
  - `create`: Add torrents from files or magnet links
  - `watch`: Monitor directories for .torrent files
  - `stat`: View download statistics
  - `history`: View download and delete history

- **putio-sync-web**: Web server (coming soon) with:
  - REST API for sync management
  - React-based UI
  - Real-time progress updates

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
  --chunk-size <SIZE>       Chunk size for large files (e.g., 256MB)
  --buffer-size <SIZE>      Buffer size per file (e.g., 16MB)
  --no-resume               Disable resume of partial downloads
  --skip-existing           Skip files already downloaded
  --delete-after            Delete from Put.io after download
  --sort <ORDER>            Sort order: date, name, extension, size
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

# Resume downloads with chunking
putio-sync download --all --chunk-size 256MB

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
  -w, --watch <PATHS>...    Watch directories for .torrent files
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

# Add multiple torrents
putio-sync create file1.torrent file2.torrent

# Watch directory for torrents
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

### Basic Configuration

```toml
[auth]
auth_method = "Token"
token = "YOUR_OAUTH_TOKEN"

[download]
workers = 2
target_directory = "~/Downloads/putio"
buffer_size_mb = 16
resume_enabled = true
skip_existing = false
progress_style = "per-file"
sort_order = "by-date"

[watch]
refresh_cycle = 60
paths = []
```

### Environment Variables

- `PUTIO_SYNC_CONFIG`: Custom config file path
- `RUST_LOG`: Log level (error, warn, info, debug, trace)

## Advanced Features

### Chunked Downloads

For large files, enable chunking to download multiple parts in parallel:

```bash
putio-sync download --all --chunk-size 256MB --workers 4
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

```bash
docker build -t putio-sync .
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
- `/watches` - Directory for watching .torrent files (optional)

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
  ghcr.io/clintoncollins/putio-sync:latest \
  putio-sync download --all --daemon --interval 300
```

### Example: Torrent Watcher

Watch a directory for .torrent files and automatically upload them:

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

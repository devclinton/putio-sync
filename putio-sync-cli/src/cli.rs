//! Command-line interface definitions using clap

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Version string (will include git hash if available)
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// putio-sync - Synchronize files with Put.io
#[derive(Parser, Debug)]
#[command(
    name = "putio-sync",
    author = "Clinton Collins",
    version = VERSION,
    about = "Synchronize files with Put.io",
    long_about = "A command-line tool for synchronizing files with Put.io cloud storage.\n\
                  Supports file listing, downloading, and management."
)]
pub struct Cli {
    /// Path to config file
    #[arg(
        short = 'c',
        long = "config",
        value_name = "PATH",
        help = "Path to config file",
        env = "PUTIO_SYNC_CONFIG"
    )]
    pub config: Option<PathBuf>,

    /// Increase logging verbosity (-v, -vv, -vvv)
    #[arg(
        short = 'v',
        long = "verbose",
        action = clap::ArgAction::Count,
        help = "Increase logging verbosity"
    )]
    pub verbose: u8,

    /// Suppress all output except errors
    #[arg(
        short = 'q',
        long = "quiet",
        help = "Suppress all output except errors",
        conflicts_with = "verbose"
    )]
    pub quiet: bool,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Authenticate with Put.io using OAuth token
    #[command(about = "Authenticate with Put.io using OAuth token")]
    Login {
        /// Don't save token to config file
        #[arg(long = "no-save", help = "Don't save token to config file (token is saved by default)")]
        no_save: bool,

        /// Don't open browser automatically
        #[arg(long = "no-browser", help = "Don't open browser automatically")]
        no_browser: bool,
    },

    /// File management commands
    #[command(about = "Manage files on Put.io", subcommand)]
    Files(FilesCommands),

    /// List files and folders from Put.io (deprecated: use 'files list')
    #[command(about = "List files and folders from Put.io", hide = true)]
    List {
        /// Remote folder path or ID to list
        #[arg(value_name = "PATH", help = "Remote folder path or ID [default: root]")]
        path: Option<String>,

        /// List folder by ID instead of path
        #[arg(
            short = 'i',
            long = "id",
            value_name = "ID",
            help = "List folder by ID instead of path",
            conflicts_with = "path"
        )]
        id: Option<i64>,

        /// List files recursively
        #[arg(short = 'r', long = "recursive", help = "List files recursively")]
        recursive: bool,

        /// Long format with details
        #[arg(
            short = 'l',
            long = "long",
            help = "Long format with details (size, date, etc.)"
        )]
        long: bool,

        /// Filter files by name pattern
        #[arg(
            short = 'f',
            long = "filter",
            value_name = "PATTERN",
            help = "Filter files by name pattern (glob)"
        )]
        filter: Option<String>,

        /// Filter by file type
        #[arg(
            short = 't',
            long = "type",
            value_name = "TYPE",
            help = "Filter by type (file, folder, video, audio, image, archive)"
        )]
        file_type: Option<String>,

        /// Minimum file size filter
        #[arg(
            long = "min-size",
            value_name = "SIZE",
            help = "Filter by minimum file size (e.g., 100MB, 1GB)"
        )]
        min_size: Option<String>,

        /// Maximum file size filter
        #[arg(
            long = "max-size",
            value_name = "SIZE",
            help = "Filter by maximum file size (e.g., 100MB, 1GB)"
        )]
        max_size: Option<String>,

        /// Sort by field
        #[arg(
            long = "sort",
            value_name = "FIELD",
            help = "Sort by field (name, size, date)"
        )]
        sort: Option<String>,

        /// Reverse sort order
        #[arg(long = "reverse", help = "Reverse sort order")]
        reverse: bool,

        /// Output format
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table",
            help = "Output format (table, json, csv, tree)"
        )]
        output: String,

        /// Output filename
        #[arg(
            long = "output-file",
            value_name = "FILE",
            help = "Output filename (to be used with -o). Optional"
        )]
        output_file: Option<PathBuf>,
    },

    /// Download files from Put.io
    #[command(about = "Download files from Put.io")]
    Download {
        /// Specific file IDs to download
        #[arg(value_name = "FILES", help = "Specific file IDs to download")]
        files: Vec<i64>,

        /// Download all files
        #[arg(
            short = 'a',
            long = "all",
            help = "Download all files",
            conflicts_with = "files"
        )]
        all: bool,

        /// Filter files with glob pattern
        #[arg(
            short = 'f',
            long = "filter",
            value_name = "PATTERN",
            help = "Filter files with glob pattern (e.g., \"*.mkv\")"
        )]
        filter: Option<String>,

        /// Download directory
        #[arg(
            short = 'd',
            long = "dir",
            value_name = "PATH",
            help = "Download directory [default: from config]"
        )]
        dir: Option<PathBuf>,

        /// Number of concurrent workers
        #[arg(
            short = 'w',
            long = "workers",
            value_name = "N",
            help = "Number of concurrent workers [default: from config]"
        )]
        workers: Option<usize>,

        /// Chunk size for large files
        #[arg(
            long = "chunk-size",
            value_name = "SIZE",
            help = "Chunk size for large files (e.g., 256MB) [default: from config]"
        )]
        chunk_size: Option<String>,

        /// Buffer size per file
        #[arg(
            long = "buffer-size",
            value_name = "SIZE",
            help = "Buffer size per file (e.g., 16MB) [default: from config]"
        )]
        buffer_size: Option<String>,

        /// Bandwidth limit in bytes per second
        #[arg(
            long = "bandwidth-limit",
            value_name = "LIMIT",
            help = "Bandwidth limit (e.g., 1MB, 5MB/s) [default: unlimited]"
        )]
        bandwidth_limit: Option<String>,

        /// Disable resume of partial downloads
        #[arg(long = "no-resume", help = "Disable resume of partial downloads")]
        no_resume: bool,

        /// Skip files already downloaded
        #[arg(
            long = "skip-existing",
            help = "Skip files already downloaded"
        )]
        skip_existing: bool,

        /// Delete from Put.io after download
        #[arg(
            long = "delete-after",
            help = "Delete from Put.io after download"
        )]
        delete_after: bool,

        /// Delete empty folders after downloading files from them
        #[arg(
            long = "delete-empty-folders",
            help = "Delete empty folders after downloading all files from them (default: true)"
        )]
        delete_empty_folders: Option<bool>,

        /// Sort order
        #[arg(
            long = "sort",
            value_name = "ORDER",
            help = "Sort order (date, name, extension, size)"
        )]
        sort: Option<String>,

        /// Reverse sort order
        #[arg(long = "reverse", help = "Reverse sort order")]
        reverse: bool,

        /// Progress style
        #[arg(
            long = "progress",
            value_name = "STYLE",
            help = "Progress style (per-file, overall, individual, rotating, none)"
        )]
        progress: Option<String>,

        /// Run in daemon mode (periodic sync)
        #[arg(long = "daemon", help = "Run in daemon mode (periodic sync)")]
        daemon: bool,

        /// Daemon check interval in seconds
        #[arg(
            long = "interval",
            value_name = "SECONDS",
            default_value = "300",
            help = "Daemon check interval in seconds"
        )]
        interval: u64,
    },

    /// Delete files from Put.io (deprecated: use 'files delete')
    #[command(about = "Delete files from Put.io", hide = true)]
    Delete {
        /// Specific file IDs to delete
        #[arg(value_name = "FILES", help = "Specific file IDs to delete")]
        files: Vec<i64>,

        /// Delete all files
        #[arg(
            short = 'a',
            long = "all",
            help = "Delete all files (requires confirmation)",
            conflicts_with = "files"
        )]
        all: bool,

        /// Filter files with glob pattern
        #[arg(
            short = 'f',
            long = "filter",
            value_name = "PATTERN",
            help = "Filter files with glob pattern (e.g., \"*.txt\")"
        )]
        filter: Option<String>,

        /// Delete folders recursively
        #[arg(
            short = 'r',
            long = "recursive",
            help = "Delete folders and their contents recursively"
        )]
        recursive: bool,

        /// Delete files before date
        #[arg(
            long = "before",
            value_name = "DATE",
            help = "Delete files created before date (e.g., \"2026-01\")"
        )]
        before: Option<String>,

        /// Dry run (show what would be deleted)
        #[arg(
            long = "dry-run",
            help = "Show what would be deleted without actually deleting"
        )]
        dry_run: bool,

        /// Skip confirmation prompt
        #[arg(
            short = 'y',
            long = "yes",
            help = "Skip confirmation prompt",
            conflicts_with = "dry_run"
        )]
        yes: bool,
    },

    /// Display download statistics
    Stat {
        /// Show breakdown by file extension
        #[arg(
            long = "by-extension",
            help = "Show breakdown by file extension"
        )]
        by_extension: bool,

        /// Show stats since date
        #[arg(
            long = "since",
            value_name = "DATE",
            help = "Show stats since date (e.g., \"2025-01-01\")"
        )]
        since: Option<String>,

        /// Show stats until date
        #[arg(
            long = "until",
            value_name = "DATE",
            help = "Show stats until date"
        )]
        until: Option<String>,

        /// Output format
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table",
            help = "Output format (table, json)"
        )]
        output: String,
    },

    /// View download and delete history
    History {
        /// Filter by action
        #[arg(
            long = "action",
            value_name = "ACTION",
            help = "Filter by action (downloaded, deleted)"
        )]
        action: Option<String>,

        /// Show history since date
        #[arg(
            long = "since",
            value_name = "DATE",
            help = "Show history since date (e.g., \"2025-01-01\")"
        )]
        since: Option<String>,

        /// Show history until date
        #[arg(
            long = "until",
            value_name = "DATE",
            help = "Show history until date"
        )]
        until: Option<String>,

        /// Limit number of results
        #[arg(
            short = 'n',
            long = "limit",
            value_name = "N",
            default_value = "50",
            help = "Limit number of results"
        )]
        limit: usize,

        /// Offset for pagination
        #[arg(
            long = "offset",
            value_name = "N",
            default_value = "0",
            help = "Offset for pagination"
        )]
        offset: usize,

        /// Output format
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table",
            help = "Output format (table, json, csv)"
        )]
        output: String,
    },

    /// Add torrents to Put.io (deprecated: use 'transfers add')
    #[command(about = "Add torrents to Put.io", hide = true)]
    Create {
        /// Torrent files or magnet links to add
        #[arg(
            value_name = "SOURCES",
            help = "Torrent files (.torrent) or magnet links to add"
        )]
        sources: Vec<String>,

        /// Watch directories for .torrent files
        #[arg(
            short = 'w',
            long = "watch",
            value_name = "PATHS",
            help = "Watch directories for .torrent files",
            num_args = 1..
        )]
        watch: Vec<PathBuf>,

        /// Parent folder ID for downloads
        #[arg(
            long = "parent-id",
            value_name = "ID",
            help = "Parent folder ID for downloads [default: 0 (root)]"
        )]
        parent_id: Option<i64>,

        /// Run in daemon mode (continuous watching)
        #[arg(
            short = 'd',
            long = "daemon",
            help = "Run in daemon mode (continuous watching)"
        )]
        daemon: bool,

        /// Watch check interval in seconds
        #[arg(
            long = "interval",
            value_name = "SECONDS",
            default_value = "60",
            help = "Watch check interval in seconds"
        )]
        interval: u64,
    },

    /// Manage download queue
    #[command(about = "Manage download queue", subcommand)]
    Queue(QueueCommands),

    /// Manage Put.io transfers
    #[command(about = "Manage Put.io transfers", subcommand)]
    Transfers(TransfersCommands),

    /// Run combined download sync and torrent watch daemon with hot reloading
    #[command(about = "Run combined download sync and torrent watch daemon with hot reloading")]
    Daemon {
        /// Download sync interval in seconds
        #[arg(
            long = "download-interval",
            value_name = "SECONDS",
            default_value = "300",
            help = "Download sync interval in seconds"
        )]
        download_interval: u64,

        /// Watch check interval in seconds
        #[arg(
            long = "watch-interval",
            value_name = "SECONDS",
            default_value = "60",
            help = "Watch check interval in seconds"
        )]
        watch_interval: u64,

        /// Watch directories for .torrent files
        #[arg(
            short = 'w',
            long = "watch",
            value_name = "PATHS",
            help = "Watch directories for .torrent files (uses config if not specified)",
            num_args = 0..
        )]
        watch: Vec<PathBuf>,

        /// Filter files for download with glob pattern
        #[arg(
            short = 'f',
            long = "filter",
            value_name = "PATTERN",
            help = "Filter files for download with glob pattern (e.g., \"*.mkv\")"
        )]
        filter: Option<String>,

        /// Skip files already downloaded
        #[arg(
            long = "skip-existing",
            help = "Skip files already downloaded"
        )]
        skip_existing: bool,

        /// Delete from Put.io after successful download
        #[arg(
            long = "delete-after",
            help = "Delete from Put.io after successful download"
        )]
        delete_after: bool,

        /// Delete empty folders after downloading files from them
        #[arg(
            long = "delete-empty-folders",
            help = "Delete empty folders after downloading all files from them (default: true)"
        )]
        delete_empty_folders: Option<bool>,

        /// Parent folder ID for torrent uploads
        #[arg(
            long = "parent-id",
            value_name = "ID",
            help = "Parent folder ID for torrent uploads [default: 0 (root)]"
        )]
        parent_id: Option<i64>,

        /// Disable download sync (watch only)
        #[arg(
            long = "watch-only",
            help = "Only watch for torrents, disable download sync"
        )]
        watch_only: bool,

        /// Disable torrent watching (download only)
        #[arg(
            long = "download-only",
            help = "Only sync downloads, disable torrent watching",
            conflicts_with = "watch_only"
        )]
        download_only: bool,
        /// Archive directory for processed torrents
        #[arg(
            long = "archive-dir",
            value_name = "PATH",
            help = "Move torrent files to this directory after upload (cannot use with --delete-torrents)",
            conflicts_with = "delete_torrents"
        )]
        archive_dir: Option<PathBuf>,
        /// Delete torrent files after upload
        #[arg(
            long = "delete-torrents",
            help = "Delete torrent files after successful upload (cannot use with --archive-dir)",
            conflicts_with = "archive_dir"
        )]
        delete_torrents: bool,
    },

    /// Start HTTP API server with optional daemon modes
    #[command(about = "Start HTTP API server")]
    Server {
        /// Enable watch mode for monitoring torrent directories
        #[arg(short = 'w', long = "watch", help = "Enable watch mode for torrent files")]
        watch: bool,

        /// Enable download mode for automatic downloading
        #[arg(short = 'd', long = "download", help = "Enable automatic download mode")]
        download: bool,

        /// Port to bind the HTTP server (default: 15432)
        #[arg(short = 'p', long = "port", value_name = "PORT", default_value = "15432", help = "Port to bind HTTP server")]
        port: u16,

        /// Bind address for the HTTP server (default: 0.0.0.0)
        #[arg(short = 'b', long = "bind", value_name = "ADDRESS", default_value = "0.0.0.0", help = "Address to bind HTTP server")]
        bind: String,
    },

    /// Show system status and health information
    #[command(about = "Show system status and health information")]
    Status {
        /// Show detailed diagnostics
        #[arg(short = 'd', long = "detailed", help = "Show detailed diagnostics report")]
        detailed: bool,

        /// Output in JSON format
        #[arg(short = 'j', long = "json", help = "Output in JSON format")]
        json: bool,
    },

    /// Quick health check (exit code 0 = healthy, 1 = issues)
    #[command(about = "Quick health check - exits with status code")]
    Health {},

    /// Comprehensive debug information dump
    #[command(about = "Show comprehensive debug information for troubleshooting")]
    Debug {},
}

/// File management subcommands
#[derive(Subcommand, Debug)]
pub enum FilesCommands {
    /// List files and folders from Put.io
    #[command(about = "List files and folders from Put.io")]
    List {
        /// Remote folder path or ID to list
        #[arg(value_name = "PATH", help = "Remote folder path or ID [default: root]")]
        path: Option<String>,

        /// List folder by ID instead of path
        #[arg(
            short = 'i',
            long = "id",
            value_name = "ID",
            help = "List folder by ID instead of path",
            conflicts_with = "path"
        )]
        id: Option<i64>,

        /// List files recursively
        #[arg(short = 'r', long = "recursive", help = "List files recursively")]
        recursive: bool,

        /// Long format with details
        #[arg(
            short = 'l',
            long = "long",
            help = "Long format with details (size, date, etc.)"
        )]
        long: bool,

        /// Filter files by name pattern
        #[arg(
            short = 'f',
            long = "filter",
            value_name = "PATTERN",
            help = "Filter files by name pattern (glob)"
        )]
        filter: Option<String>,

        /// Filter by file type
        #[arg(
            short = 't',
            long = "type",
            value_name = "TYPE",
            help = "Filter by type (file, folder, video, audio, image, archive)"
        )]
        file_type: Option<String>,

        /// Minimum file size filter
        #[arg(
            long = "min-size",
            value_name = "SIZE",
            help = "Filter by minimum file size (e.g., 100MB, 1GB)"
        )]
        min_size: Option<String>,

        /// Maximum file size filter
        #[arg(
            long = "max-size",
            value_name = "SIZE",
            help = "Filter by maximum file size (e.g., 100MB, 1GB)"
        )]
        max_size: Option<String>,

        /// Sort by field
        #[arg(
            long = "sort",
            value_name = "FIELD",
            help = "Sort by field (name, size, date)"
        )]
        sort: Option<String>,

        /// Reverse sort order
        #[arg(long = "reverse", help = "Reverse sort order")]
        reverse: bool,

        /// Output format
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table",
            help = "Output format (table, json, csv, tree)"
        )]
        output: String,

        /// Output filename
        #[arg(
            long = "output-file",
            value_name = "FILE",
            help = "Output filename (to be used with -o). Optional"
        )]
        output_file: Option<PathBuf>,
    },

    /// Delete files from Put.io
    #[command(about = "Delete files from Put.io")]
    Delete {
        /// Specific file IDs to delete
        #[arg(value_name = "FILES", help = "Specific file IDs to delete")]
        files: Vec<i64>,

        /// Delete all files
        #[arg(
            short = 'a',
            long = "all",
            help = "Delete all files (requires confirmation)",
            conflicts_with = "files"
        )]
        all: bool,

        /// Filter files with glob pattern
        #[arg(
            short = 'f',
            long = "filter",
            value_name = "PATTERN",
            help = "Filter files with glob pattern (e.g., \"*.txt\")"
        )]
        filter: Option<String>,

        /// Delete folders recursively
        #[arg(
            short = 'r',
            long = "recursive",
            help = "Delete folders and their contents recursively"
        )]
        recursive: bool,

        /// Delete files before date
        #[arg(
            long = "before",
            value_name = "DATE",
            help = "Delete files created before date (e.g., \"2026-01\")"
        )]
        before: Option<String>,

        /// Dry run (show what would be deleted)
        #[arg(
            long = "dry-run",
            help = "Show what would be deleted without actually deleting"
        )]
        dry_run: bool,

        /// Skip confirmation prompt
        #[arg(
            short = 'y',
            long = "yes",
            help = "Skip confirmation prompt",
            conflicts_with = "dry_run"
        )]
        yes: bool,
    },
}

/// Queue management subcommands
#[derive(Subcommand, Debug)]
pub enum QueueCommands {
    /// List queue items
    #[command(about = "List queue items")]
    List {
        /// Filter by status
        #[arg(
            long = "status",
            value_name = "STATUS",
            help = "Filter by status (pending, downloading, paused, completed, failed)"
        )]
        status: Option<String>,

        /// Output format
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table",
            help = "Output format (table, json)"
        )]
        output: String,
    },

    /// Add items to queue
    #[command(about = "Add files to download queue")]
    Add {
        /// File IDs to add to queue
        #[arg(value_name = "FILES", help = "File IDs to add to queue")]
        files: Vec<i64>,

        /// Priority (higher = downloaded first)
        #[arg(
            short = 'p',
            long = "priority",
            value_name = "N",
            default_value = "0",
            help = "Priority (higher = downloaded first)"
        )]
        priority: i64,

        /// Add all files matching filter
        #[arg(
            short = 'a',
            long = "all",
            help = "Add all files matching filter",
            conflicts_with = "files"
        )]
        all: bool,

        /// Filter files with glob pattern
        #[arg(
            short = 'f',
            long = "filter",
            value_name = "PATTERN",
            help = "Filter files with glob pattern (e.g., \"*.mkv\")"
        )]
        filter: Option<String>,
    },

    /// Pause queue items
    #[command(about = "Pause queue items")]
    Pause {
        /// File IDs to pause
        #[arg(value_name = "FILES", help = "File IDs to pause (omit to pause all)")]
        files: Vec<i64>,
    },

    /// Resume queue items
    #[command(about = "Resume queue items")]
    Resume {
        /// File IDs to resume
        #[arg(value_name = "FILES", help = "File IDs to resume (omit to resume all)")]
        files: Vec<i64>,
    },

    /// Remove items from queue
    #[command(about = "Remove items from queue")]
    Remove {
        /// File IDs to remove
        #[arg(value_name = "FILES", help = "File IDs to remove")]
        files: Vec<i64>,
    },

    /// Clear completed items
    #[command(about = "Clear completed items from queue")]
    Clear {
        /// Skip confirmation prompt
        #[arg(
            short = 'y',
            long = "yes",
            help = "Skip confirmation prompt"
        )]
        yes: bool,
    },

    /// Update item priority
    #[command(about = "Update queue item priority")]
    Priority {
        /// File ID to update
        #[arg(value_name = "FILE", help = "File ID to update")]
        file_id: i64,

        /// New priority
        #[arg(value_name = "PRIORITY", help = "New priority (higher = downloaded first)")]
        priority: i64,
    },

    /// Show queue statistics
    #[command(about = "Show queue statistics")]
    Stats {
        /// Output format
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table",
            help = "Output format (table, json)"
        )]
        output: String,
    },
}

/// Transfer management subcommands
#[derive(Subcommand, Debug)]
pub enum TransfersCommands {
    /// List active transfers
    #[command(about = "List active transfers")]
    List {
        /// Output format
        #[arg(
            short = 'o',
            long = "output",
            value_name = "FORMAT",
            default_value = "table",
            help = "Output format (table, json)"
        )]
        output: String,
    },

    /// Add torrents to Put.io
    #[command(about = "Add torrents to Put.io")]
    Add {
        /// Torrent files or magnet links to add
        #[arg(
            value_name = "SOURCES",
            help = "Torrent files (.torrent) or magnet links to add"
        )]
        sources: Vec<String>,

        /// Watch directories for .torrent files
        #[arg(
            short = 'w',
            long = "watch",
            value_name = "PATHS",
            help = "Watch directories for .torrent files",
            num_args = 1..
        )]
        watch: Vec<PathBuf>,

        /// Parent folder ID for downloads
        #[arg(
            long = "parent-id",
            value_name = "ID",
            help = "Parent folder ID for downloads [default: 0 (root)]"
        )]
        parent_id: Option<i64>,

        /// Run in daemon mode (continuous watching)
        #[arg(
            short = 'd',
            long = "daemon",
            help = "Run in daemon mode (continuous watching)"
        )]
        daemon: bool,

        /// Watch check interval in seconds
        #[arg(
            long = "interval",
            value_name = "SECONDS",
            default_value = "60",
            help = "Watch check interval in seconds [default: 60]"
        )]
        interval: u64,
    },

    /// Cancel transfers
    #[command(about = "Cancel one or more transfers")]
    Cancel {
        /// Transfer IDs to cancel
        #[arg(value_name = "IDS", help = "Transfer IDs to cancel")]
        transfer_ids: Vec<i64>,

        /// Skip confirmation prompt
        #[arg(
            short = 'y',
            long = "yes",
            help = "Skip confirmation prompt"
        )]
        yes: bool,
    },

    /// Retry a failed transfer
    #[command(about = "Retry a failed transfer")]
    Retry {
        /// Transfer ID to retry
        #[arg(value_name = "ID", help = "Transfer ID to retry")]
        transfer_id: i64,
    },
}

impl Cli {
    /// Parse command-line arguments
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_no_args() {
        // This test just ensures the CLI structure is valid
        // Actual parsing would fail without a subcommand
    }

    #[test]
    fn test_default_output_format() {
        // Verify default output format for list command
        let cli = Cli::try_parse_from(["putio-sync", "list"]).unwrap();
        if let Commands::List { output, .. } = cli.command {
            assert_eq!(output, "table");
        } else {
            panic!("Expected List command");
        }
    }

    #[test]
    fn test_verbose_flag() {
        let cli = Cli::try_parse_from(["putio-sync", "-vvv", "list"]).unwrap();
        assert_eq!(cli.verbose, 3);
    }

    #[test]
    fn test_quiet_flag() {
        let cli = Cli::try_parse_from(["putio-sync", "--quiet", "list"]).unwrap();
        assert!(cli.quiet);
    }

    #[test]
    fn test_config_flag() {
        let cli = Cli::try_parse_from(["putio-sync", "-c", "/tmp/config.toml", "list"]).unwrap();
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/config.toml")));
    }

    #[test]
    fn test_list_with_filters() {
        let cli = Cli::try_parse_from([
            "putio-sync",
            "list",
            "--filter",
            "*.mp4",
            "--type",
            "video",
            "--long",
        ])
        .unwrap();

        if let Commands::List {
            filter,
            file_type,
            long,
            ..
        } = cli.command
        {
            assert_eq!(filter, Some("*.mp4".to_string()));
            assert_eq!(file_type, Some("video".to_string()));
            assert!(long);
        } else {
            panic!("Expected List command");
        }
    }
}

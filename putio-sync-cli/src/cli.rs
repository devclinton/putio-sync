//! Command-line interface definitions using clap

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// putio-sync - Synchronize files with Put.io
#[derive(Parser, Debug)]
#[command(
    name = "putio-sync",
    author = "Clinton Collins",
    version,
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
    /// Authenticate with Put.io and save credentials
    #[command(about = "Authenticate with Put.io and save credentials")]
    Login {
        /// Put.io OAuth token
        #[arg(
            short = 't',
            long = "token",
            value_name = "TOKEN",
            help = "Put.io OAuth token",
            conflicts_with_all = ["username", "password"]
        )]
        token: Option<String>,

        /// Put.io username or email
        #[arg(
            short = 'u',
            long = "username",
            value_name = "USERNAME",
            help = "Put.io username or email (for app-specific password auth)",
            requires = "password",
            conflicts_with = "token"
        )]
        username: Option<String>,

        /// App-specific password
        #[arg(
            short = 'p',
            long = "password",
            value_name = "PASSWORD",
            help = "App-specific password (not your main password)",
            requires = "username",
            conflicts_with = "token"
        )]
        password: Option<String>,

        /// Prompt for credentials interactively
        #[arg(
            long = "interactive",
            help = "Prompt for credentials interactively",
            conflicts_with_all = ["token", "username", "password"]
        )]
        interactive: bool,

        /// Skip credential verification
        #[arg(
            long = "no-verify",
            help = "Skip credential verification before saving"
        )]
        no_verify: bool,
    },

    /// Get OAuth token from Put.io
    #[command(about = "Helper command to obtain OAuth token from Put.io")]
    GetToken {
        /// Save token to config file
        #[arg(long = "save", help = "Save token to config file after verification")]
        save: bool,

        /// Don't open browser automatically
        #[arg(long = "no-browser", help = "Don't open browser automatically")]
        no_browser: bool,
    },

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

    /// Delete files from Put.io
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
    fn test_login_with_token() {
        let cli = Cli::try_parse_from(["putio-sync", "login", "--token", "abc123"]).unwrap();
        if let Commands::Login {
            token,
            username,
            password,
            interactive,
            no_verify,
        } = cli.command
        {
            assert_eq!(token, Some("abc123".to_string()));
            assert_eq!(username, None);
            assert_eq!(password, None);
            assert!(!interactive);
            assert!(!no_verify);
        } else {
            panic!("Expected Login command");
        }
    }

    #[test]
    fn test_login_interactive() {
        let cli = Cli::try_parse_from(["putio-sync", "login", "--interactive"]).unwrap();
        if let Commands::Login {
            token,
            username,
            password,
            interactive,
            ..
        } = cli.command
        {
            assert_eq!(token, None);
            assert_eq!(username, None);
            assert_eq!(password, None);
            assert!(interactive);
        } else {
            panic!("Expected Login command");
        }
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

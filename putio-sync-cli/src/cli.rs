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
            help = "Put.io OAuth token"
        )]
        token: Option<String>,

        /// Prompt for token interactively
        #[arg(
            long = "interactive",
            help = "Prompt for token interactively",
            conflicts_with = "token"
        )]
        interactive: bool,

        /// Skip token verification
        #[arg(
            long = "no-verify",
            help = "Skip token verification before saving"
        )]
        no_verify: bool,
    },

    /// List files and folders from Put.io
    #[command(about = "List files and folders from Put.io")]
    List {
        /// Remote folder path or ID to list
        #[arg(
            value_name = "PATH",
            help = "Remote folder path or ID [default: root]"
        )]
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
        #[arg(
            short = 'r',
            long = "recursive",
            help = "List files recursively"
        )]
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
        #[arg(
            long = "reverse",
            help = "Reverse sort order"
        )]
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
        let cli = Cli::try_parse_from(&["putio-sync", "list"]).unwrap();
        if let Commands::List { output, .. } = cli.command {
            assert_eq!(output, "table");
        } else {
            panic!("Expected List command");
        }
    }

    #[test]
    fn test_verbose_flag() {
        let cli = Cli::try_parse_from(&["putio-sync", "-vvv", "list"]).unwrap();
        assert_eq!(cli.verbose, 3);
    }

    #[test]
    fn test_quiet_flag() {
        let cli = Cli::try_parse_from(&["putio-sync", "--quiet", "list"]).unwrap();
        assert_eq!(cli.quiet, true);
    }

    #[test]
    fn test_config_flag() {
        let cli = Cli::try_parse_from(&["putio-sync", "-c", "/tmp/config.toml", "list"]).unwrap();
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/config.toml")));
    }

    #[test]
    fn test_login_with_token() {
        let cli = Cli::try_parse_from(&["putio-sync", "login", "--token", "abc123"]).unwrap();
        if let Commands::Login { token, interactive, no_verify } = cli.command {
            assert_eq!(token, Some("abc123".to_string()));
            assert_eq!(interactive, false);
            assert_eq!(no_verify, false);
        } else {
            panic!("Expected Login command");
        }
    }

    #[test]
    fn test_login_interactive() {
        let cli = Cli::try_parse_from(&["putio-sync", "login", "--interactive"]).unwrap();
        if let Commands::Login { token, interactive, .. } = cli.command {
            assert_eq!(token, None);
            assert_eq!(interactive, true);
        } else {
            panic!("Expected Login command");
        }
    }

    #[test]
    fn test_list_with_filters() {
        let cli = Cli::try_parse_from(&[
            "putio-sync",
            "list",
            "--filter",
            "*.mp4",
            "--type",
            "video",
            "--long",
        ])
        .unwrap();

        if let Commands::List { filter, file_type, long, .. } = cli.command {
            assert_eq!(filter, Some("*.mp4".to_string()));
            assert_eq!(file_type, Some("video".to_string()));
            assert_eq!(long, true);
        } else {
            panic!("Expected List command");
        }
    }
}

//! # putio-sync CLI
//!
//! Command-line interface for putio-sync

mod cli;
mod commands;
mod logging;

use cli::{Cli, Commands};
use tracing::{debug, error, info};

#[tokio::main]
async fn main() {
    let args = Cli::parse_args();

    // Initialize logging based on verbosity flags
    logging::init(args.verbose, args.quiet);

    // Log startup information
    info!("putio-sync v{}", env!("CARGO_PKG_VERSION"));

    if let Some(config_path) = &args.config {
        debug!("Config path: {}", config_path.display());
    }

    debug!("Verbosity level: {}", args.verbose);
    debug!("Quiet mode: {}", args.quiet);

    // Handle commands
    let result = match args.command {
        Commands::Login { token, interactive, no_verify } => {
            commands::execute_login(token, interactive, no_verify, args.config).await
        }
        Commands::List {
            path,
            id,
            recursive,
            long,
            filter,
            file_type,
            min_size,
            max_size,
            sort,
            reverse,
            output,
            output_file,
        } => {
            info!("List command");
            if let Some(p) = &path {
                debug!("Path: {}", p);
            } else if let Some(i) = id {
                debug!("ID: {}", i);
            } else {
                debug!("Path: / (root)");
            }

            if recursive {
                debug!("Recursive listing enabled");
            }
            if long {
                debug!("Long format enabled");
            }
            if let Some(f) = &filter {
                debug!("Filter: {}", f);
            }
            if let Some(t) = &file_type {
                debug!("Type filter: {}", t);
            }
            if let Some(s) = &min_size {
                debug!("Min size: {}", s);
            }
            if let Some(s) = &max_size {
                debug!("Max size: {}", s);
            }
            if let Some(s) = &sort {
                debug!("Sort by: {}", s);
            }
            if reverse {
                debug!("Reverse sort enabled");
            }
            debug!("Output format: {}", output);

            info!("List functionality coming soon!");
            Ok(())
        }
    };

    // Handle command execution errors
    if let Err(e) = result {
        error!("Error: {}", e);
        std::process::exit(1);
    }
}

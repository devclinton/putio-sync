//! # putio-sync CLI
//!
//! Command-line interface for putio-sync

mod cli;
mod logging;

// Import from library
use putio_sync_cli::commands;

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
        Commands::Login {
            token,
            username,
            password,
            interactive,
            no_verify,
        } => {
            commands::execute_login(
                token,
                username,
                password,
                interactive,
                no_verify,
                args.config,
            )
            .await
        }
        Commands::GetToken { save, no_browser } => {
            commands::execute_get_token(save, no_browser, args.config).await
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
            commands::execute_list(
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
                args.config,
            )
            .await
        }
    };

    // Handle command execution errors
    if let Err(e) = result {
        error!("Error: {}", e);
        std::process::exit(1);
    }
}

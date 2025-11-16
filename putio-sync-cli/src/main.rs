//! # putio-sync CLI
//!
//! Command-line interface for putio-sync

mod cli;
mod logging;

// Import from library
use putio_sync_cli::commands;

use cli::{Cli, Commands, QueueCommands, TransfersCommands};
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
        Commands::Login { no_save, no_browser } => {
            commands::execute_login(no_save, no_browser, args.config).await
        }
        Commands::Files(files_cmd) => match files_cmd {
            cli::FilesCommands::List {
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
            cli::FilesCommands::Delete {
                files,
                all,
                filter,
                recursive,
                before,
                dry_run,
                yes,
            } => {
                commands::execute_delete(
                    files,
                    all,
                    filter,
                    recursive,
                    before,
                    dry_run,
                    yes,
                    args.config,
                )
                .await
            }
        },
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
        Commands::Download {
            files,
            all,
            filter,
            dir,
            workers,
            chunk_size,
            buffer_size,
            bandwidth_limit,
            no_resume,
            skip_existing,
            delete_after,
            sort,
            reverse,
            progress,
            daemon,
            interval,
        } => {
            commands::execute_download(
                files,
                all,
                filter,
                dir,
                workers,
                chunk_size,
                buffer_size,
                bandwidth_limit,
                no_resume,
                skip_existing,
                delete_after,
                sort,
                reverse,
                progress,
                daemon,
                interval,
                args.config,
            )
            .await
        }
        Commands::Delete {
            files,
            all,
            filter,
            recursive,
            before,
            dry_run,
            yes,
        } => {
            commands::execute_delete(
                files,
                all,
                filter,
                recursive,
                before,
                dry_run,
                yes,
                args.config,
            )
            .await
        }
        Commands::Stat {
            by_extension,
            since,
            until,
            output,
        } => {
            commands::execute_stat(
                by_extension,
                since,
                until,
                output,
                args.config,
            )
            .await
        }
        Commands::History {
            action,
            since,
            until,
            limit,
            offset,
            output,
        } => {
            commands::execute_history(
                action,
                since,
                until,
                limit,
                offset,
                output,
                args.config,
            )
            .await
        }
        Commands::Create {
            sources,
            watch,
            parent_id,
            daemon,
            interval,
        } => {
            commands::execute_create(
                sources,
                watch,
                parent_id,
                daemon,
                interval,
                args.config,
            )
            .await
        }
        Commands::Queue(queue_cmd) => match queue_cmd {
            QueueCommands::List { status, output } => {
                commands::queue::execute_list(status, output, args.config).await
            }
            QueueCommands::Add {
                files,
                priority,
                all,
                filter,
            } => {
                commands::queue::execute_add(files, priority, all, filter, args.config).await
            }
            QueueCommands::Pause { files } => {
                commands::queue::execute_pause(files, args.config).await
            }
            QueueCommands::Resume { files } => {
                commands::queue::execute_resume(files, args.config).await
            }
            QueueCommands::Remove { files } => {
                commands::queue::execute_remove(files, args.config).await
            }
            QueueCommands::Clear { yes } => {
                commands::queue::execute_clear(yes, args.config).await
            }
            QueueCommands::Priority { file_id, priority } => {
                commands::queue::execute_priority(file_id, priority, args.config).await
            }
            QueueCommands::Stats { output } => {
                commands::queue::execute_stats(output, args.config).await
            }
        },
        Commands::Transfers(transfers_cmd) => match transfers_cmd {
            TransfersCommands::List { output } => {
                commands::transfers::execute_list(output, args.config).await
            }
            TransfersCommands::Cancel { transfer_ids, yes } => {
                commands::transfers::execute_cancel(transfer_ids, yes, args.config).await
            }
            TransfersCommands::Retry { transfer_id } => {
                commands::transfers::execute_retry(transfer_id, args.config).await
            }
        },
    };

    // Handle command execution errors
    if let Err(e) = result {
        error!("Error: {}", e);
        std::process::exit(1);
    }
}

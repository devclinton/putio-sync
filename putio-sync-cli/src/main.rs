//! # putio-sync CLI
//!
//! Command-line interface for putio-sync

mod cli;
mod logging;

// Import from library
use putio_sync_cli::commands;

use chrono::Utc;
use cli::{Cli, Commands, QueueCommands, TransfersCommands};
use putio_sync_core::config::Config;
use tracing::{debug, error, info};

/// Get version string with git commit info
fn get_version_with_commit() -> String {
    let version = env!("CARGO_PKG_VERSION");

    // Try to get git commit hash at compile time
    match option_env!("GIT_HASH") {
        Some(hash) => format!("v{} ({})", version, hash),
        None => {
            // If GIT_HASH wasn't set, try to get it from git command at compile time
            match std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
            {
                Ok(output) if output.status.success() => {
                    let hash_string = String::from_utf8_lossy(&output.stdout);
                    let hash = hash_string.trim();
                    if !hash.is_empty() {
                        format!("v{} ({})", version, hash)
                    } else {
                        format!("v{}", version)
                    }
                }
                _ => format!("v{}", version),
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // Get version with commit info
    let version = get_version_with_commit();

    // Initialize basic logging first for early debugging - these ALWAYS show
    eprintln!("=== PUTIO-SYNC STARTUP DEBUG ===");
    eprintln!("🚀 putio-sync {} starting up...", version);
    eprintln!("⚙️  Built for: {}", std::env::consts::OS);
    eprintln!("⚙️  Architecture: {}", std::env::consts::ARCH);
    eprintln!(
        "⚙️  Current time: {}",
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    // Force stdout/stderr flush
    use std::io::Write;
    let _ = std::io::stderr().flush();

    eprintln!("🔧 Parsing command line arguments...");
    let args = match std::panic::catch_unwind(Cli::parse_args) {
        Ok(args) => {
            eprintln!("✅ Command line arguments parsed successfully");
            args
        }
        Err(_) => {
            eprintln!("❌ Failed to parse command line arguments");
            std::process::exit(1);
        }
    };

    eprintln!("🔧 Loading configuration...");
    // Load config early for logging configuration
    let mut config = if let Some(config_path) = args.config.as_ref() {
        match Config::load(config_path) {
            Ok(cfg) => {
                eprintln!(
                    "✅ Configuration loaded successfully from {}",
                    config_path.display()
                );
                cfg
            }
            Err(e) => {
                eprintln!(
                    "⚠️  Failed to load config from {}: {}",
                    config_path.display(),
                    e
                );
                eprintln!("⚠️  Using defaults");
                Config::default()
            }
        }
    } else {
        eprintln!("⚠️  No config file specified, using defaults");
        Config::default()
    };

    eprintln!("🔧 Initializing logging system...");
    // Override log level based on verbosity flags
    if args.quiet {
        config.logging.level = "error".to_string();
    } else {
        config.logging.level = match args.verbose {
            0 => "info".to_string(),
            1 => "debug".to_string(),
            _ => "trace".to_string(),
        };
    }

    // Initialize logging system
    if let Err(e) = putio_sync_core::logging::init_logging(&config.logging) {
        eprintln!("⚠️  Failed to initialize logging: {}", e);
        eprintln!("⚠️  Falling back to basic logging");
        logging::init(args.verbose, args.quiet);
    }

    eprintln!(
        "✅ Logging initialized - level: {}, verbosity: {}, quiet: {}",
        config.logging.level, args.verbose, args.quiet
    );

    // Log startup information
    info!("putio-sync {}", version);
    eprintln!("📋 Application version: {}", version);

    if let Some(config_path) = &args.config {
        debug!("Config path: {}", config_path.display());
        eprintln!("📁 Explicit config path: {}", config_path.display());
    } else {
        eprintln!("📁 No explicit config path provided, will use defaults");
    }

    debug!("Verbosity level: {}", args.verbose);
    debug!("Quiet mode: {}", args.quiet);

    // Handle commands
    let result = match args.command {
        Commands::Login {
            no_save,
            no_browser,
        } => commands::execute_login(no_save, no_browser, args.config).await,
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
            cli::FilesCommands::CleanEmpty { dry_run, yes } => {
                commands::execute_clean_empty(dry_run, yes, args.config).await
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
            delete_empty_folders,
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
                delete_empty_folders,
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
        } => commands::execute_stat(by_extension, since, until, output, args.config).await,
        Commands::History {
            action,
            since,
            until,
            limit,
            offset,
            output,
        } => {
            commands::execute_history(action, since, until, limit, offset, output, args.config)
                .await
        }
        Commands::Create {
            sources,
            watch,
            parent_id,
            daemon,
            interval,
        } => {
            commands::execute_create(sources, watch, parent_id, daemon, interval, args.config).await
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
            } => commands::queue::execute_add(files, priority, all, filter, args.config).await,
            QueueCommands::Pause { files } => {
                commands::queue::execute_pause(files, args.config).await
            }
            QueueCommands::Resume { files } => {
                commands::queue::execute_resume(files, args.config).await
            }
            QueueCommands::Remove { files } => {
                commands::queue::execute_remove(files, args.config).await
            }
            QueueCommands::Clear { yes } => commands::queue::execute_clear(yes, args.config).await,
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
            TransfersCommands::Add {
                sources,
                watch,
                parent_id,
                daemon,
                interval,
            } => {
                commands::execute_create(sources, watch, parent_id, daemon, interval, args.config)
                    .await
            }
            TransfersCommands::Cancel { transfer_ids, yes } => {
                commands::transfers::execute_cancel(transfer_ids, yes, args.config).await
            }
            TransfersCommands::Retry { transfer_id } => {
                commands::transfers::execute_retry(transfer_id, args.config).await
            }
        },
        Commands::Daemon {
            download_interval,
            watch_interval,
            watch,
            filter,
            skip_existing,
            delete_after,
            delete_empty_folders,
            parent_id,
            watch_only,
            download_only,
            archive_dir,
            delete_torrents,
        } => {
            // The effective settings are logged by execute_daemon once the config is loaded.
            eprintln!("🔄 Entering daemon mode...");

            commands::execute_daemon(
                download_interval,
                watch_interval,
                watch,
                filter,
                skip_existing,
                delete_after,
                delete_empty_folders,
                parent_id,
                watch_only,
                download_only,
                archive_dir,
                delete_torrents,
                args.config,
            )
            .await
        }
        Commands::Server {
            watch,
            download,
            port,
            bind,
        } => commands::execute_server(watch, download, port, bind, args.config).await,
        Commands::Status { detailed, json } => {
            commands::execute_status(args.config, detailed, json).await
        }
        Commands::Health {} => commands::execute_health(args.config).await,
        Commands::Debug {} => commands::execute_debug(args.config).await,
    };

    // Handle command execution errors
    eprintln!("🏁 Command execution completed");
    if let Err(e) = result {
        eprintln!("❌ Error occurred: {}", e);
        error!("Error: {}", e);
        std::process::exit(1);
    }
    eprintln!("✅ Program completed successfully");
}

//! # putio-sync CLI
//!
//! Command-line interface for putio-sync

mod cli;
mod logging;

// Import from library
use putio_sync_cli::commands;

use chrono::Utc;
use cli::{ArchiveCommands, Cli, Commands, QueueCommands, TransfersCommands};
use putio_sync_core::config::{Config, ExistingFile};
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

/// Stack for the thread that runs the CLI. Parsing the arguments takes more than the 1 MB
/// Windows gives the main thread in debug builds, so match Linux's 8 MB.
const MAIN_STACK_SIZE: usize = 8 * 1024 * 1024;

fn main() {
    let cli = std::thread::Builder::new()
        .name("main".to_string())
        .stack_size(MAIN_STACK_SIZE)
        .spawn(run)
        .expect("failed to start the main thread");
    // A panic has already been reported by the thread; exit as a panicking main would
    if cli.join().is_err() {
        std::process::exit(101);
    }
}

#[tokio::main]
async fn run() {
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

    // Plugin checks need no config, token or logging (feature 059)
    if let Commands::Plugins(cli::PluginsCommands::Check { folder, json }) = &args.command {
        let code = commands::plugins::execute_check(folder, *json, &mut std::io::stdout().lock());
        std::process::exit(code);
    }

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

    // Secrets to mask in the lines kept for the web UI's Logs page (feature 047)
    putio_sync_core::log_redact::set_secrets(config.secret_values());

    // Initialize logging system. The doctor's stdout is its report, so its logs go to stderr.
    if matches!(
        args.command,
        Commands::Doctor { .. } | Commands::Setup { .. } | Commands::Archive(_)
    ) {
        logging::init_stderr(&config.logging.level);
    } else if let Err(e) = putio_sync_core::logging::init_logging(&config.logging) {
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
            cli::FilesCommands::FreeSpace { dry_run, yes } => {
                commands::execute_free_space(dry_run, yes, args.config).await
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
            on_existing,
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
                on_existing
                    .map(Into::into)
                    .or(skip_existing.then_some(ExistingFile::Resume)),
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
            on_existing,
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
                on_existing
                    .map(Into::into)
                    .or(skip_existing.then_some(ExistingFile::Resume)),
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
        Commands::Setup {
            download_dir,
            preset,
            watch_dir,
            metrics,
            no_metrics,
            no_dry_run,
            yes,
        } => {
            let setup_args = commands::setup::SetupArgs {
                download_dir,
                preset,
                watch_dir,
                metrics: (metrics || no_metrics).then_some(metrics),
                no_dry_run,
                yes,
            };
            let code = commands::execute_setup(setup_args, args.config).await;
            let _ = std::io::stdout().flush();
            std::process::exit(code);
        }
        Commands::Plugins(_) => unreachable!("handled before the config loads"),
        Commands::Archive(job) => {
            let job = match job {
                ArchiveCommands::Metrics {
                    common,
                    prometheus,
                    keep_days,
                    since,
                } => commands::archive::ArchiveJob::Metrics {
                    common: common.into(),
                    prometheus: prometheus.prometheus,
                    instance: prometheus.instance,
                    keep_days,
                    since,
                },
                ArchiveCommands::Db { common, keep_days } => commands::archive::ArchiveJob::Db {
                    common: common.into(),
                    keep_days,
                },
                ArchiveCommands::Summary {
                    common,
                    prometheus,
                    from,
                    to,
                } => commands::archive::ArchiveJob::Summary {
                    common: common.into(),
                    prometheus: prometheus.prometheus,
                    instance: prometheus.instance,
                    from,
                    to,
                },
            };
            let code = commands::execute_archive(job, args.config).await;
            let _ = std::io::stdout().flush();
            std::process::exit(code);
        }
        Commands::Doctor {
            json,
            fix,
            dry_run,
            yes,
        } => {
            let code = commands::execute_doctor(json, fix, dry_run, yes, args.config).await;
            let _ = std::io::stdout().flush();
            std::process::exit(code);
        }
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

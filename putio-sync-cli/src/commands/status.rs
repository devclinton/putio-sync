//! Status and diagnostics command implementation

use anyhow::Result;
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_core::t;
use std::path::PathBuf;
use tracing::{debug, info};

use crate::diagnostics::{
    format_diagnostics_report, DiagnosticsCollector, DownloadStatus, HealthCheck, HealthStatus,
    WatchStatus,
};

/// Execute the status command - show system health and diagnostics
pub async fn execute_status(
    config_path: Option<PathBuf>,
    detailed: bool,
    json: bool,
) -> Result<()> {
    info!("Generating system status report...");

    // Load configuration
    let config_file_path = config_path.or_else(|| Config::default_path().ok());

    // A config that exists but does not load is reported, then the status runs on defaults
    // so the diagnostics can still show what is wrong
    let config = match load_existing_config(config_file_path.as_deref()) {
        Ok(config) => config,
        Err(e) => {
            eprintln!(
                "{}",
                t!("Warning: {0}. Showing status with default settings.", e)
            );
            None
        }
    };

    // Create diagnostics collector
    let collector = DiagnosticsCollector::new();

    // Generate basic status information
    let download_status = generate_download_status(&config).await?;
    let watch_status = generate_watch_status(&config).await?;

    if detailed {
        // Generate full diagnostics report
        let diagnostics = collector
            .generate_diagnostics(config_file_path.as_ref(), &download_status, &watch_status)
            .await?;

        if json {
            println!("{}", serde_json::to_string_pretty(&diagnostics)?);
        } else {
            println!("{}", format_diagnostics_report(&diagnostics));
        }
    } else {
        // Generate quick status summary
        let summary = collector.generate_health_summary(&download_status, &watch_status);

        if json {
            let quick_status = serde_json::json!({
                "status": "summary",
                "message": summary,
                "download": {
                    "workers_configured": download_status.workers_configured,
                    "downloads_queued": download_status.downloads_queued,
                    "downloads_active": download_status.downloads_active
                },
                "watch": {
                    "directories_count": watch_status.directories_watched.len(),
                    "directories_accessible": watch_status.directories_accessible,
                    "files_processed": watch_status.files_processed_total
                }
            });
            println!("{}", serde_json::to_string_pretty(&quick_status)?);
        } else {
            println!("{}", t!("🔍 putio-sync Status"));
            println!("{}", summary);

            // Quick health checks
            println!("\n{}", t!("📊 Quick Health Checks"));
            let health_checks =
                generate_quick_health_checks(&config, &download_status, &watch_status).await;
            for check in health_checks {
                println!("  {} {}: {}", check.status, check.subsystem, check.message);
            }
        }
    }

    Ok(())
}

/// Load the config file if there is one: `Ok(None)` when it does not exist (not set up yet),
/// an error naming the file when it exists but cannot be read or parsed
fn load_existing_config(path: Option<&std::path::Path>) -> Result<Option<Config>> {
    match path {
        Some(path) if path.exists() => Config::load(path)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("Could not load config file {}: {e}", path.display())),
        _ => Ok(None),
    }
}

/// Execute the health command - basic health check
pub async fn execute_health(config_path: Option<PathBuf>) -> Result<()> {
    debug!("Running health check...");

    let config_file_path = config_path.or_else(|| Config::default_path().ok());
    // Unlike `status`, a health check must not pass on defaults when the real config is broken
    let config = load_existing_config(config_file_path.as_deref())?;

    let download_status = generate_download_status(&config).await?;
    let watch_status = generate_watch_status(&config).await?;

    let health_checks =
        generate_quick_health_checks(&config, &download_status, &watch_status).await;

    let mut overall_healthy = true;
    for check in &health_checks {
        match check.status {
            HealthStatus::Critical => {
                println!("❌ {}: {}", check.subsystem, check.message);
                overall_healthy = false;
            }
            HealthStatus::Warning => {
                println!("⚠️  {}: {}", check.subsystem, check.message);
                overall_healthy = false;
            }
            HealthStatus::Healthy => {
                println!("✅ {}: {}", check.subsystem, check.message);
            }
            HealthStatus::Unknown => {
                println!("❓ {}: {}", check.subsystem, check.message);
                overall_healthy = false;
            }
        }
    }

    if overall_healthy {
        println!("\n{}", t!("✅ System is healthy"));
        Ok(())
    } else {
        println!(
            "\n{}",
            t!("⚠️  System has issues - run 'putio-sync status --detailed' for more information")
        );
        std::process::exit(1);
    }
}

/// Generate download status from current configuration
async fn generate_download_status(config: &Option<Config>) -> Result<DownloadStatus> {
    let mut status = DownloadStatus::default();

    if let Some(config) = config {
        status.workers_configured = config.download.workers;

        // Check if target directory exists and is writable
        let target_dir = &config.download.target_directory;
        if target_dir.exists() && target_dir.is_dir() {
            // Try to test write access
            let test_file = target_dir.join(".putiosync_test");
            if std::fs::write(&test_file, b"test").is_ok() {
                let _ = std::fs::remove_file(&test_file);
            }
        }

        // Test API connectivity if auth is configured
        if let Some(ref auth) = config.auth {
            status.last_api_call = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );

            let client = match auth.auth_method {
                AuthMethod::Token => {
                    if let Some(ref token) = auth.token {
                        PutioApiClient::new(token.clone())
                    } else {
                        debug!("API test skipped: token not configured");
                        return Ok(status);
                    }
                }
                AuthMethod::AppPassword => {
                    if let (Some(ref username), Some(ref password)) =
                        (&auth.username, &auth.password)
                    {
                        PutioApiClient::with_credentials(username.clone(), password.clone())
                    } else {
                        debug!("API test skipped: credentials not configured");
                        return Ok(status);
                    }
                }
            };
            match client.get_account_info().await {
                Ok(_) => {
                    debug!("API connectivity test successful");
                }
                Err(e) => {
                    debug!("API connectivity test failed: {}", e);
                    status.api_errors_recent = 1;
                }
            }
        }
    }

    Ok(status)
}

/// Generate watch status from current configuration
async fn generate_watch_status(config: &Option<Config>) -> Result<WatchStatus> {
    let mut status = WatchStatus::default();

    if let Some(config) = config {
        for path in &config.watch.paths {
            status.directories_watched.push(path.display().to_string());
            if path.exists() && path.is_dir() {
                status.directories_accessible += 1;
            }
        }
    }

    Ok(status)
}

/// Generate quick health checks for summary display
async fn generate_quick_health_checks(
    config: &Option<Config>,
    download_status: &DownloadStatus,
    watch_status: &WatchStatus,
) -> Vec<HealthCheck> {
    let mut checks = Vec::new();

    // Configuration check
    if let Some(config) = config {
        if config.auth.is_none() {
            checks.push(HealthCheck::warning(
                "Config",
                "No authentication configured",
            ));
        } else if !config.download.target_directory.exists() {
            checks.push(HealthCheck::critical(
                "Config",
                "Download target directory does not exist",
            ));
        } else {
            checks.push(HealthCheck::healthy("Config", "Configuration valid"));
        }
    } else {
        checks.push(HealthCheck::critical(
            "Config",
            "No configuration file found",
        ));
    }

    // Download system check
    if download_status.workers_configured == 0 {
        checks.push(HealthCheck::warning(
            "Download",
            "No download workers configured",
        ));
    } else if download_status.api_errors_recent > 0 {
        checks.push(HealthCheck::warning(
            "Download",
            "Recent API errors detected",
        ));
    } else {
        checks.push(HealthCheck::healthy("Download", "Download system ready"));
    }

    // Watch system check
    if watch_status.directories_watched.is_empty() {
        checks.push(HealthCheck::healthy(
            "Watch",
            "No watch directories configured",
        ));
    } else if watch_status.directories_accessible == 0 {
        checks.push(HealthCheck::critical(
            "Watch",
            "No watch directories are accessible",
        ));
    } else if watch_status.directories_accessible < watch_status.directories_watched.len() {
        checks.push(HealthCheck::warning(
            "Watch",
            "Some watch directories are not accessible",
        ));
    } else {
        checks.push(HealthCheck::healthy(
            "Watch",
            "All watch directories accessible",
        ));
    }

    checks
}

/// Execute the debug command - comprehensive debug information dump
pub async fn execute_debug(config_path: Option<PathBuf>) -> Result<()> {
    println!("{}\n", t!("🔧 putio-sync Debug Information"));

    // Configuration debugging
    println!("{}", t!("📋 Configuration Analysis"));
    let config_file_path = config_path.or_else(|| Config::default_path().ok());

    if let Some(ref path) = config_file_path {
        println!("  {}", t!("Config file: {0}", path.display()));
        println!("  {}", t!("Exists: {0}", path.exists()));

        if path.exists() {
            match std::fs::metadata(path) {
                Ok(metadata) => {
                    println!("  {}", t!("Size: {0} bytes", metadata.len()));
                    println!(
                        "  {}",
                        t!("Readable: {0}", !metadata.permissions().readonly())
                    );
                }
                Err(e) => println!("  {}", t!("Metadata error: {0}", e)),
            }

            match Config::load(path) {
                Ok(config) => {
                    println!("  {}", t!("✅ Config loads successfully"));
                    println!("  {}", t!("Auth configured: {0}", config.auth.is_some()));
                    if let Some(auth) = &config.auth {
                        println!(
                            "  {}",
                            t!("Auth method: {0}", format!("{:?}", auth.auth_method))
                        );
                    }
                    println!("  {}", t!("Download workers: {0}", config.download.workers));
                    println!(
                        "  {}",
                        t!(
                            "Download target: {0}",
                            config.download.target_directory.display()
                        )
                    );
                    println!(
                        "  {}",
                        t!(
                            "Target exists: {0}",
                            config.download.target_directory.exists()
                        )
                    );
                    println!(
                        "  {}",
                        t!("Watch directories: {0}", config.watch.paths.len())
                    );
                    for (i, path) in config.watch.paths.iter().enumerate() {
                        println!(
                            "    {}",
                            t!(
                                "{0}: {1} (exists: {2})",
                                i + 1,
                                path.display(),
                                path.exists()
                            )
                        );
                    }
                }
                Err(e) => {
                    println!("  {}", t!("❌ Config load error: {0}", e));
                }
            }
        } else {
            println!("  {}", t!("❌ Config file does not exist"));
        }
    } else {
        println!("  {}", t!("❌ No config file path found"));
    }

    // Environment debugging
    println!("\n{}", t!("🌍 Environment Information"));
    println!("  {}", t!("OS: {0}", std::env::consts::OS));
    println!("  {}", t!("Architecture: {0}", std::env::consts::ARCH));
    println!(
        "  {}",
        t!("Working directory: {0}", std::env::current_dir()?.display())
    );

    if let Ok(home) = std::env::var("HOME") {
        println!("  {}", t!("Home directory: {0}", home));
    }

    // Check for common environment variables
    let env_vars = ["RUST_LOG", "PUTIO_SYNC_CONFIG", "PUTIO_TOKEN"];
    println!("  {}", t!("Environment variables:"));
    for var in &env_vars {
        match std::env::var(var) {
            Ok(value) => {
                if var.contains("TOKEN") || var.contains("PASS") {
                    println!("    {}", t!("{0}: [REDACTED]", var));
                } else {
                    println!("    {}: {}", var, value);
                }
            }
            Err(_) => println!("    {}", t!("{0}: (not set)", var)),
        }
    }

    // Network connectivity test
    println!("\n{}", t!("🌐 Network Connectivity"));
    println!("  {}", t!("Testing Put.io API connectivity..."));

    if let Some(ref path) = config_file_path {
        if let Ok(config) = Config::load(path) {
            if let Some(auth) = config.auth {
                let client = match auth.auth_method {
                    AuthMethod::Token => match auth.token {
                        Some(token) => PutioApiClient::new(token),
                        None => {
                            println!("  {}", t!("⚠️  No token configured"));
                            return Ok(());
                        }
                    },
                    AuthMethod::AppPassword => match (auth.username, auth.password) {
                        (Some(username), Some(password)) => {
                            PutioApiClient::with_credentials(username, password)
                        }
                        _ => {
                            println!("  {}", t!("⚠️  Credentials not fully configured"));
                            return Ok(());
                        }
                    },
                };
                match client.get_account_info().await {
                    Ok(account) => {
                        println!("  {}", t!("✅ API connection successful"));
                        println!("  {}", t!("Account: {0}", account.username));
                        println!("  {}", t!("Account ID: {0}", account.user_id));
                    }
                    Err(e) => {
                        println!("  {}", t!("❌ API connection failed: {0}", e));
                    }
                }
            } else {
                println!(
                    "  {}",
                    t!("⏭️  No authentication configured, skipping API test")
                );
            }
        }
    }

    // File system checks
    println!("\n{}", t!("📁 File System Checks"));
    if let Some(ref path) = config_file_path {
        if let Ok(config) = Config::load(path) {
            let target_dir = &config.download.target_directory;
            println!("  {}", t!("Download target: {0}", target_dir.display()));
            println!("    {}", t!("Exists: {0}", target_dir.exists()));

            if target_dir.exists() {
                println!("    {}", t!("Is directory: {0}", target_dir.is_dir()));

                // Test write permissions
                let test_file = target_dir.join(".putiosync_write_test");
                match std::fs::write(&test_file, b"test") {
                    Ok(_) => {
                        println!("    {}", t!("Writable: ✅"));
                        let _ = std::fs::remove_file(&test_file);
                    }
                    Err(e) => {
                        println!("    {}", t!("Writable: ❌ ({0})", e));
                    }
                }

                // Check available disk space
                if let Ok(metadata) = std::fs::metadata(target_dir) {
                    println!(
                        "    {}",
                        t!("Permissions: {0}", format!("{:?}", metadata.permissions()))
                    );
                }
            }
        }
    }

    println!("\n{}", t!("✅ Debug information collection complete"));
    Ok(())
}

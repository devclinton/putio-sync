//! Comprehensive diagnostics and state reporting system
//!
//! Provides tools for debugging user issues, capturing system state,
//! and generating helpful diagnostic reports without requiring trace logging.

use anyhow::Result;
use putio_sync_core::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Overall system health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
    Unknown,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "✅ Healthy"),
            HealthStatus::Warning => write!(f, "⚠️  Warning"),
            HealthStatus::Critical => write!(f, "❌ Critical"),
            HealthStatus::Unknown => write!(f, "❓ Unknown"),
        }
    }
}

/// Health check result for a specific subsystem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub subsystem: String,
    pub status: HealthStatus,
    pub message: String,
    pub details: HashMap<String, String>,
    pub timestamp: u64,
}

impl HealthCheck {
    pub fn healthy(subsystem: &str, message: &str) -> Self {
        Self::new(subsystem, HealthStatus::Healthy, message)
    }

    pub fn warning(subsystem: &str, message: &str) -> Self {
        Self::new(subsystem, HealthStatus::Warning, message)
    }

    pub fn critical(subsystem: &str, message: &str) -> Self {
        Self::new(subsystem, HealthStatus::Critical, message)
    }

    pub fn unknown(subsystem: &str, message: &str) -> Self {
        Self::new(subsystem, HealthStatus::Unknown, message)
    }

    fn new(subsystem: &str, status: HealthStatus, message: &str) -> Self {
        Self {
            subsystem: subsystem.to_string(),
            status,
            message: message.to_string(),
            details: HashMap::new(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    pub fn with_detail(mut self, key: &str, value: &str) -> Self {
        self.details.insert(key.to_string(), value.to_string());
        self
    }
}

/// Daemon operational statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonStats {
    pub uptime_seconds: u64,
    pub config_reloads: usize,
    pub last_config_reload: Option<u64>,
    pub download_cycles: usize,
    pub watch_cycles: usize,
    pub last_download_cycle: Option<u64>,
    pub last_watch_cycle: Option<u64>,
    pub worker_restarts: usize,
}

/// Download subsystem status
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DownloadStatus {
    pub workers_configured: usize,
    pub workers_active: usize,
    pub downloads_queued: usize,
    pub downloads_active: usize,
    pub downloads_completed_total: usize,
    pub downloads_failed_total: usize,
    pub total_bytes_downloaded: u64,
    pub current_download_speed_bps: u64,
    pub last_successful_download: Option<u64>,
    pub last_api_call: Option<u64>,
    pub api_errors_recent: usize,
}

/// Watch subsystem status  
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WatchStatus {
    pub directories_watched: Vec<String>,
    pub directories_accessible: usize,
    pub files_processed_total: usize,
    pub files_failed_total: usize,
    pub last_scan_time: Option<u64>,
    pub last_file_processed: Option<u64>,
    pub torrent_uploads_total: usize,
    pub torrent_uploads_failed: usize,
}

/// Complete system diagnostic snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemDiagnostics {
    pub timestamp: u64,
    pub version: String,
    pub overall_health: HealthStatus,
    pub health_checks: Vec<HealthCheck>,
    pub daemon_stats: DaemonStats,
    pub download_status: DownloadStatus,
    pub watch_status: WatchStatus,
    pub configuration_summary: ConfigurationSummary,
    pub environment_info: EnvironmentInfo,
    pub recent_issues: Vec<DiagnosticIssue>,
}

/// Configuration summary (no sensitive data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationSummary {
    pub config_file_path: Option<String>,
    pub config_file_exists: bool,
    pub config_file_readable: bool,
    pub auth_configured: bool,
    pub auth_method: Option<String>,
    pub download_workers: usize,
    pub download_target_exists: bool,
    pub watch_enabled: bool,
    pub watch_directories_count: usize,
    pub notifications_enabled: bool,
}

/// Environment and system information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub os: String,
    pub architecture: String,
    pub working_directory: String,
    pub home_directory: Option<String>,
    pub disk_space_available_mb: Option<u64>,
    pub memory_usage_mb: Option<u64>,
}

/// Diagnostic issue detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticIssue {
    pub severity: HealthStatus,
    pub category: String,
    pub description: String,
    pub suggestions: Vec<String>,
    pub timestamp: u64,
    pub count: usize,
}

/// Main diagnostics collector and reporter
pub struct DiagnosticsCollector {
    start_time: Instant,
    daemon_stats: DaemonStats,
    recent_issues: Vec<DiagnosticIssue>,
    issue_counts: HashMap<String, usize>,
}

impl Default for DiagnosticsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl DiagnosticsCollector {
    /// Create a new diagnostics collector
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            daemon_stats: DaemonStats::default(),
            recent_issues: Vec::new(),
            issue_counts: HashMap::new(),
        }
    }

    /// Record a daemon event
    pub fn record_daemon_event(&mut self, event: DaemonEvent) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        match event {
            DaemonEvent::ConfigReload => {
                self.daemon_stats.config_reloads += 1;
                self.daemon_stats.last_config_reload = Some(now);
            }
            DaemonEvent::DownloadCycle => {
                self.daemon_stats.download_cycles += 1;
                self.daemon_stats.last_download_cycle = Some(now);
            }
            DaemonEvent::WatchCycle => {
                self.daemon_stats.watch_cycles += 1;
                self.daemon_stats.last_watch_cycle = Some(now);
            }
            DaemonEvent::WorkerRestart => {
                self.daemon_stats.worker_restarts += 1;
            }
        }

        self.daemon_stats.uptime_seconds = self.start_time.elapsed().as_secs();
    }

    /// Record an issue for tracking and reporting
    pub fn record_issue(&mut self, issue: DiagnosticIssue) {
        let key = format!("{}:{}", issue.category, issue.description);
        let count = self.issue_counts.get(&key).unwrap_or(&0) + 1;
        self.issue_counts.insert(key, count);

        let mut updated_issue = issue;
        updated_issue.count = count;

        // Keep only recent issues (last 50)
        self.recent_issues.push(updated_issue);
        if self.recent_issues.len() > 50 {
            self.recent_issues.remove(0);
        }
    }

    /// Generate complete system diagnostics
    pub async fn generate_diagnostics(
        &self,
        config_path: Option<&PathBuf>,
        download_status: &DownloadStatus,
        watch_status: &WatchStatus,
    ) -> Result<SystemDiagnostics> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Generate health checks
        let mut health_checks = Vec::new();

        // Check configuration
        health_checks.push(self.check_configuration_health(config_path).await);

        // Check download subsystem
        health_checks.push(self.check_download_health(download_status));

        // Check watch subsystem
        health_checks.push(self.check_watch_health(watch_status));

        // Check daemon health
        health_checks.push(self.check_daemon_health());

        // Determine overall health
        let overall_health = self.determine_overall_health(&health_checks);

        // Generate configuration summary
        let config_summary = self.generate_config_summary(config_path).await;

        // Generate environment info
        let env_info = self.generate_environment_info();

        Ok(SystemDiagnostics {
            timestamp,
            version: env!("CARGO_PKG_VERSION").to_string(),
            overall_health,
            health_checks,
            daemon_stats: self.daemon_stats.clone(),
            download_status: download_status.clone(),
            watch_status: watch_status.clone(),
            configuration_summary: config_summary,
            environment_info: env_info,
            recent_issues: self.recent_issues.clone(),
        })
    }

    /// Generate a quick health summary for regular logging
    pub fn generate_health_summary(
        &self,
        download_status: &DownloadStatus,
        watch_status: &WatchStatus,
    ) -> String {
        let uptime = Duration::from_secs(self.daemon_stats.uptime_seconds);
        let uptime_str = if uptime.as_secs() < 60 {
            format!("{}s", uptime.as_secs())
        } else if uptime.as_secs() < 3600 {
            format!("{}m", uptime.as_secs() / 60)
        } else {
            format!(
                "{}h{}m",
                uptime.as_secs() / 3600,
                (uptime.as_secs() % 3600) / 60
            )
        };

        format!(
            "Daemon Health - Uptime: {}, Downloads: {}/{} active, {} queued, Watch: {} dirs, {} processed",
            uptime_str,
            download_status.downloads_active,
            download_status.workers_active,
            download_status.downloads_queued,
            watch_status.directories_accessible,
            watch_status.files_processed_total
        )
    }

    // Private helper methods for health checks
    async fn check_configuration_health(&self, config_path: Option<&PathBuf>) -> HealthCheck {
        if let Some(path) = config_path {
            if !path.exists() {
                return HealthCheck::critical("Configuration", "Config file does not exist")
                    .with_detail("path", &path.display().to_string());
            }

            match Config::load(path) {
                Ok(config) => {
                    if config.auth.is_none() {
                        HealthCheck::warning("Configuration", "No authentication configured")
                    } else {
                        HealthCheck::healthy("Configuration", "Config loaded successfully")
                            .with_detail("path", &path.display().to_string())
                    }
                }
                Err(e) => HealthCheck::critical("Configuration", "Failed to load config")
                    .with_detail("error", &e.to_string()),
            }
        } else {
            HealthCheck::warning("Configuration", "No config file path specified")
        }
    }

    fn check_download_health(&self, status: &DownloadStatus) -> HealthCheck {
        if status.workers_configured == 0 {
            return HealthCheck::warning("Download", "No download workers configured");
        }

        if status.api_errors_recent > 5 {
            return HealthCheck::critical("Download", "High number of recent API errors")
                .with_detail("errors", &status.api_errors_recent.to_string());
        }

        if status.workers_active == 0 && status.downloads_queued > 0 {
            return HealthCheck::warning("Download", "Downloads queued but no active workers")
                .with_detail("queued", &status.downloads_queued.to_string());
        }

        HealthCheck::healthy("Download", "Download subsystem operational")
            .with_detail(
                "workers",
                &format!("{}/{}", status.workers_active, status.workers_configured),
            )
            .with_detail("queued", &status.downloads_queued.to_string())
    }

    fn check_watch_health(&self, status: &WatchStatus) -> HealthCheck {
        if status.directories_watched.is_empty() {
            return HealthCheck::healthy("Watch", "No directories configured for watching");
        }

        let accessible_ratio = if status.directories_watched.is_empty() {
            1.0
        } else {
            status.directories_accessible as f64 / status.directories_watched.len() as f64
        };

        if accessible_ratio < 0.5 {
            return HealthCheck::critical(
                "Watch",
                "More than half of watch directories are inaccessible",
            )
            .with_detail("accessible", &status.directories_accessible.to_string())
            .with_detail("total", &status.directories_watched.len().to_string());
        }

        if accessible_ratio < 1.0 {
            return HealthCheck::warning("Watch", "Some watch directories are inaccessible")
                .with_detail("accessible", &status.directories_accessible.to_string())
                .with_detail("total", &status.directories_watched.len().to_string());
        }

        HealthCheck::healthy("Watch", "Watch subsystem operational")
            .with_detail("directories", &status.directories_watched.len().to_string())
            .with_detail("processed", &status.files_processed_total.to_string())
    }

    fn check_daemon_health(&self) -> HealthCheck {
        let uptime = self.daemon_stats.uptime_seconds;

        if uptime < 60 {
            return HealthCheck::warning("Daemon", "Daemon recently started")
                .with_detail("uptime_seconds", &uptime.to_string());
        }

        if self.daemon_stats.worker_restarts > 10 {
            return HealthCheck::warning("Daemon", "High number of worker restarts")
                .with_detail("restarts", &self.daemon_stats.worker_restarts.to_string());
        }

        HealthCheck::healthy("Daemon", "Daemon operating normally")
            .with_detail("uptime_seconds", &uptime.to_string())
            .with_detail(
                "config_reloads",
                &self.daemon_stats.config_reloads.to_string(),
            )
    }

    fn determine_overall_health(&self, health_checks: &[HealthCheck]) -> HealthStatus {
        let mut has_critical = false;
        let mut has_warning = false;

        for check in health_checks {
            match check.status {
                HealthStatus::Critical => has_critical = true,
                HealthStatus::Warning => has_warning = true,
                HealthStatus::Healthy => {}
                HealthStatus::Unknown => has_warning = true,
            }
        }

        if has_critical {
            HealthStatus::Critical
        } else if has_warning {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        }
    }

    async fn generate_config_summary(&self, config_path: Option<&PathBuf>) -> ConfigurationSummary {
        let (config_file_path, config_file_exists, config_file_readable, config) =
            if let Some(path) = config_path {
                let exists = path.exists();
                let readable = exists && std::fs::metadata(path).is_ok();
                let config = if readable {
                    Config::load(path).ok()
                } else {
                    None
                };
                (Some(path.display().to_string()), exists, readable, config)
            } else {
                (None, false, false, None)
            };

        if let Some(config) = config {
            ConfigurationSummary {
                config_file_path,
                config_file_exists,
                config_file_readable,
                auth_configured: config.auth.is_some(),
                auth_method: config.auth.as_ref().map(|a| format!("{:?}", a.auth_method)),
                download_workers: config.download.workers,
                download_target_exists: config.download.target_directory.exists(),
                watch_enabled: !config.watch.paths.is_empty(),
                watch_directories_count: config.watch.paths.len(),
                notifications_enabled: config.notifications.enabled,
            }
        } else {
            ConfigurationSummary {
                config_file_path,
                config_file_exists,
                config_file_readable,
                auth_configured: false,
                auth_method: None,
                download_workers: 0,
                download_target_exists: false,
                watch_enabled: false,
                watch_directories_count: 0,
                notifications_enabled: false,
            }
        }
    }

    fn generate_environment_info(&self) -> EnvironmentInfo {
        use sysinfo::{Disks, System};

        // Get disk space information for the working directory
        let disk_space_mb = std::env::current_dir()
            .ok()
            .and_then(|cwd| {
                let disks = Disks::new_with_refreshed_list();
                disks
                    .iter()
                    .find(|disk| cwd.starts_with(disk.mount_point()))
                    .map(|disk| disk.available_space() / 1024 / 1024) // Convert bytes to MB
            });

        // Get current process memory usage
        let memory_usage_mb = {
            use sysinfo::ProcessesToUpdate;

            let mut sys = System::new();
            let pid = sysinfo::Pid::from_u32(std::process::id());
            sys.refresh_processes(ProcessesToUpdate::Some(&[pid]));

            sys.process(pid)
                .map(|process| process.memory() / 1024 / 1024) // Convert bytes to MB
        };

        EnvironmentInfo {
            os: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            working_directory: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            home_directory: std::env::var("HOME").ok(),
            disk_space_available_mb,
            memory_usage_mb,
        }
    }
}

/// Events that can be recorded for daemon statistics
#[derive(Debug, Clone)]
pub enum DaemonEvent {
    ConfigReload,
    DownloadCycle,
    WatchCycle,
    WorkerRestart,
}

/// Format diagnostics for human-readable output
pub fn format_diagnostics_report(diag: &SystemDiagnostics) -> String {
    let mut report = String::new();

    report.push_str("🔍 putio-sync Diagnostics Report\n");
    report.push_str(&format!("Version: {}\n", diag.version));
    report.push_str(&format!(
        "Timestamp: {}\n",
        format_timestamp(diag.timestamp)
    ));
    report.push_str(&format!("Overall Health: {}\n\n", diag.overall_health));

    // Health checks
    report.push_str("📊 System Health Checks\n");
    for check in &diag.health_checks {
        report.push_str(&format!(
            "  {} {}: {}\n",
            check.status, check.subsystem, check.message
        ));
        for (key, value) in &check.details {
            report.push_str(&format!("    {}: {}\n", key, value));
        }
    }
    report.push('\n');

    // Daemon stats
    let uptime = Duration::from_secs(diag.daemon_stats.uptime_seconds);
    report.push_str("⏱️  Daemon Statistics\n");
    report.push_str(&format!(
        "  Uptime: {}h {}m {}s\n",
        uptime.as_secs() / 3600,
        (uptime.as_secs() % 3600) / 60,
        uptime.as_secs() % 60
    ));
    report.push_str(&format!(
        "  Config reloads: {}\n",
        diag.daemon_stats.config_reloads
    ));
    report.push_str(&format!(
        "  Download cycles: {}\n",
        diag.daemon_stats.download_cycles
    ));
    report.push_str(&format!(
        "  Watch cycles: {}\n",
        diag.daemon_stats.watch_cycles
    ));
    report.push_str(&format!(
        "  Worker restarts: {}\n",
        diag.daemon_stats.worker_restarts
    ));
    report.push('\n');

    // Download status
    report.push_str("⬇️  Download Status\n");
    report.push_str(&format!(
        "  Workers: {}/{} active\n",
        diag.download_status.downloads_active, diag.download_status.workers_configured
    ));
    report.push_str(&format!(
        "  Queue: {} downloads pending\n",
        diag.download_status.downloads_queued
    ));
    report.push_str(&format!(
        "  Completed: {} files\n",
        diag.download_status.downloads_completed_total
    ));
    report.push_str(&format!(
        "  Failed: {} files\n",
        diag.download_status.downloads_failed_total
    ));
    report.push_str(&format!(
        "  Total downloaded: {} MB\n",
        diag.download_status.total_bytes_downloaded / 1024 / 1024
    ));
    report.push('\n');

    // Watch status
    report.push_str("👁️  Watch Status\n");
    report.push_str(&format!(
        "  Directories: {}/{} accessible\n",
        diag.watch_status.directories_accessible,
        diag.watch_status.directories_watched.len()
    ));
    report.push_str(&format!(
        "  Files processed: {}\n",
        diag.watch_status.files_processed_total
    ));
    report.push_str(&format!(
        "  Upload failures: {}\n",
        diag.watch_status.torrent_uploads_failed
    ));
    report.push('\n');

    // Recent issues
    if !diag.recent_issues.is_empty() {
        report.push_str("⚠️  Recent Issues\n");
        for issue in diag.recent_issues.iter().take(5) {
            report.push_str(&format!(
                "  {} [{}] {}: {}\n",
                issue.severity, issue.category, issue.count, issue.description
            ));
            for suggestion in &issue.suggestions {
                report.push_str(&format!("    💡 {}\n", suggestion));
            }
        }
        report.push('\n');
    }

    report
}

fn format_timestamp(timestamp: u64) -> String {
    // Simple timestamp formatting - in real implementation you might want to use chrono
    format!("{}", timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_creation() {
        let check = HealthCheck::healthy("Test", "All good").with_detail("key", "value");

        assert_eq!(check.subsystem, "Test");
        assert_eq!(check.status, HealthStatus::Healthy);
        assert_eq!(check.message, "All good");
        assert_eq!(check.details.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_daemon_stats_tracking() {
        let mut collector = DiagnosticsCollector::new();

        collector.record_daemon_event(DaemonEvent::ConfigReload);
        collector.record_daemon_event(DaemonEvent::DownloadCycle);

        assert_eq!(collector.daemon_stats.config_reloads, 1);
        assert_eq!(collector.daemon_stats.download_cycles, 1);
        assert!(collector.daemon_stats.last_config_reload.is_some());
        assert!(collector.daemon_stats.last_download_cycle.is_some());
    }
}

//! Status summary route (feature 004, FR-010)
//!
//! Provides the one request behind the web UI's status page:
//! - GET  /status - Build, uptime, sync, queue, downloads, active transfers, workers,
//!   watcher, disk, reachability and auth method
//!
//! A section that cannot be produced is `null` and named in `unavailable`; the route
//! itself never fails because of one section. No token value is ever included.

use axum::{extract::State, Json};
use chrono::{DateTime, SecondsFormat, Utc};
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_download::pipeline::{summary, Summary};
use putio_sync_download::{SyncOutcome, SyncStatus};
use serde::Serialize;
use std::time::SystemTime;
use tracing::debug;

// Re-export AppState from parent
pub use crate::AppState;

/// Response of `GET /status`
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub build: BuildInfo,
    pub uptime_seconds: u64,
    pub sync: SyncSection,
    pub queue: Option<QueueCounts>,
    pub downloads: Option<Vec<ActiveDownloadInfo>>,
    /// The Active transfers card (feature 039), from the last Put.io list fetched
    pub active: Option<Summary>,
    /// The Pause local downloads switch is on (feature 039)
    pub nas_paused: bool,
    pub workers: WorkersSection,
    /// The download limit in force (feature 032), the `GET /bandwidth` body
    pub bandwidth: putio_sync_core::bandwidth::Status,
    pub watcher: WatcherSection,
    pub disk: Option<DiskSection>,
    pub reachability: Reachability,
    pub auth: AuthSection,
    pub install_channel: Option<&'static str>,
    pub unavailable: Vec<Unavailable>,
}

#[derive(Debug, Serialize)]
pub struct BuildInfo {
    pub version: &'static str,
    pub commit: &'static str,
}

/// The update channel's `kind` (feature 055, FR-012), so support output shows a wrong
/// detection; `None` without an update checker (setup mode)
fn install_channel(state: &AppState) -> Option<&'static str> {
    state.updates.as_ref().map(|u| u.snapshot().channel.kind())
}

/// Folder sync state, times in RFC 3339 UTC and durations in seconds
#[derive(Debug, Serialize)]
pub struct SyncSection {
    pub in_progress: bool,
    pub last_started_at: Option<String>,
    pub last_finished_at: Option<String>,
    pub last_duration_seconds: Option<f64>,
    pub last_outcome: Option<OutcomeSection>,
    pub next_due_at: Option<String>,
    pub interval_seconds: Option<u64>,
}

/// Result of the last sync
#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum OutcomeSection {
    Success {
        found: usize,
        queued: usize,
        skipped: usize,
        blocked: usize,
        failed: usize,
    },
    Failure {
        error: String,
    },
}

/// Queue rows per state
#[derive(Debug, Serialize)]
pub struct QueueCounts {
    pub pending: i64,
    pub downloading: i64,
    pub completed: i64,
    pub failed: i64,
    pub paused: i64,
    /// Skipped items leave the queue, so this is always 0; kept for a stable shape
    pub skipped: i64,
}

#[derive(Debug, Serialize)]
pub struct ActiveDownloadInfo {
    pub file_id: i64,
    pub filename: String,
    pub file_size: u64,
    pub bytes_downloaded: u64,
    pub percentage: f64,
    pub speed_bps: f64,
    pub eta_seconds: Option<u64>,
    pub elapsed_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct WorkersSection {
    pub busy: usize,
    pub configured: usize,
}

#[derive(Debug, Serialize)]
pub struct WatcherSection {
    pub configured: bool,
    pub paths: Vec<String>,
    /// `None` when no watcher runs in this process
    pub files_processed: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct DiskSection {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Reachability {
    /// `ok` or `unavailable`
    pub database: &'static str,
    /// `ok`, `failed` or `unknown`, from the last sync; no Put.io request is made
    pub putio: &'static str,
}

#[derive(Debug, Serialize)]
pub struct AuthSection {
    /// `token`, `app_password` or `none`
    pub putio: &'static str,
    /// Always true: the request was authenticated
    pub api_token: bool,
}

/// A section that is `null` and why
#[derive(Debug, Serialize)]
pub struct Unavailable {
    pub section: &'static str,
    pub reason: String,
}

/// Get the status summary
///
/// GET /status
pub async fn get_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let config = state.config.read().await.clone();
    let mut unavailable = Vec::new();

    let sync_status = state.download_manager.sync_status().await;

    let (queue, database) = match state.database.as_ref() {
        Some(db) => match db.queue().get_stats().await {
            Ok(stats) => (
                Some(QueueCounts {
                    pending: stats.pending,
                    downloading: stats.downloading,
                    completed: stats.completed,
                    failed: stats.failed,
                    paused: stats.paused,
                    skipped: 0,
                }),
                "ok",
            ),
            Err(e) => {
                debug!(error = %e, "Status: queue counts failed");
                (None, "unavailable")
            }
        },
        None => (None, "unavailable"),
    };
    if queue.is_none() {
        unavailable.push(Unavailable {
            section: "queue",
            reason: "database unavailable".to_string(),
        });
    }

    let downloads = match state.download_manager.get_active_downloads().await {
        Ok(list) => Some(
            list.into_iter()
                .map(|d| ActiveDownloadInfo {
                    file_id: d.file_id,
                    filename: d.filename,
                    file_size: d.file_size,
                    bytes_downloaded: d.bytes_downloaded,
                    percentage: d.percentage,
                    speed_bps: d.speed_bps,
                    eta_seconds: d.eta_seconds,
                    elapsed_seconds: d.elapsed_seconds,
                })
                .collect(),
        ),
        Err(e) => {
            debug!(error = %e, "Status: active downloads failed");
            let reason = if state.database.is_none() {
                "database unavailable".to_string()
            } else {
                e.to_string()
            };
            unavailable.push(Unavailable {
                section: "downloads",
                reason,
            });
            None
        }
    };

    let active = match state.database.as_ref() {
        Some(_) => match state
            .download_manager
            .pipeline_cached(
                &state.stall_tracker,
                config.transfers.finished_retention_hours,
            )
            .await
        {
            Ok(pipeline) => Some(summary(&pipeline)),
            Err(e) => {
                debug!(error = %e, "Status: active transfers failed");
                unavailable.push(Unavailable {
                    section: "active",
                    reason: e.to_string(),
                });
                None
            }
        },
        None => {
            unavailable.push(Unavailable {
                section: "active",
                reason: "database unavailable".to_string(),
            });
            None
        }
    };

    let workers = WorkersSection {
        busy: state.download_manager.active_workers().await,
        configured: config.download.workers,
    };

    let watcher = match state.file_watcher.as_ref() {
        Some(watcher) => {
            let stats = watcher.get_stats();
            WatcherSection {
                configured: config.watch.enabled,
                paths: stats
                    .watch_directories
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
                files_processed: Some(stats.total_processed),
            }
        }
        None => WatcherSection {
            configured: config.watch.enabled,
            paths: config
                .watch
                .paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            files_processed: None,
        },
    };

    let disk = match putio_sync_telemetry::gauges::disk_usage(&config.download.target_directory) {
        Ok((free_bytes, total_bytes)) => Some(DiskSection {
            free_bytes,
            total_bytes,
        }),
        Err(e) => {
            unavailable.push(Unavailable {
                section: "disk",
                reason: format!("statvfs failed: {}", e),
            });
            None
        }
    };

    let putio = match sync_status.last_outcome {
        Some(SyncOutcome::Success { .. }) => "ok",
        Some(SyncOutcome::Failure { .. }) => "failed",
        None => "unknown",
    };

    Json(StatusResponse {
        build: BuildInfo {
            version: env!("CARGO_PKG_VERSION"),
            commit: option_env!("PUTIO_SYNC_GIT_SHA").unwrap_or("unknown"),
        },
        uptime_seconds: state.started_at.elapsed().as_secs(),
        sync: sync_json(&sync_status),
        queue,
        downloads,
        active,
        nas_paused: state.download_manager.nas_paused().await,
        workers,
        bandwidth: state.download_manager.bandwidth().status(),
        watcher,
        disk,
        reachability: Reachability { database, putio },
        auth: AuthSection {
            putio: putio_auth(&config),
            api_token: true,
        },
        install_channel: install_channel(&state),
        unavailable,
    })
}

/// The `sync` object of `/status` (also returned by "sync now")
pub(crate) fn sync_json(status: &SyncStatus) -> SyncSection {
    SyncSection {
        in_progress: status.in_progress,
        last_started_at: status.last_started_at.map(rfc3339),
        last_finished_at: status.last_finished_at.map(rfc3339),
        last_duration_seconds: status.last_duration.map(|d| d.as_secs_f64()),
        last_outcome: status.last_outcome.as_ref().map(|o| match o {
            SyncOutcome::Success {
                found,
                queued,
                skipped,
                blocked,
                failed,
            } => OutcomeSection::Success {
                found: *found,
                queued: *queued,
                skipped: *skipped,
                blocked: *blocked,
                failed: *failed,
            },
            SyncOutcome::Failure { error } => OutcomeSection::Failure {
                error: error.clone(),
            },
        }),
        next_due_at: status.next_due_at.map(rfc3339),
        interval_seconds: status.interval.map(|d| d.as_secs()),
    }
}

/// How the daemon signs in to Put.io, never the credential itself
fn putio_auth(config: &Config) -> &'static str {
    match config.auth.as_ref() {
        Some(auth) => match auth.auth_method {
            AuthMethod::Token if auth.token.is_some() => "token",
            AuthMethod::AppPassword if auth.username.is_some() && auth.password.is_some() => {
                "app_password"
            }
            _ => "none",
        },
        None => "none",
    }
}

fn rfc3339(time: SystemTime) -> String {
    DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_core::config::AuthConfig;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn sync_json_converts_times_and_outcome() {
        let status = SyncStatus {
            in_progress: false,
            last_started_at: Some(UNIX_EPOCH + Duration::from_secs(1_761_386_400)),
            last_finished_at: Some(UNIX_EPOCH + Duration::from_secs(1_761_386_404)),
            last_duration: Some(Duration::from_millis(4200)),
            last_outcome: Some(SyncOutcome::Failure {
                error: "boom".to_string(),
            }),
            next_due_at: None,
            interval: Some(Duration::from_secs(60)),
        };
        let json = serde_json::to_value(sync_json(&status)).unwrap();
        assert_eq!(json["last_started_at"], "2025-10-25T10:00:00Z");
        assert_eq!(json["last_duration_seconds"], 4.2);
        assert_eq!(
            json["last_outcome"],
            serde_json::json!({"result": "failure", "error": "boom"})
        );
        assert!(json["next_due_at"].is_null());
        assert_eq!(json["interval_seconds"], 60);
    }

    #[test]
    fn putio_auth_names_the_method_only() {
        let mut config = Config::default();
        assert_eq!(putio_auth(&config), "none");
        config.auth = Some(AuthConfig::with_token("t".to_string()));
        assert_eq!(putio_auth(&config), "token");
        config.auth = Some(AuthConfig::with_app_password(
            "u".to_string(),
            "p".to_string(),
        ));
        assert_eq!(putio_auth(&config), "app_password");
    }
}

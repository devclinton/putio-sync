//! Configuration assistant routes (feature 015)
//!
//! - GET  /settings/assistant - What each setting affects, what applies live, the
//!   defaults of each group and the risks of the running config
//! - GET  /settings/export - The settings as a TOML file, without credentials or `[arr]`
//! - POST /settings/import - Preview or apply a settings file over the current settings
//!
//! The group previews (`POST /settings/{group}/preview`) are in `settings.rs`, next to
//! the PUTs they share their code with.

use super::settings::{body, groups, invalid, push_watcher, AppState};
use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    http::header,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use putio_sync_core::assistant::{self, Change, Risk};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Largest settings file an import accepts
const MAX_IMPORT: usize = 1024 * 1024;

/// What a save or an import would do
#[derive(Debug, Default, Serialize)]
pub struct Preview {
    /// Whether the save would be accepted
    pub valid: bool,
    /// Why it would be refused: the text the save returns as 400
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The fields that would change
    pub changes: Vec<Change>,
    /// When the live fields apply; empty when none apply live
    pub when: &'static str,
    /// Risky combinations in the config the save would give
    pub risks: Vec<Risk>,
    /// Import only: keys the config does not know, or old spellings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unknown: Option<Vec<putio_sync_core::config_lint::ConfigWarning>>,
    /// Import only: parts of the file that were left out (credentials, `[arr]`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored: Option<Vec<String>>,
    /// Import apply only: what was done
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Preview {
    /// A save that would be refused for `error`
    pub fn refused(error: ApiError, when: &'static str) -> Self {
        Preview {
            error: Some(reason(error)),
            when,
            ..Preview::default()
        }
    }
}

fn reason(error: ApiError) -> String {
    match error {
        ApiError::BadRequest(message) => message,
        other => other.to_string(),
    }
}

/// What each field of each group affects, in plain words. The test in
/// `tests/settings_assistant.rs` checks every field of every group view has an entry.
const AFFECTS: &[(&str, &[(&str, &str)])] = &[
    (
        "download",
        &[
            ("workers", "How many files download at the same time"),
            (
                "target_directory",
                "Where downloads are saved on this machine",
            ),
            (
                "buffer_size_mb",
                "Memory used per download while writing to disk",
            ),
            (
                "on_existing",
                "What happens when a file is already in the download folder",
            ),
            (
                "delete_after_download",
                "Deletes the file from Put.io once it is downloaded",
            ),
            (
                "resume_enabled",
                "Continues interrupted downloads instead of starting over",
            ),
            (
                "chunk_size_mb",
                "Size of each request to Put.io; empty picks one",
            ),
            (
                "progress_style",
                "How the command line shows download progress",
            ),
            ("sort_order", "Which files download first"),
            ("reverse_sort", "Reverses the download order"),
            (
                "whitelist_patterns",
                "Only files matching these patterns download",
            ),
            (
                "blacklist_patterns",
                "Files matching these patterns are skipped",
            ),
            ("blocked_extensions", "File types that never download"),
            (
                "bandwidth_limit_bytes_per_sec",
                "Caps total download speed; empty is no cap",
            ),
            ("max_retries", "How often a failed download is tried again"),
            (
                "retry_initial_delay_secs",
                "Wait before the first retry; it doubles each time",
            ),
            (
                "delete_empty_folders",
                "Removes Put.io folders left empty after a delete",
            ),
        ],
    ),
    (
        "watcher",
        &[
            (
                "enabled",
                "Uploads torrent and magnet files dropped in the watch folders",
            ),
            ("paths", "Folders scanned for torrent and magnet files"),
            (
                "refresh_cycle",
                "Seconds between scans of the watch folders",
            ),
            (
                "parent_folder_id",
                "Put.io folder the transfers are added to",
            ),
            (
                "archive_directory",
                "Where uploaded torrent files are moved",
            ),
            (
                "delete_after_upload",
                "Deletes torrent files once uploaded instead of keeping them",
            ),
        ],
    ),
    (
        "cleanup",
        &[
            ("enabled", "Runs the daily removal of empty Put.io folders"),
            ("run_at", "Time of day the cleanup runs"),
            (
                "min_age_hours",
                "Folders newer than this are kept, so new transfers are safe",
            ),
            (
                "nested",
                "Also removes folders that hold only empty folders",
            ),
        ],
    ),
    (
        "logging",
        &[
            ("level", "How much detail the logs hold"),
            (
                "file_enabled",
                "Writes logs to a file as well as the console",
            ),
            ("file_path", "Where the log file is written"),
            ("console_enabled", "Writes logs to the console"),
            (
                "console_format",
                "Console log layout: text for people, JSON for tools",
            ),
            (
                "module_filters",
                "Per-module log levels, such as putio_sync_client=debug",
            ),
        ],
    ),
    (
        "notifications",
        &[
            (
                "enabled",
                "Sends a notification when downloads finish or fail",
            ),
            ("backend", "How notifications are sent"),
            ("webhook_url", "Where webhook notifications are posted"),
            ("email", "Where email notifications are sent"),
        ],
    ),
    (
        "precheck",
        &[
            ("enabled", "Checks releases before they are added to Put.io"),
            (
                "dry_run",
                "Logs what the check would refuse without refusing it",
            ),
            ("min_size_mb", "Releases smaller than this are refused"),
            (
                "allow_archives",
                "Allows releases that hold archives such as RAR",
            ),
            (
                "magnet_lookup",
                "Looks up magnet links in caches to see their files",
            ),
            ("lookup_timeout_secs", "How long a cache lookup may take"),
            (
                "hold_magnet_secs",
                "How long a magnet waits for its file list",
            ),
            (
                "hold_retry_secs",
                "Wait between tries while a magnet is held",
            ),
            ("first_lookup_secs", "Wait before the first cache lookup"),
            ("caches", "Caches asked for a magnet's file list"),
            (
                "categories",
                "Different rules per Sonarr or Radarr category",
            ),
            ("watch", "Different rules per watch folder"),
        ],
    ),
];

/// Routes merged into the `/settings` router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/assistant", get(get_assistant))
        .route("/export", get(export_settings))
        .route("/import", post(import_settings))
}

#[derive(Debug, Serialize)]
struct GroupHelp {
    affects: BTreeMap<&'static str, &'static str>,
    live: &'static [&'static str],
    when: &'static str,
    defaults: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct AssistantResponse {
    groups: BTreeMap<&'static str, GroupHelp>,
    risks: Vec<Risk>,
}

/// GET /settings/assistant
async fn get_assistant(State(state): State<AppState>) -> Json<AssistantResponse> {
    let config = state.config.read().await.clone();
    let groups = groups(&state, &config)
        .into_iter()
        .map(|g| {
            let affects = AFFECTS
                .iter()
                .find(|(key, _)| *key == g.key)
                .map(|(_, fields)| fields.iter().copied().collect())
                .unwrap_or_default();
            let help = GroupHelp {
                affects,
                live: g.live,
                when: g.when,
                defaults: g.defaults,
            };
            (g.key, help)
        })
        .collect();
    Json(AssistantResponse {
        groups,
        risks: assistant::risks(&config),
    })
}

/// GET /settings/export
async fn export_settings(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let text = assistant::export_toml(&*state.config.read().await).map_err(invalid)?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"putio-sync-settings.toml\"",
            ),
        ],
        text,
    ))
}

#[derive(Debug, Deserialize)]
struct ImportBody {
    toml: String,
    #[serde(default)]
    apply: bool,
}

/// The settings group a config section belongs to, for what applies live
fn group_of(section: &str) -> &str {
    match section {
        "watch" => "watcher",
        other => other,
    }
}

/// POST /settings/import
async fn import_settings(
    State(state): State<AppState>,
    request: Result<Json<ImportBody>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let request = body(request)?;
    if request.toml.len() > MAX_IMPORT {
        return Err(ApiError::BadRequest(
            "The settings file is larger than 1 MB.".to_string(),
        ));
    }

    // The write lock is held from the merge to the save when applying, so a save in
    // between cannot be lost
    let mut config = state.config.write().await;
    let imported = assistant::import(&config, &request.toml).map_err(invalid)?;
    let live: BTreeMap<_, _> = groups(&state, &config)
        .into_iter()
        .map(|g| (g.key, g.live))
        .collect();
    // A loaded config has its paths expanded; the import's are too, so compare like that
    let mut current = config.clone();
    let _ = current.expand_paths();
    let mut changes = assistant::diff(&current, &imported.config).map_err(invalid)?;
    for change in &mut changes {
        let (section, field) = change.key.split_once('.').unwrap_or((&change.key, ""));
        let field = field.split('.').next().unwrap_or_default();
        change.live = live
            .get(group_of(section))
            .is_some_and(|fields| fields.contains(&field));
    }
    let mut preview = Preview {
        valid: true,
        risks: assistant::risks(&imported.config),
        unknown: Some(imported.unknown),
        ignored: Some(imported.ignored),
        ..Preview::default()
    };
    if let Err(e) = imported.config.validate() {
        if request.apply {
            return Err(invalid(e));
        }
        preview.valid = false;
        preview.error = Some(reason(invalid(e)));
    }

    if request.apply && preview.valid {
        let candidate = imported.config;
        if let Some(ref path) = state.config_path {
            candidate
                .save_keeping_file_credentials(path.as_ref())
                .map_err(|e| ApiError::InternalError(format!("Failed to persist config: {}", e)))?;
        }
        state.doctor.configure(&candidate);
        state.cleanup.configure(&candidate);
        state.storage.configure(&candidate);
        let download = crate::extract_download_config(&candidate);
        *config = candidate;
        drop(config);
        push_watcher(&state).await;
        state.download_manager.update_config(download).await;
        tracing::info!(changes = changes.len(), "settings imported");

        let restart: Vec<&str> = changes
            .iter()
            .filter(|c| !c.live)
            .map(|c| c.key.as_str())
            .collect();
        preview.message = Some(match changes.len() {
            0 => "Nothing to import; the settings already match.".to_string(),
            n => {
                let mut message =
                    format!("Imported {} change{}.", n, if n == 1 { "" } else { "s" });
                if !restart.is_empty() {
                    message.push_str(&format!(" Needs a restart: {}.", restart.join(", ")));
                }
                message
            }
        });
    }
    preview.changes = changes;
    Ok(Json(preview))
}

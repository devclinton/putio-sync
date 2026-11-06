//! Local files routes (feature 044, contracts/api.md)
//!
//! Browse and fix by hand what is in the download folder and the watch folders:
//! - GET    /local/files                     - The roots: downloads, and each watch path
//!   when watching is on
//! - GET    /local/files/{root}?path=        - One folder of a root
//! - GET    /local/files/{root}/summary?path= - Files and bytes under an entry
//! - DELETE /local/files/{root}?path=        - Delete a file, link or folder
//! - POST   /local/files/{root}/rename       - `{path, name}`: rename in place
//! - GET/POST /local/files/{root}/subtitles - Fetch subtitles for a video (025, see `subtitles.rs`)
//! - POST   /local/files/{root}/requeue      - `{path}`: give a `.rejected` watch file its
//!   name back so the watcher checks it again
//!
//! Paths are relative to the root; `putio_sync_file_manager::local_files` keeps them in it.

use crate::error::ApiError;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use putio_sync_file_manager::local_files::{self, Entry, LocalFileError, Resolved};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::{info, warn};

// Re-export AppState from parent
pub use crate::AppState;

/// A folder the page may show and act in
#[derive(Debug, Serialize)]
pub struct RootInfo {
    pub key: String,
    pub label: String,
    pub path: String,
    pub exists: bool,
    /// Re-queue is offered only in watch roots
    pub watch: bool,
}

#[derive(Debug, Serialize)]
pub struct RootsResponse {
    pub roots: Vec<RootInfo>,
}

#[derive(Debug, Serialize)]
pub struct EntryInfo {
    pub name: String,
    pub path: String,
    pub kind: &'static str,
    pub size: u64,
    pub modified: Option<String>,
    pub partial: Option<String>,
    pub rejected: Option<String>,
    pub in_use: bool,
    pub valid_name: bool,
}

#[derive(Debug, Serialize)]
pub struct ListingResponse {
    pub root: String,
    pub path: String,
    pub entries: Vec<EntryInfo>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub in_use: bool,
}

#[derive(Debug, Serialize)]
pub struct ActionResponse {
    pub success: bool,
    pub message: String,
    /// The entry's relative path after the action; absent after a delete
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct RequeueRequest {
    pub path: String,
}

/// Create the local files router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_roots))
        .route("/:root", get(list_folder).delete(delete_entry))
        .route("/:root/summary", get(summarize))
        .route("/:root/rename", post(rename_entry))
        .route("/:root/requeue", post(requeue_entry))
        .route(
            "/:root/subtitles",
            get(super::subtitles::preflight).post(super::subtitles::start),
        )
}

/// One configured root
pub(super) struct Root {
    pub(super) key: String,
    pub(super) label: String,
    pub(super) path: PathBuf,
    pub(super) watch: bool,
}

/// The roots from the live config: downloads, then each watch path when watching is on
async fn roots(state: &AppState) -> Vec<Root> {
    let config = state.config.read().await;
    let mut roots = vec![Root {
        key: "downloads".into(),
        label: "Downloads".into(),
        path: config.download.target_directory.clone(),
        watch: false,
    }];
    if config.watch.enabled {
        for (i, path) in config.watch.paths.iter().enumerate() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            roots.push(Root {
                key: format!("watch-{i}"),
                label: format!("Watch: {name}"),
                path: path.clone(),
                watch: true,
            });
        }
    }
    roots
}

async fn find_root(state: &AppState, key: &str) -> Result<Root, ApiError> {
    roots(state)
        .await
        .into_iter()
        .find(|r| r.key == key)
        .ok_or_else(|| ApiError::NotFound(format!("No folder called {key}")))
}

fn api_error(e: LocalFileError) -> ApiError {
    match e {
        LocalFileError::Invalid(msg) => ApiError::BadRequest(msg),
        LocalFileError::NotFound(msg) => ApiError::NotFound(msg),
        LocalFileError::Conflict(msg) => ApiError::Conflict(msg),
        LocalFileError::Io(e) => ApiError::InternalError(e.to_string()),
    }
}

/// The root named `key` and `rel` resolved inside it
pub(super) async fn resolve(
    state: &AppState,
    key: &str,
    rel: &str,
) -> Result<(Root, Resolved), ApiError> {
    let root = find_root(state, key).await?;
    let resolved = local_files::resolve(&root.path, rel).map_err(api_error)?;
    Ok((root, resolved))
}

/// Partial downloads a worker writes now and putio-sync's own database files, canonical so
/// they compare with resolved paths
fn in_use(state: &AppState) -> HashSet<PathBuf> {
    let database = state.database.as_deref().map(|db| db.files());
    local_files::canonical_in_use(
        state
            .download_manager
            .partial_paths_in_use()
            .into_iter()
            .chain(database.into_iter().flatten()),
    )
}

fn entry_info(entry: Entry) -> EntryInfo {
    EntryInfo {
        kind: entry.kind.as_str(),
        modified: entry
            .modified
            .map(|t| DateTime::<Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        name: entry.name,
        path: entry.path,
        size: entry.size,
        partial: entry.partial,
        rejected: entry.rejected,
        in_use: entry.in_use,
        valid_name: entry.valid_name,
    }
}

/// GET /local/files
async fn list_roots(State(state): State<AppState>) -> Json<RootsResponse> {
    let roots = roots(&state)
        .await
        .into_iter()
        .map(|r| RootInfo {
            exists: r.path.is_dir(),
            path: r.path.display().to_string(),
            key: r.key,
            label: r.label,
            watch: r.watch,
        })
        .collect();
    Json(RootsResponse { roots })
}

/// GET /local/files/{root}?path=
async fn list_folder(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(query): Query<PathQuery>,
) -> Result<Json<ListingResponse>, ApiError> {
    let (_, dir) = resolve(&state, &key, &query.path).await?;
    let listing = local_files::list(&dir, &in_use(&state)).map_err(api_error)?;
    Ok(Json(ListingResponse {
        root: key,
        path: dir.rel,
        entries: listing.entries.into_iter().map(entry_info).collect(),
        truncated: listing.truncated,
    }))
}

/// GET /local/files/{root}/summary?path=
async fn summarize(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(query): Query<PathQuery>,
) -> Result<Json<SummaryResponse>, ApiError> {
    let (_, target) = resolve(&state, &key, &query.path).await?;
    let s = local_files::summary(&target, &in_use(&state)).map_err(api_error)?;
    Ok(Json(SummaryResponse {
        files: s.files,
        folders: s.folders,
        bytes: s.bytes,
        in_use: s.in_use,
    }))
}

/// Log the outcome of an action and turn its error into the API's
fn logged<T>(
    action: &'static str,
    root: &str,
    rel: &str,
    result: Result<T, LocalFileError>,
) -> Result<T, ApiError> {
    match &result {
        Ok(_) => info!(action, root, path = rel, "Local file changed by hand"),
        Err(e) => warn!(action, root, path = rel, error = %e, "Local file action refused"),
    }
    result.map_err(api_error)
}

/// DELETE /local/files/{root}?path=
async fn delete_entry(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(query): Query<PathQuery>,
) -> Result<Json<ActionResponse>, ApiError> {
    let (_, target) = resolve(&state, &key, &query.path).await?;
    logged(
        "delete",
        &key,
        &target.rel,
        local_files::delete(&target, &in_use(&state)),
    )?;
    Ok(Json(ActionResponse {
        success: true,
        message: format!("Deleted {}", target.rel),
        path: None,
    }))
}

/// POST /local/files/{root}/rename
async fn rename_entry(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<RenameRequest>,
) -> Result<Json<ActionResponse>, ApiError> {
    let (_, target) = resolve(&state, &key, &body.path).await?;
    let path = logged(
        "rename",
        &key,
        &target.rel,
        local_files::rename(&target, &body.name, &in_use(&state)),
    )?;
    Ok(Json(ActionResponse {
        success: true,
        message: format!("Renamed {} to {}", target.rel, body.name),
        path: Some(path),
    }))
}

/// POST /local/files/{root}/requeue
async fn requeue_entry(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<RequeueRequest>,
) -> Result<Json<ActionResponse>, ApiError> {
    let (root, target) = resolve(&state, &key, &body.path).await?;
    if !root.watch {
        return Err(ApiError::BadRequest(
            "Only files in a watch folder can be checked again".into(),
        ));
    }
    let result = local_files::requeue(
        &target,
        state.database.as_deref(),
        state.file_watcher.as_deref(),
    )
    .await;
    let path = logged("requeue", &key, &target.rel, result)?;
    Ok(Json(ActionResponse {
        success: true,
        message: format!("{path} will be checked again on the next watch scan"),
        path: Some(path),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
    }
}

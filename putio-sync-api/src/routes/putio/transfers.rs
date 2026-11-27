//! Put.io transfer proxy routes
//!
//! Provides REST API endpoints for Put.io transfer operations:
//! - GET  /putio/transfers        - List all transfers
//! - POST /putio/transfers        - Add transfer (multipart for .torrent, JSON for magnet);
//!   records it as added by hand for the Stats page (feature 006)
//! - GET  /putio/transfers/:id    - Get specific transfer stats
//! - DELETE /putio/transfers/:id  - Remove transfer (Put.io cancel)
//! - POST /putio/transfers/:id/retry - Retry a failed transfer
//! - POST /putio/transfers/:id/readd - Cancel a transfer and add its magnet again
//! - POST /putio/transfers/remove - Remove several transfers (one Put.io cancel)
//! - POST /putio/transfers/retry  - Retry several transfers, with a result per id
//! - POST /putio/transfers/clear  - Clear completed, failed or all finished transfers
//! - POST /putio/transfers/preview - The files of a `.torrent`, before it is added
//! - GET  /putio/transfers/:id/files - The transfer's files, for the file picker
//! - PUT  /putio/transfers/:id/choice - Pick which of its files to download (feature 070)
//! - DELETE /putio/transfers/:id/choice - Forget the pick; the folder rules decide again
//!
//! Put.io has no pause or resume for transfers, so there are no start/stop routes:
//! retry restarts a failed one, remove stops and deletes one.

use crate::error::ApiError;
use crate::handlers::transfers;
use crate::handlers::unique_ids;
use crate::models::transfers::{
    AddTransferRequest, CancelTransfersRequest, ChoiceRequest, ChoiceSavedResponse, ChoiceView,
    ClearRequest, ClearResponse, Others, PreviewFile, PreviewRequest, PreviewResponse,
    ReaddResponse, RetryManyResponse, TransferFileResponse, TransferFilesResponse,
    TransferResponse,
};
use crate::models::{ListResponse, SuccessResponse};
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    routing::{delete, get, post, put},
    Json, Router,
};
use base64::Engine;
use putio_sync_db::models::{Origin, TransferOrigin};
use putio_sync_db::Choice;
use putio_sync_download::waiting::code;
use putio_sync_download::{DownloadError, Waiting};
use std::collections::{HashMap, HashSet};
use tracing::warn;

// Re-export AppState from parent
pub use crate::AppState;

/// Create the Put.io transfers router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_transfers))
        .route("/", post(add_transfer))
        .route("/:id", get(get_transfer))
        .route("/:id", delete(remove_transfer))
        .route("/:id/retry", post(retry_transfer))
        .route("/:id/readd", post(readd_transfer))
        .route("/remove", post(remove_transfers))
        .route("/retry", post(retry_transfers))
        .route("/clear", post(clear_transfers))
        .route("/preview", post(preview_torrent))
        .route("/:id/files", get(transfer_files))
        .route("/:id/choice", put(set_choice))
        .route("/:id/choice", delete(clear_choice))
}

/// List all transfers
///
/// GET /putio/transfers
async fn list_transfers(
    State(state): State<AppState>,
) -> Result<Json<ListResponse<TransferResponse>>, ApiError> {
    let mut listed = transfers::list_transfers(&state.client, &state.stall_tracker).await?;
    // Which transfers have their files picked by hand, for the page's marker (feature 070)
    if let Some(db) = state.database.as_ref() {
        match db.choices().all().await {
            Ok(choices) => {
                let pending: HashMap<i64, bool> = choices
                    .into_iter()
                    .map(|c| (c.transfer_id, c.pending))
                    .collect();
                for transfer in &mut listed.items {
                    transfer.choice = pending
                        .get(&transfer.id)
                        .map(|pending| if *pending { "pending" } else { "chosen" }.to_string());
                }
            }
            Err(e) => warn!(error = %e, "Failed to read the file choices"),
        }
    }
    Ok(Json(listed))
}

/// Add a new transfer
///
/// POST /putio/transfers
async fn add_transfer(
    State(state): State<AppState>,
    Json(mut request): Json<AddTransferRequest>,
) -> Result<Json<TransferResponse>, ApiError> {
    // Checked before Put.io is asked, so a bad choice adds nothing
    let choice = match (request.choice.take(), request.choose_first) {
        (Some(choice), _) => Some(choice_of(0, choice)?),
        (None, true) => Some(Choice {
            pending: true,
            ..Choice::default()
        }),
        (None, false) => None,
    };
    if choice.is_some() && state.database.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "File choices need the database".to_string(),
        ));
    }
    let transfer = transfers::add_transfer(&state.client, request).await?;
    if let Some(choice) = choice {
        // Saved at once: the transfer's files must not be queued before the choice exists
        let choice = Choice {
            transfer_id: transfer.id,
            ..choice
        };
        if let Err(e) = state.download_manager.set_choice(choice).await {
            warn!(transfer_id = transfer.id, error = %e, "Failed to save the file choice of a new transfer");
            return Err(ApiError::InternalError(format!(
                "The transfer was added, but its file choice was not saved: {e}"
            )));
        }
    }
    if let Some(db) = state.database.as_ref() {
        let origin = TransferOrigin {
            origin: Origin::Manual,
            watch_folder: None,
        };
        if let Err(e) = db
            .sources()
            .record_transfer_origin(transfer.id, &origin)
            .await
        {
            warn!(transfer_id = transfer.id, error = %e, "Failed to record transfer origin");
        }
    }
    Ok(Json(transfer))
}

/// Get transfer details
///
/// GET /putio/transfers/:id
async fn get_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<TransferResponse>, ApiError> {
    transfers::get_transfer(&state.client, &state.stall_tracker, transfer_id)
        .await
        .map(Json)
}

/// Remove a transfer
///
/// DELETE /putio/transfers/:id
async fn remove_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<SuccessResponse>, ApiError> {
    transfers::remove_transfer(&state.client, transfer_id)
        .await
        .map(Json)
}

/// Retry a failed transfer; answers with the transfer as Put.io reports it afterwards
///
/// POST /putio/transfers/:id/retry
async fn retry_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<TransferResponse>, ApiError> {
    transfers::retry_transfer(&state.client, transfer_id)
        .await
        .map(Json)
}

/// Cancel a transfer and add its magnet again to the same folder (feature 007)
///
/// POST /putio/transfers/:id/readd
async fn readd_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<ReaddResponse>, ApiError> {
    transfers::readd_transfer(&state.client, transfer_id)
        .await
        .map(Json)
}

/// The ids of a bulk request body; a bad body or an empty list is 400
fn bulk_ids(
    request: Result<Json<CancelTransfersRequest>, JsonRejection>,
) -> Result<Vec<i64>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    unique_ids(request.transfer_ids)
}

/// Remove several transfers with one Put.io cancel (feature 008)
///
/// POST /putio/transfers/remove `{"transfer_ids": [1, 2]}`
async fn remove_transfers(
    State(state): State<AppState>,
    request: Result<Json<CancelTransfersRequest>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let ids = bulk_ids(request)?;
    transfers::remove_transfers(&state.client, ids)
        .await
        .map(Json)
}

/// Retry several transfers; answers which Put.io accepted and which it refused (feature 008)
///
/// POST /putio/transfers/retry `{"transfer_ids": [1, 2]}`
async fn retry_transfers(
    State(state): State<AppState>,
    request: Result<Json<CancelTransfersRequest>, JsonRejection>,
) -> Result<Json<RetryManyResponse>, ApiError> {
    let ids = bulk_ids(request)?;
    transfers::retry_transfers(&state.client, ids)
        .await
        .map(Json)
}

/// Clear the transfers in a status group (feature 008)
///
/// POST /putio/transfers/clear `{"status": "completed", "dry_run": true}`
async fn clear_transfers(
    State(state): State<AppState>,
    request: Result<Json<ClearRequest>, JsonRejection>,
) -> Result<Json<ClearResponse>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    transfers::clear_transfers(&state.client, request)
        .await
        .map(Json)
}

/// Most files one choice may name
const MAX_CHOICE_FILES: usize = 20_000;
/// Longest path one choice may name
const MAX_CHOICE_PATH: usize = 2048;

/// A request body as a stored choice; a bad path is 400
fn choice_of(transfer_id: i64, request: ChoiceRequest) -> Result<Choice, ApiError> {
    let pending = request.pending;
    if pending && (request.others != Others::Skip || !request.files.is_empty()) {
        return Err(ApiError::BadRequest(
            "A pending choice names no files and skips the others".to_string(),
        ));
    }
    if request.files.len() > MAX_CHOICE_FILES {
        return Err(ApiError::BadRequest(format!(
            "A choice names at most {MAX_CHOICE_FILES} files"
        )));
    }
    let mut files = HashMap::new();
    for file in request.files {
        if file.path.is_empty() || file.path.len() > MAX_CHOICE_PATH {
            return Err(ApiError::BadRequest(
                "A file path is empty or too long".to_string(),
            ));
        }
        files.insert(file.path, file.wanted);
    }
    Ok(Choice {
        transfer_id,
        root_file_id: None,
        others_wanted: request.others == Others::Download,
        pending,
        files,
    })
}

fn download_error(e: DownloadError) -> ApiError {
    match e {
        DownloadError::ClientError(e) => ApiError::Client(e),
        other => ApiError::InternalError(other.to_string()),
    }
}

/// The files of a `.torrent` before it is added, so they can be picked (feature 070).
/// Nothing is sent to Put.io.
///
/// POST /putio/transfers/preview `{"file_data": "<base64>"}`
async fn preview_torrent(
    request: Result<Json<PreviewRequest>, JsonRejection>,
) -> Result<Json<PreviewResponse>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&request.file_data)
        .map_err(|e| ApiError::BadRequest(format!("Invalid base64 file data: {e}")))?;
    let info = putio_sync_precheck::metainfo::parse(&bytes)
        .ok_or_else(|| ApiError::Unprocessable("This is not a torrent file".to_string()))?;
    let listed: Vec<(String, u64)> = info.files.into_iter().map(|f| (f.path, f.size)).collect();
    let files = putio_sync_download::choice::torrent_paths(&listed)
        .into_iter()
        .map(|(path, size)| PreviewFile { path, size })
        .collect();
    Ok(Json(PreviewResponse {
        name: info.name,
        files,
    }))
}

/// The files of a transfer with what its choice says of each (feature 070)
///
/// GET /putio/transfers/:id/files
async fn transfer_files(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<TransferFilesResponse>, ApiError> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable("File choices need the database".to_string())
    })?;
    let listed = state
        .download_manager
        .transfer_files(transfer_id)
        .await
        .map_err(download_error)?;
    let internal = |e: putio_sync_db::DbError| ApiError::InternalError(e.to_string());
    let choice = db.choices().get(transfer_id).await.map_err(internal)?;
    let queue: HashMap<i64, String> = db
        .queue()
        .statuses()
        .await
        .map_err(internal)?
        .into_iter()
        .collect();
    let done: HashSet<i64> = db
        .downloads()
        .completed()
        .await
        .map_err(internal)?
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    let files = listed
        .files
        .into_iter()
        .map(|file| {
            let wanted = choice.as_ref().is_none_or(|c| c.wants(&file.path));
            let state = match queue.get(&file.file_id) {
                Some(status) if status != "completed" => status.clone(),
                Some(_) => "done".to_string(),
                None if done.contains(&file.file_id) => "done".to_string(),
                None => "none".to_string(),
            };
            TransferFileResponse {
                waiting: (!wanted && state == "none").then(|| Waiting::new(code::NOT_CHOSEN)),
                file_id: file.file_id,
                path: file.path,
                size: file.size,
                wanted,
                state,
            }
        })
        .collect();
    Ok(Json(TransferFilesResponse {
        transfer_id,
        name: listed.name,
        ready: listed.root_file_id.is_some(),
        choice: choice.map(|c| ChoiceView {
            others: if c.others_wanted {
                Others::Download
            } else {
                Others::Skip
            },
            pending: c.pending,
        }),
        files,
    }))
}

/// Pick which files of a transfer to download (feature 070). The choice is for this
/// transfer only and wins over the folder rules and filters for it.
///
/// PUT /putio/transfers/:id/choice `{"others": "skip", "files": [{"path": "a.mkv", "wanted": true}]}`
async fn set_choice(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
    request: Result<Json<ChoiceRequest>, JsonRejection>,
) -> Result<Json<ChoiceSavedResponse>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    if state.database.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "File choices need the database".to_string(),
        ));
    }
    let choice = choice_of(transfer_id, request)?;
    let removed = state
        .download_manager
        .set_choice(choice)
        .await
        .map_err(download_error)?;
    Ok(Json(ChoiceSavedResponse {
        message: "Saved your choice of files".to_string(),
        removed,
    }))
}

/// Forget the file choice of a transfer; 404 when it has none
///
/// DELETE /putio/transfers/:id/choice
async fn clear_choice(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<SuccessResponse>, ApiError> {
    if state.database.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "File choices need the database".to_string(),
        ));
    }
    let had = state
        .download_manager
        .clear_choice(transfer_id)
        .await
        .map_err(download_error)?;
    if !had {
        return Err(ApiError::NotFound(
            "This transfer has no file choice".to_string(),
        ));
    }
    Ok(Json(SuccessResponse {
        message: "The folder rules decide for this transfer again".to_string(),
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

//! What's new routes (feature 017)
//!
//! - GET /whats-new - The built-in changelog, each entry marked new or seen
//! - POST /whats-new/seen - Record entries as seen `{"ids": [...]}`
//!
//! The first GET of an install (no seen entries yet) records every entry as seen, so a new
//! install, or the first build with this feature, shows nothing as new.

use crate::changelog::{self, Release};
use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub use crate::AppState;

/// Most ids one `POST /whats-new/seen` takes
const MAX_IDS: usize = 1000;

/// Create the What's new router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(whats_new))
        .route("/seen", post(mark_seen))
}

#[derive(Serialize)]
struct WhatsNew {
    version: &'static str,
    commit: Option<&'static str>,
    /// Distinct new ids
    new_count: usize,
    releases: Vec<ReleaseView>,
}

#[derive(Serialize)]
struct ReleaseView {
    name: &'static str,
    date: Option<&'static str>,
    kinds: Vec<KindView>,
}

#[derive(Serialize)]
struct KindView {
    name: &'static str,
    entries: Vec<EntryView>,
}

#[derive(Serialize)]
struct EntryView {
    id: &'static str,
    text: &'static str,
    new: bool,
}

#[derive(Deserialize)]
struct SeenRequest {
    ids: Vec<String>,
}

fn all_ids(releases: &'static [Release]) -> impl Iterator<Item = &'static str> {
    releases
        .iter()
        .flat_map(|r| &r.kinds)
        .flat_map(|k| &k.entries)
        .map(|e| e.id.as_str())
}

/// The ids already seen, after recording a first-run baseline. `None` means "treat every
/// entry as seen": no database, or it failed (logged).
async fn seen(state: &AppState, releases: &'static [Release]) -> Option<HashSet<String>> {
    let db = state.database.as_ref()?;
    let repo = db.whats_new();
    let result = async {
        let seen = repo.seen_ids().await?;
        if seen.is_empty() {
            let ids: Vec<String> = all_ids(releases).map(str::to_string).collect();
            repo.mark_seen(&ids).await?;
            return Ok(None);
        }
        Ok(Some(seen))
    }
    .await;
    result.unwrap_or_else(|e: putio_sync_db::DbError| {
        tracing::warn!(error = %e, "Could not read the seen What's new entries");
        None
    })
}

/// GET /whats-new
async fn whats_new(State(state): State<AppState>) -> Json<WhatsNew> {
    let releases = changelog::builtin();
    let seen = seen(&state, releases).await;
    let is_new = |id: &str| seen.as_ref().is_some_and(|s| !s.contains(id));
    let new_count = all_ids(releases)
        .filter(|id| is_new(id))
        .collect::<HashSet<_>>()
        .len();

    Json(WhatsNew {
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("PUTIO_SYNC_GIT_SHA"),
        new_count,
        releases: releases
            .iter()
            .map(|r| ReleaseView {
                name: &r.name,
                date: r.date.as_deref(),
                kinds: r
                    .kinds
                    .iter()
                    .map(|k| KindView {
                        name: &k.name,
                        entries: k
                            .entries
                            .iter()
                            .map(|e| EntryView {
                                id: &e.id,
                                text: &e.text,
                                new: is_new(&e.id),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// POST /whats-new/seen `{"ids": [...]}`; ids not in the built-in changelog are ignored
async fn mark_seen(
    State(state): State<AppState>,
    request: Result<Json<SeenRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(request) = request.map_err(|e| ApiError::BadRequest(e.body_text()))?;
    if request.ids.len() > MAX_IDS {
        return Err(ApiError::BadRequest(format!("at most {MAX_IDS} ids")));
    }
    let Some(db) = state.database.as_ref() else {
        return Ok(StatusCode::NO_CONTENT);
    };
    let known: HashSet<&str> = all_ids(changelog::builtin()).collect();
    let ids: Vec<String> = request
        .ids
        .into_iter()
        .filter(|id| known.contains(id.as_str()))
        .collect();
    db.whats_new().mark_seen(&ids).await.map_err(|e| {
        tracing::warn!(error = %e, "Could not record seen What's new entries");
        ApiError::Internal("could not record the entries as seen".into())
    })?;
    Ok(StatusCode::NO_CONTENT)
}

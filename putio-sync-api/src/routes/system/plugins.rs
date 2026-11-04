//! Lua plugin routes (feature 028)
//!
//! - GET /plugins - The plugins folder and each plugin with its state, permissions, config
//!   fields, queue and last 20 runs (no secret values)
//! - POST /plugins/test - Send one `plugin.test` event to a plugin now
//! - POST /plugins/reload - Read the plugins folder again without a restart

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_plugins::{PluginRun, PluginStatus, TestError};
use serde::{Deserialize, Serialize};

pub use crate::AppState;

/// Create the plugins router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/test", post(test))
        .route("/reload", post(reload))
}

#[derive(Serialize)]
struct PluginList {
    dir: Option<String>,
    plugins: Vec<PluginStatus>,
}

#[derive(Deserialize)]
struct TestRequest {
    name: String,
}

#[derive(Serialize)]
struct Message {
    message: String,
}

/// GET /plugins
async fn list(State(state): State<AppState>) -> Json<PluginList> {
    Json(PluginList {
        dir: state.plugins.dir().map(|d| d.display().to_string()),
        plugins: state.plugins.status(),
    })
}

/// POST /plugins/test `{"name": "..."}`. 200 with the run even when the plugin failed.
async fn test(
    State(state): State<AppState>,
    request: Result<Json<TestRequest>, JsonRejection>,
) -> Result<Json<PluginRun>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    state
        .plugins
        .test(&request.name)
        .await
        .map(Json)
        .map_err(|e| match e {
            TestError::NotFound => {
                ApiError::NotFound(format!("no plugin named {:?}", request.name))
            }
            TestError::Running => ApiError::Conflict(format!(
                "a test of plugin {:?} is still running",
                request.name
            )),
            TestError::Broken(error) => {
                ApiError::Unprocessable(format!("plugin {:?} cannot load: {}", request.name, error))
            }
            TestError::Unavailable => ApiError::Internal(e.to_string()),
        })
}

/// POST /plugins/reload
async fn reload(State(state): State<AppState>) -> Json<Message> {
    let (found, broken) = state.plugins.reload();
    let plural = if found == 1 { "" } else { "s" };
    let message = if broken == 0 {
        format!("Reloaded {} plugin{}", found, plural)
    } else {
        format!("Reloaded {} plugin{} ({} broken)", found, plural, broken)
    };
    Json(Message { message })
}

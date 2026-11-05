//! Lua plugin routes (feature 028)
//!
//! - GET /plugins - The plugins folder and each plugin with its state, permissions, config
//!   fields, queue and last 20 runs (no secret values)
//! - POST /plugins/test - Send one `plugin.test` event to a plugin now
//! - POST /plugins/reload - Read the plugins folder again without a restart
//! - POST /plugins/action - Run a plugin's settings action (031); values it saves are
//!   written to the config like a Settings save

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_plugins::action::ActionResult;
use putio_sync_plugins::{CallError, PluginRun, PluginStatus, TestError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use crate::AppState;

/// Create the plugins router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/test", post(test))
        .route("/reload", post(reload))
        .route("/action", post(action))
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

#[derive(Deserialize)]
struct ActionRequest {
    name: String,
    action: String,
    #[serde(default)]
    input: Value,
    /// Unsaved form values that are not secret, for this call only
    #[serde(default)]
    config: Map<String, Value>,
}

#[derive(Serialize)]
struct ActionResponse {
    #[serde(flatten)]
    result: ActionResult,
    /// Every key the action saved; secrets are named, never shown
    #[serde(skip_serializing_if = "Option::is_none")]
    saved: Option<Vec<String>>,
    /// The saved values that are not secret
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<Map<String, Value>>,
    run: PluginRun,
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

/// POST /plugins/action `{"name", "action", "input", "config"}`
async fn action(
    State(state): State<AppState>,
    request: Result<Json<ActionRequest>, JsonRejection>,
) -> Result<Json<ActionResponse>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let input = if request.input.is_null() {
        Value::Object(Map::new())
    } else {
        request.input
    };
    let name = request.name;
    let answer = state
        .plugins
        .action(&name, &request.action, input, request.config)
        .await
        .map_err(|e| match e {
            CallError::NotFound => ApiError::NotFound(format!("no plugin named {:?}", name)),
            CallError::NoAction => ApiError::NotFound(format!(
                "plugin {:?} has no action {:?}",
                name, request.action
            )),
            CallError::Busy => ApiError::Conflict(format!(
                "a call to plugin {:?} is still running",
                name
            )),
            CallError::Broken(error) => {
                ApiError::Unprocessable(format!("plugin {:?} cannot run: {}", name, error))
            }
            CallError::Failed(run) => ApiError::Unprocessable("the action failed".to_string())
                .with_details(run.error.unwrap_or_else(|| "no answer".to_string())),
            CallError::Invalid(error) => ApiError::Unprocessable(error),
            CallError::Unavailable => ApiError::Internal(e.to_string()),
        })?;

    let (saved, config) = match (&answer.settings, &answer.result.save) {
        (Some(settings), Some(save)) => {
            super::settings::save_plugin(&state, settings.clone()).await?;
            let shown = save
                .config
                .keys()
                .filter_map(|k| settings.config.get(k).map(|v| (k.clone(), v.clone())))
                .collect();
            (Some(save.keys()), Some(shown))
        }
        _ => (None, None),
    };
    Ok(Json(ActionResponse {
        result: answer.result,
        saved,
        config,
        run: answer.run,
    }))
}

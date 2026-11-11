//! Plugin workbench routes (feature 055), under `/plugins/workbench`
//!
//! - GET / - Whether the editor is on, the plugins folder, the starters, a sample per
//!   event type and a sample file for sync hooks
//! - PUT /editor - Turn the editor on or off (`plugin_editor` in the config)
//! - POST /new - Make a plugin from a starter (editor on)
//! - GET /source/:name - A plugin's files
//! - POST /check - Check a manifest and script without writing them
//! - PUT /source/:name - Save a folder Lua plugin and reload it alone (editor on)
//! - POST /copy/:name - Copy a bundled plugin to a folder to edit it (editor on)
//! - POST /run - Call one plugin function and show what it did; a dry run by default
//! - GET /runs - The last 20 runs, newest first
//! - POST /runs/:id - Run one of them again

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    routing::{get, post, put},
    Json, Router,
};
use putio_sync_core::events::EventType;
use putio_sync_plugins::discover::Source;
use putio_sync_plugins::source::{self, PluginSource, Problem, SaveRequest, SourceError, Starter};
use putio_sync_plugins::workbench::{self, Run, RunRequest, WorkbenchError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::AppState;

/// Create the workbench router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(overview))
        .route("/editor", put(editor))
        .route("/new", post(new))
        .route("/check", post(check))
        .route("/source/:name", get(read).put(save))
        .route("/copy/:name", post(copy))
        .route("/run", post(run))
        .route("/runs", get(runs))
        .route("/runs/:id", post(rerun))
}

#[derive(Serialize)]
struct EventSample {
    #[serde(rename = "type")]
    event_type: String,
    data: Value,
}

#[derive(Serialize)]
struct Overview {
    editor: bool,
    dir: Option<String>,
    starters: Vec<Starter>,
    events: Vec<EventSample>,
    file: Value,
}

#[derive(Deserialize)]
struct EditorRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct NewRequest {
    starter: String,
    name: String,
}

#[derive(Deserialize)]
struct CheckRequest {
    manifest: String,
    script: String,
}

#[derive(Serialize)]
struct Message {
    message: String,
}

#[derive(Serialize)]
struct Created {
    name: String,
    message: String,
}

#[derive(Serialize)]
struct Checked {
    problems: Vec<Problem>,
}

#[derive(Serialize)]
struct Saved {
    problems: Vec<Problem>,
    message: String,
}

#[derive(Serialize)]
struct Runs {
    runs: Vec<Run>,
}

fn body<T>(request: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    request
        .map(|Json(t)| t)
        .map_err(|rejection| ApiError::BadRequest(rejection.body_text()))
}

async fn editor_on(state: &AppState) -> bool {
    state.config.read().await.plugin_editor
}

async fn need_editor(state: &AppState) -> Result<(), ApiError> {
    if editor_on(state).await {
        return Ok(());
    }
    Err(ApiError::Forbidden(
        "the plugin editor is off; turn it on in the plugin workbench".to_string(),
    ))
}

fn source_error(e: SourceError) -> ApiError {
    let text = e.to_string();
    match e {
        SourceError::NotFound => ApiError::NotFound(text),
        SourceError::BadName(_) => ApiError::BadRequest(text),
        SourceError::Taken(_) | SourceError::Stale(_) => ApiError::Conflict(text),
        SourceError::NotEditable(_) => ApiError::Forbidden(text),
        SourceError::NoFolder | SourceError::Io(_) => ApiError::Unprocessable(text),
    }
}

fn run_error(e: WorkbenchError) -> ApiError {
    let text = e.to_string();
    match e {
        WorkbenchError::NotFound => ApiError::NotFound(text),
        WorkbenchError::Busy => ApiError::Conflict(text),
        WorkbenchError::Invalid(_) => ApiError::BadRequest(text),
        WorkbenchError::Unavailable => ApiError::Internal(text),
    }
}

/// GET /plugins/workbench
async fn overview(State(state): State<AppState>) -> Json<Overview> {
    let events = workbench::event_samples()
        .into_iter()
        .filter(|(t, _)| t.parse::<EventType>().is_ok_and(|t| !t.is_test()))
        .map(|(event_type, data)| EventSample { event_type, data })
        .collect();
    Json(Overview {
        editor: editor_on(&state).await,
        dir: state.plugins.dir().map(|d| d.display().to_string()),
        starters: source::starters(),
        events,
        file: workbench::sample_file(),
    })
}

/// PUT /plugins/workbench/editor `{"enabled": true}`
async fn editor(
    State(state): State<AppState>,
    request: Result<Json<EditorRequest>, JsonRejection>,
) -> Result<Json<Message>, ApiError> {
    let enabled = body(request)?.enabled;
    super::settings::update(
        &state,
        |c| c.plugin_editor,
        |config| {
            config.plugin_editor = enabled;
            Ok(())
        },
    )
    .await?;
    let message = if enabled {
        "The plugin editor is on"
    } else {
        "The plugin editor is off"
    };
    Ok(Json(Message {
        message: message.to_string(),
    }))
}

/// POST /plugins/workbench/new `{"starter", "name"}`
async fn new(
    State(state): State<AppState>,
    request: Result<Json<NewRequest>, JsonRejection>,
) -> Result<Json<Created>, ApiError> {
    need_editor(&state).await?;
    let request = body(request)?;
    let plugins = state.plugins.clone();
    let name = request.name.clone();
    tokio::task::spawn_blocking(move || plugins.workbench_new(&request.starter, &request.name))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(source_error)?;
    Ok(Json(Created {
        message: format!("Made {}; enable it to run it", name),
        name,
    }))
}

/// GET /plugins/workbench/source/:name
async fn read(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<PluginSource>, ApiError> {
    let editor = editor_on(&state).await;
    let plugins = state.plugins.clone();
    let wanted = name.clone();
    tokio::task::spawn_blocking(move || plugins.workbench_source(&wanted, editor))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no plugin named {:?}", name)))
}

/// POST /plugins/workbench/check `{"manifest", "script"}`
async fn check(
    request: Result<Json<CheckRequest>, JsonRejection>,
) -> Result<Json<Checked>, ApiError> {
    let request = body(request)?;
    let problems = tokio::task::spawn_blocking(move || {
        source::check(&request.manifest, &request.script, &Source::Bundled).1
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(Checked { problems }))
}

/// PUT /plugins/workbench/source/:name `{"manifest", "script", "versions", "force"}`
async fn save(
    State(state): State<AppState>,
    Path(name): Path<String>,
    request: Result<Json<SaveRequest>, JsonRejection>,
) -> Result<Json<Saved>, ApiError> {
    need_editor(&state).await?;
    let request = body(request)?;
    let plugins = state.plugins.clone();
    let wanted = name.clone();
    let problems = tokio::task::spawn_blocking(move || plugins.workbench_save(&wanted, &request))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(source_error)?;
    let message = if problems.is_empty() {
        format!("Saved {} and reloaded it", name)
    } else {
        format!(
            "Saved {}; it cannot load until the problems are fixed",
            name
        )
    };
    Ok(Json(Saved { problems, message }))
}

/// POST /plugins/workbench/copy/:name
async fn copy(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Message>, ApiError> {
    need_editor(&state).await?;
    let plugins = state.plugins.clone();
    let wanted = name.clone();
    tokio::task::spawn_blocking(move || plugins.workbench_copy(&wanted))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(source_error)?;
    Ok(Json(Message {
        message: format!(
            "Copied {} to the plugins folder; the copy replaces it",
            name
        ),
    }))
}

/// POST /plugins/workbench/run: 200 with the run even when the plugin failed
async fn run(
    State(state): State<AppState>,
    request: Result<Json<RunRequest>, JsonRejection>,
) -> Result<Json<Run>, ApiError> {
    let request = body(request)?;
    // Running unsaved text is editing: it needs the editor on
    if request.manifest.is_some() || request.script.is_some() {
        need_editor(&state).await?;
    }
    state
        .plugins
        .workbench_run(request)
        .await
        .map(Json)
        .map_err(run_error)
}

/// GET /plugins/workbench/runs
async fn runs(State(state): State<AppState>) -> Json<Runs> {
    Json(Runs {
        runs: state.plugins.workbench_runs(),
    })
}

/// POST /plugins/workbench/runs/:id
async fn rerun(State(state): State<AppState>, Path(id): Path<u64>) -> Result<Json<Run>, ApiError> {
    let unsaved = state
        .plugins
        .workbench_runs()
        .iter()
        .any(|r| r.id == id && r.unsaved);
    if unsaved {
        need_editor(&state).await?;
    }
    state
        .plugins
        .workbench_rerun(id)
        .await
        .map(Json)
        .map_err(run_error)
}

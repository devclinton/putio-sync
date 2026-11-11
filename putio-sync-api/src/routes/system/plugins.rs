//! Plugin routes (features 028, 029)
//!
//! - GET /plugins - The plugins folder and each plugin with its state, permissions, config
//!   fields, queue and last 20 runs (no secret values)
//! - POST /plugins/test - Send one `plugin.test` event to a plugin now
//! - POST /plugins/reload - Read the plugins folder again without a restart
//! - POST /plugins/action - Run a plugin's settings action (031); values it saves are
//!   written to the config like a Settings save
//! - POST /plugins/install - Add a plugin to the plugins folder from a URL or uploaded files
//!   (029); it starts disabled

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, State},
    routing::{get, post},
    Json, Router,
};
use base64::Engine;
use futures_util::StreamExt;
use putio_sync_plugins::action::ActionResult;
use putio_sync_plugins::discover::MAX_WASM_BYTES;
use putio_sync_plugins::install::{InstallError, Package};
use putio_sync_plugins::manifest::Manifest;
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
        .route(
            "/install",
            post(install).layer(DefaultBodyLimit::max(INSTALL_BODY_BYTES)),
        )
}

/// A base64 module at the size limit, with room for the JSON around it
const INSTALL_BODY_BYTES: usize = (MAX_WASM_BYTES as usize) / 3 * 4 + 64 * 1024;
/// How long a download may take
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Redirects followed, on the same host only
const MAX_REDIRECTS: usize = 3;

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

/// One of: `url` (a `.wasm` with its manifest inside, or a `plugin.toml` whose entry sits
/// next to it), `wasm` (a module, base64), or `manifest` with `entry` (the `plugin.toml`
/// text and its entry file, base64)
#[derive(Deserialize)]
struct InstallRequest {
    url: Option<String>,
    wasm: Option<String>,
    manifest: Option<String>,
    entry: Option<String>,
    #[serde(default)]
    replace: bool,
}

#[derive(Serialize)]
struct Installed {
    name: String,
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
            CallError::Busy => {
                ApiError::Conflict(format!("a call to plugin {:?} is still running", name))
            }
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

/// POST /plugins/install. 200 with the name, 400 for a bad request, 409 when the name is
/// taken and `replace` is not set, 422 when the package is not a plugin or the download
/// fails.
async fn install(
    State(state): State<AppState>,
    request: Result<Json<InstallRequest>, JsonRejection>,
) -> Result<Json<Installed>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let decode = |field: &str, text: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .map_err(|e| ApiError::BadRequest(format!("{}: not base64 ({})", field, e)))
    };
    let package = match (
        &request.url,
        &request.wasm,
        &request.manifest,
        &request.entry,
    ) {
        (Some(url), None, None, None) => fetch(url).await?,
        (None, Some(wasm), None, None) => Package::Wasm(decode("wasm", wasm)?),
        (None, None, Some(manifest), Some(entry)) => Package::Files {
            manifest: manifest.clone(),
            entry: decode("entry", entry)?,
        },
        _ => {
            return Err(ApiError::BadRequest(
                "send one of: url, wasm, or manifest with entry".to_string(),
            ))
        }
    };
    let plugins = state.plugins.clone();
    let replace = request.replace;
    let name = tokio::task::spawn_blocking(move || plugins.install(package, replace))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(|e| match e {
            InstallError::NoFolder => ApiError::Unprocessable(e.to_string()),
            InstallError::Invalid(_) => ApiError::Unprocessable(e.to_string()),
            InstallError::Exists(_) => ApiError::Conflict(e.to_string()),
            InstallError::Io(_) => ApiError::Internal(e.to_string()),
        })?;
    Ok(Json(Installed {
        message: format!("Installed {}; enable it to run it", name),
        name,
    }))
}

/// The plugin at `url`: a module, or a manifest and the entry it names in the same folder
async fn fetch(url: &str) -> Result<Package, ApiError> {
    let bytes = download(url).await?;
    if bytes.starts_with(putio_sync_plugins::wasm::MAGIC) {
        return Ok(Package::Wasm(bytes));
    }
    let invalid = |e: String| ApiError::Unprocessable(format!("{}: {}", url, e));
    let manifest = String::from_utf8(bytes)
        .map_err(|_| invalid("neither a WebAssembly module nor a plugin.toml".to_string()))?;
    let parsed = Manifest::parse(&manifest, "plugin.toml").map_err(invalid)?;
    // The entry is a plain file name (the manifest checks it), so this stays in the folder
    let entry_url = reqwest::Url::parse(url)
        .and_then(|base| base.join(&parsed.entry))
        .map_err(|e| invalid(e.to_string()))?;
    let entry = download(entry_url.as_str()).await?;
    Ok(Package::Files { manifest, entry })
}

/// The bytes at `url`: http or https, same-host redirects only, within the size and time
/// limits
async fn download(url: &str) -> Result<Vec<u8>, ApiError> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| ApiError::BadRequest(format!("url: {}", e)))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(ApiError::BadRequest("url: only http and https".to_string()));
    }
    let host = parsed.host_str().map(str::to_string);
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if attempt.url().host_str().map(str::to_string) != host {
                attempt.error("redirected to another host")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let failed = |e: String| ApiError::Unprocessable(format!("cannot download {}: {}", url, e));
    let response = client
        .get(parsed.clone())
        .send()
        .await
        .map_err(|e| failed(without_url(e)))?;
    if !response.status().is_success() {
        return Err(failed(format!("HTTP {}", response.status().as_u16())));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| failed(without_url(e)))?;
        if (bytes.len() + chunk.len()) as u64 > MAX_WASM_BYTES {
            return Err(failed(format!("larger than {} KiB", MAX_WASM_BYTES / 1024)));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// A request error's text without the URL, which the message already names
fn without_url(e: reqwest::Error) -> String {
    let source = std::error::Error::source(&e).map(|s| s.to_string());
    let text = e.without_url().to_string();
    match source {
        Some(source) if !text.contains(&source) => format!("{}: {}", text, source),
        _ => text,
    }
}

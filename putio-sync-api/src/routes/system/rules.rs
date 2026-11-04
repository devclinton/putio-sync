//! Folder rule routes (feature 009)
//!
//! - GET    /rules                   - Rules, presets and the global defaults
//! - GET    /rules/folder/{id}       - A Put.io folder's own rule and its effective rule
//! - PUT    /rules/folder/{id}       - Set (replace) a folder's rule; an empty body clears it
//! - DELETE /rules/folder/{id}       - Remove a folder's rule
//! - POST   /rules/delete            - Remove a rule by path (`{path}`)
//! - PUT    /rules/presets/{name}    - Create a user preset or override a built-in
//! - DELETE /rules/presets/{name}    - Delete a user preset or reset a built-in
//! - POST   /rules/preview           - What a rule (or the account-wide patterns) would do
//!                                     under a folder; changes nothing
//! - GET    /rules/decisions         - Recent blocked, skipped and deleted files
//! - POST   /rules/decisions/clear   - Forget the recorded decisions
//!
//! Every save goes through `settings::update` (validated, written with the file's own
//! credentials) and hands the new rules to the download manager.

use crate::error::ApiError;
use crate::models::SuccessResponse;
use crate::routes::system::settings::{body, invalid, update};
use crate::Engine;
use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use putio_sync_core::config::{Config, ExistingFile};
use putio_sync_core::rules::{
    self, normalize_extension, normalize_path, Decision, Effective, FolderRule, Preset,
    BUILTIN_PRESETS,
};
use putio_sync_download::{DownloadError, PreviewItem};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub use crate::AppState;

/// Create the rules router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_rules))
        .route(
            "/folder/:id",
            get(get_folder).put(put_folder).delete(delete_folder),
        )
        .route("/delete", post(delete_by_path))
        .route("/presets/:name", put(put_preset).delete(delete_preset))
        .route("/preview", post(preview))
        .route("/decisions", get(get_decisions))
        .route("/decisions/clear", post(clear_decisions))
}

/// A rule without its path, as the folder routes take and give it
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleBody {
    pub preset: Option<String>,
    pub allow: Vec<String>,
    pub block: Vec<String>,
    /// Only download (feature 020): `None` inherits, `""` clears the inherited pattern
    pub include: Option<String>,
    /// Never download (feature 020): `None` inherits, `""` clears the inherited pattern
    pub exclude: Option<String>,
    pub skip: Option<bool>,
    pub delete_blocked: Option<bool>,
    pub delete_after_download: Option<bool>,
    /// `None` = use the global setting (feature 040)
    pub on_existing: Option<ExistingFile>,
    /// Replaced by `on_existing`; still read from an old UI tab, never sent
    #[serde(skip_serializing)]
    pub skip_existing: Option<bool>,
}

impl RuleBody {
    fn from_rule(rule: &FolderRule) -> Self {
        Self {
            preset: rule.preset.clone(),
            allow: rule.allow.clone(),
            block: rule.block.clone(),
            include: rule.include.clone(),
            exclude: rule.exclude.clone(),
            skip: rule.skip,
            delete_blocked: rule.delete_blocked,
            delete_after_download: rule.delete_after_download,
            on_existing: rule.existing(),
            skip_existing: None,
        }
    }

    /// The rule for `path`, extensions normalised and deduplicated, patterns trimmed and
    /// checked; an empty preset name means none
    fn into_rule(self, path: &str) -> Result<FolderRule, ApiError> {
        let pattern = |text: Option<String>, what: &str| {
            let text = text.map(|t| t.trim().to_string());
            rules::check_pattern(text.as_deref(), what).map_err(invalid)?;
            Ok::<_, ApiError>(text)
        };
        Ok(FolderRule {
            path: path.to_string(),
            preset: self.preset.filter(|p| !p.trim().is_empty()),
            allow: extensions(&self.allow)?,
            block: extensions(&self.block)?,
            include: pattern(self.include, "Only download")?,
            exclude: pattern(self.exclude, "Never download")?,
            skip: self.skip,
            delete_blocked: self.delete_blocked,
            delete_after_download: self.delete_after_download,
            on_existing: self.on_existing.or(self.skip_existing.map(Into::into)),
            skip_existing: None,
        })
    }
}

/// Normalise and deduplicate extensions, keeping their order
fn extensions(list: &[String]) -> Result<Vec<String>, ApiError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for ext in list {
        let ext = normalize_extension(ext).map_err(invalid)?;
        if seen.insert(ext.clone()) {
            out.push(ext);
        }
    }
    Ok(out)
}

/// One part of the effective rule and the rule path it came from (`None` = global)
#[derive(Debug, Serialize)]
pub struct SourcedView<T> {
    pub value: T,
    pub from: Option<String>,
}

/// The type filter in force, with where it came from
#[derive(Debug, Serialize)]
pub struct FilterView {
    pub only: Option<Vec<String>>,
    pub blocked: Vec<String>,
    pub from: Option<String>,
}

/// The settings in force in a folder
#[derive(Debug, Serialize)]
pub struct EffectiveView {
    pub filter: FilterView,
    pub preset: SourcedView<Option<String>>,
    /// Only download; `""` when no pattern is in force
    pub include: SourcedView<String>,
    /// Never download; `""` when no pattern is in force
    pub exclude: SourcedView<String>,
    pub skip: SourcedView<bool>,
    pub delete_blocked: SourcedView<bool>,
    pub delete_after_download: SourcedView<bool>,
    pub on_existing: SourcedView<ExistingFile>,
}

impl From<Effective> for EffectiveView {
    fn from(e: Effective) -> Self {
        fn part<T>(s: rules::Sourced<T>) -> SourcedView<T> {
            SourcedView {
                value: s.value,
                from: s.from,
            }
        }
        Self {
            filter: FilterView {
                only: e.filter.value.only.map(|set| set.into_iter().collect()),
                blocked: e.filter.value.blocked.into_iter().collect(),
                from: e.filter.from,
            },
            preset: part(e.preset),
            include: SourcedView {
                value: e.include.value.as_str().to_string(),
                from: e.include.from,
            },
            exclude: SourcedView {
                value: e.exclude.value.as_str().to_string(),
                from: e.exclude.from,
            },
            skip: part(e.skip),
            delete_blocked: part(e.delete_blocked),
            delete_after_download: part(e.delete_after_download),
            on_existing: part(e.on_existing),
        }
    }
}

/// A preset as Settings lists it
#[derive(Debug, Serialize)]
pub struct PresetView {
    pub name: String,
    pub label: String,
    pub builtin: bool,
    /// A built-in whose lists `[download.presets]` replaces
    pub overridden: bool,
    pub only: Option<Vec<String>>,
    pub block: Vec<String>,
    pub used_by: Vec<String>,
}

/// The `[download]` settings the root uses
#[derive(Debug, Serialize)]
pub struct GlobalView {
    pub blocked_extensions: Vec<String>,
    /// `[download] whitelist_patterns` (Only download), `""` when unset
    pub include: String,
    /// `[download] blacklist_patterns` (Never download), `""` when unset
    pub exclude: String,
    pub delete_after_download: bool,
    pub on_existing: ExistingFile,
}

/// Body of `GET /rules`
#[derive(Debug, Serialize)]
pub struct RulesResponse {
    pub rules: Vec<FolderRule>,
    pub presets: Vec<PresetView>,
    pub global: GlobalView,
}

fn presets_view(config: &Config) -> Vec<PresetView> {
    let download = &config.download;
    let used_by = |name: &str| {
        rules::rules_using(&download.rules, name)
            .into_iter()
            .map(str::to_string)
            .collect()
    };
    let mut presets: Vec<PresetView> = BUILTIN_PRESETS
        .iter()
        .map(|builtin| {
            let own = download.presets.get(builtin.name);
            let preset = own.cloned().unwrap_or_else(|| builtin.preset());
            PresetView {
                name: builtin.name.to_string(),
                label: builtin.label.to_string(),
                builtin: true,
                overridden: own.is_some(),
                only: preset.only,
                block: preset.block,
                used_by: used_by(builtin.name),
            }
        })
        .collect();
    presets.extend(
        download
            .presets
            .iter()
            .filter(|(name, _)| rules::builtin(name).is_none())
            .map(|(name, preset)| PresetView {
                name: name.clone(),
                label: name.clone(),
                builtin: false,
                overridden: false,
                only: preset.only.clone(),
                block: preset.block.clone(),
                used_by: used_by(name),
            }),
    );
    presets
}

fn rules_view(config: &Config) -> RulesResponse {
    RulesResponse {
        // An old `skip_existing` shows as the `on_existing` it maps to
        rules: config
            .download
            .rules
            .iter()
            .map(|rule| FolderRule {
                on_existing: rule.existing(),
                skip_existing: None,
                ..rule.clone()
            })
            .collect(),
        presets: presets_view(config),
        global: GlobalView {
            blocked_extensions: config.download.blocked_extensions.clone(),
            include: config
                .download
                .whitelist_patterns
                .clone()
                .unwrap_or_default(),
            exclude: config
                .download
                .blacklist_patterns
                .clone()
                .unwrap_or_default(),
            delete_after_download: config.download.delete_after_download,
            on_existing: config.download.on_existing(),
        },
    }
}

/// Rules, presets and the global defaults
///
/// GET /rules
async fn get_rules(State(state): State<AppState>) -> Json<RulesResponse> {
    Json(rules_view(&*state.config.read().await))
}

/// The path of a Put.io folder; 404 when Put.io doesn't know it, 400 for a file
async fn folder_path(state: &AppState, id: i64) -> Result<String, ApiError> {
    state
        .download_manager
        .putio_folder_path(id)
        .await
        .map_err(|e| match e {
            DownloadError::ClientError(ref c) if c.is_not_found() => {
                ApiError::NotFound("Folder not found".to_string())
            }
            DownloadError::InvalidInput(msg) => ApiError::BadRequest(msg),
            other => ApiError::InternalError(format!("Failed to look up the folder: {other}")),
        })
}

/// The folder path a rule applies to, for comparing with a normalised path
fn rule_at<'a>(config: &'a Config, path: &str) -> Option<&'a FolderRule> {
    config
        .download
        .rules
        .iter()
        .find(|r| normalize_path(&r.path).is_ok_and(|p| p == path))
}

/// A folder's own rule and the effective rule
#[derive(Debug, Serialize)]
pub struct FolderResponse {
    pub folder_id: i64,
    pub path: String,
    pub rule: Option<RuleBody>,
    pub effective: EffectiveView,
}

/// GET /rules/folder/{id}
async fn get_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<FolderResponse>, ApiError> {
    let path = folder_path(&state, id).await?;
    let config = state.config.read().await;
    let rule = rule_at(&config, &path).map(RuleBody::from_rule);
    let effective = crate::extract_download_config(&config).effective(&path);
    Ok(Json(FolderResponse {
        folder_id: id,
        path,
        rule,
        effective: effective.into(),
    }))
}

/// Hand the saved rules to the download manager the API holds
async fn apply(state: &AppState) {
    let download_config = {
        let cfg = state.config.read().await;
        crate::extract_download_config(&cfg)
    };
    state.download_manager.update_config(download_config).await;
}

/// When a rule change takes effect, for the engine the API runs on
fn applies(state: &AppState) -> &'static str {
    match state.engine {
        Engine::Daemon => "Applies from the next sync.",
        Engine::Server => {
            "Applies to the API's own downloads; the server --download background thread keeps its startup rules until a restart."
        }
    }
}

/// Replace the rule at `path` (or remove it with `None`); true when one was there
fn set_rule(config: &mut Config, path: &str, rule: Option<FolderRule>) -> bool {
    let rules = &mut config.download.rules;
    let at = rules
        .iter()
        .position(|r| normalize_path(&r.path).is_ok_and(|p| p == path));
    match (at, rule) {
        (Some(i), Some(rule)) => {
            rules[i] = rule;
            true
        }
        (Some(i), None) => {
            rules.remove(i);
            true
        }
        (None, Some(rule)) => {
            rules.push(rule);
            false
        }
        (None, None) => false,
    }
}

fn root_refused() -> ApiError {
    ApiError::BadRequest("The root uses the Download settings".to_string())
}

/// PUT /rules/folder/{id}
async fn put_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    rule: Result<Json<RuleBody>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let rule = body(rule)?;
    if id == 0 {
        return Err(root_refused());
    }
    let path = folder_path(&state, id).await?;
    let rule = rule.into_rule(&path)?;
    let cleared = rule.is_empty();
    update(&state, rules_view, |config| {
        set_rule(config, &path, (!cleared).then_some(rule));
        Ok(())
    })
    .await?;
    apply(&state).await;
    let message = if cleared {
        format!("Rule for {path} removed. {}", applies(&state))
    } else {
        format!("Rule for {path} saved. {}", applies(&state))
    };
    Ok(Json(SuccessResponse { message }))
}

/// DELETE /rules/folder/{id}
async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<SuccessResponse>, ApiError> {
    if id == 0 {
        return Err(root_refused());
    }
    let path = folder_path(&state, id).await?;
    remove_rule(&state, &path).await
}

async fn remove_rule(state: &AppState, path: &str) -> Result<Json<SuccessResponse>, ApiError> {
    if rule_at(&*state.config.read().await, path).is_none() {
        return Ok(Json(SuccessResponse {
            message: format!("No rule for {path}; nothing changed."),
        }));
    }
    update(state, rules_view, |config| {
        set_rule(config, path, None);
        Ok(())
    })
    .await?;
    apply(state).await;
    Ok(Json(SuccessResponse {
        message: format!("Rule for {path} removed. {}", applies(state)),
    }))
}

/// Body of `POST /rules/delete`
#[derive(Debug, Deserialize)]
pub struct DeleteByPath {
    pub path: String,
}

/// Remove a rule by path, for folders that may no longer exist on Put.io
///
/// POST /rules/delete
async fn delete_by_path(
    State(state): State<AppState>,
    request: Result<Json<DeleteByPath>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let request = body(request)?;
    let path = normalize_path(&request.path).map_err(invalid)?;
    if rule_at(&*state.config.read().await, &path).is_none() {
        return Err(ApiError::NotFound(format!("No rule for {path}")));
    }
    remove_rule(&state, &path).await
}

/// PUT /rules/presets/{name}
async fn put_preset(
    State(state): State<AppState>,
    Path(name): Path<String>,
    preset: Result<Json<Preset>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let preset = body(preset)?;
    let preset = Preset {
        only: preset.only.as_deref().map(extensions).transpose()?,
        block: extensions(&preset.block)?,
    };
    rules::validate_preset(&name, &preset).map_err(invalid)?;
    update(&state, rules_view, |config| {
        config.download.presets.insert(name.clone(), preset);
        Ok(())
    })
    .await?;
    apply(&state).await;
    Ok(Json(SuccessResponse {
        message: format!(
            "Preset {} saved. {}",
            rules::preset_label(&name),
            applies(&state)
        ),
    }))
}

/// DELETE /rules/presets/{name}
async fn delete_preset(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let builtin = rules::builtin(&name);
    {
        let config = state.config.read().await;
        if !config.download.presets.contains_key(&name) {
            return match builtin {
                Some(b) => Ok(Json(SuccessResponse {
                    message: format!("{} is already the built-in; nothing changed.", b.label),
                })),
                None => Err(ApiError::NotFound(format!("No preset {name}"))),
            };
        }
        let used = rules::rules_using(&config.download.rules, &name);
        if builtin.is_none() && !used.is_empty() {
            let rules = if used.len() == 1 { "rule" } else { "rules" };
            return Err(ApiError::WithDetails(
                Box::new(ApiError::Conflict(format!(
                    "Preset {name} is used by {} {rules}",
                    used.len()
                ))),
                used.join(", "),
            ));
        }
    }
    update(&state, rules_view, |config| {
        config.download.presets.remove(&name);
        Ok(())
    })
    .await?;
    apply(&state).await;
    let message = match builtin {
        Some(b) => format!("{} reset to the built-in. {}", b.label, applies(&state)),
        None => format!("Preset {name} deleted."),
    };
    Ok(Json(SuccessResponse { message }))
}

/// Body of `POST /rules/preview`
#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    pub folder_id: i64,
    /// Replaces the folder's own rule; omitted = the saved rules
    pub rule: Option<RuleBody>,
    /// Replaces the account-wide Only / Never download patterns (feature 020)
    pub global: Option<GlobalPatterns>,
}

/// Candidate `[download] whitelist_patterns` / `blacklist_patterns`; null or `""` = unset
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct GlobalPatterns {
    pub include: Option<String>,
    pub exclude: Option<String>,
}

/// One file a preview would download
#[derive(Debug, Serialize)]
pub struct PreviewDownloadView {
    pub file_id: i64,
    pub name: String,
    pub path: String,
}

/// One blocked or skipped file in a preview
#[derive(Debug, Serialize)]
pub struct PreviewItemView {
    pub file_id: i64,
    pub name: String,
    pub path: String,
    pub decision: &'static str,
    pub reason: String,
    pub rule: Option<String>,
    pub would_delete: bool,
}

impl From<PreviewItem> for PreviewItemView {
    fn from(item: PreviewItem) -> Self {
        let decision = match item.decision {
            Decision::Skipped { .. } => "skipped",
            _ => "blocked",
        };
        Self {
            file_id: item.file_id,
            name: item.name,
            path: item.path,
            decision,
            reason: item.decision.reason(),
            rule: item.decision.rule().map(str::to_string),
            would_delete: item.would_delete,
        }
    }
}

/// What the rules would do under a folder
#[derive(Debug, Serialize)]
pub struct PreviewResponse {
    pub path: String,
    pub files: usize,
    pub download: usize,
    pub blocked: usize,
    pub skipped: usize,
    pub would_delete: usize,
    pub items: Vec<PreviewItemView>,
    pub truncated: bool,
    /// The first files that would be downloaded
    pub downloads: Vec<PreviewDownloadView>,
}

/// Dry run: list the folder's tree and decide each file with the candidate rule.
/// Nothing is saved, deleted or recorded.
///
/// POST /rules/preview
async fn preview(
    State(state): State<AppState>,
    request: Result<Json<PreviewRequest>, JsonRejection>,
) -> Result<Json<PreviewResponse>, ApiError> {
    let request = body(request)?;
    let path = folder_path(&state, request.folder_id).await?;
    let mut config = state.config.read().await.clone();
    let changed = request.rule.is_some() || request.global.is_some();
    if let Some(rule) = request.rule {
        if request.folder_id == 0 {
            return Err(root_refused());
        }
        let rule = rule.into_rule(&path)?;
        let cleared = rule.is_empty();
        set_rule(&mut config, &path, (!cleared).then_some(rule));
    }
    if let Some(global) = request.global {
        let set =
            |text: Option<String>| text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        config.download.whitelist_patterns = set(global.include);
        config.download.blacklist_patterns = set(global.exclude);
    }
    if changed {
        config.validate().map_err(invalid)?;
    }
    let download_config = crate::extract_download_config(&config);
    let preview = state
        .download_manager
        .preview_rules(request.folder_id, &download_config)
        .await
        .map_err(|e| match e {
            DownloadError::ClientError(ref c) if c.is_not_found() => {
                ApiError::NotFound("Folder not found".to_string())
            }
            other => ApiError::InternalError(format!("Failed to preview the rule: {other}")),
        })?;
    Ok(Json(PreviewResponse {
        path,
        files: preview.files,
        download: preview.download,
        blocked: preview.blocked,
        skipped: preview.skipped,
        would_delete: preview.would_delete,
        items: preview.items.into_iter().map(Into::into).collect(),
        truncated: preview.truncated,
        downloads: preview
            .downloads
            .into_iter()
            .map(|d| PreviewDownloadView {
                file_id: d.file_id,
                name: d.name,
                path: d.path,
            })
            .collect(),
    }))
}

/// `?limit=` of `GET /rules/decisions`
#[derive(Debug, Deserialize)]
pub struct DecisionsQuery {
    pub limit: Option<i64>,
}

/// Body of `GET /rules/decisions`
#[derive(Debug, Serialize)]
pub struct DecisionsResponse {
    pub items: Vec<putio_sync_db::RuleDecisionRecord>,
}

fn database(state: &AppState) -> Result<&putio_sync_db::Database, ApiError> {
    state
        .database
        .as_deref()
        .ok_or_else(|| ApiError::ServiceUnavailable("Database unavailable".to_string()))
}

/// Newest first
///
/// GET /rules/decisions?limit=N
async fn get_decisions(
    State(state): State<AppState>,
    Query(query): Query<DecisionsQuery>,
) -> Result<Json<DecisionsResponse>, ApiError> {
    let limit = query.limit.unwrap_or(100);
    if !(1..=500).contains(&limit) {
        return Err(ApiError::BadRequest(
            "limit must be between 1 and 500".to_string(),
        ));
    }
    let items = database(&state)?
        .rule_decisions()
        .recent(limit)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to read decisions: {e}")))?;
    Ok(Json(DecisionsResponse { items }))
}

/// POST /rules/decisions/clear
async fn clear_decisions(State(state): State<AppState>) -> Result<Json<SuccessResponse>, ApiError> {
    let cleared = database(&state)?
        .rule_decisions()
        .clear()
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to clear decisions: {e}")))?;
    let noun = if cleared == 1 {
        "decision"
    } else {
        "decisions"
    };
    Ok(Json(SuccessResponse {
        message: format!("Cleared {cleared} {noun}"),
    }))
}

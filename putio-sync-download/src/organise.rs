//! Organising files stream mode keeps on Put.io (feature 023).
//!
//! Each newly streamed file gets an `organise` arrived step. It reads the release name
//! (`putio_sync_core::organise::plan_target`) or asks the plugins' folder and rename rules,
//! and records a planned move. With `apply = "preview"` the move waits for the operator on
//! the Put.io page; with `"auto"` the step makes it at once. Every applied move records
//! where the file was, so it can be undone.
//!
//! TV episodes (023) and movies (024) are the two [`OrganiseKind`]s; nothing here depends on
//! which.

use crate::arrived::{ArrivedFile, ArrivedStep, StepResult};
use crate::error::DownloadResult;
use crate::manager::{hook_file, DownloadConfig, DownloadManager};
use futures_util::future::BoxFuture;
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::SyncMode;
use putio_sync_core::organise::{
    join_folder, normalize_folder, plan_target, OrganiseApply, OrganiseConfig, OrganiseKind,
};
use putio_sync_core::rules::Decision;
use putio_sync_core::sync_hooks::Hook;
use putio_sync_db::{Database, MoveStatus, NewMove, OrganiseMove};
use putio_sync_telemetry::{self as telemetry, OrganiseMoveKind, OrganiseResult};
use std::collections::HashMap;
use std::sync::{Arc, RwLock, Weak};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Most ids one apply, undo or dismiss takes
pub const MAX_BATCH: usize = 200;
/// Most moves the list returns
pub const LIST_LIMIT: i64 = 500;
/// Most streamed files one scan reads
const SCAN_LIMIT: i64 = 10_000;

/// What happened to one id of a batch
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MoveOutcome {
    pub file_id: i64,
    /// `applied`, `undone`, `dismissed`, `failed`, `missing` (no such move) or `skipped`
    /// (not in a state the action takes)
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl MoveOutcome {
    fn new(file_id: i64, status: &'static str) -> Self {
        Self {
            file_id,
            status,
            error: None,
        }
    }

    fn failed(file_id: i64, error: String) -> Self {
        Self {
            file_id,
            status: "failed",
            error: Some(error),
        }
    }
}

/// Why a move could not be made
enum Failure {
    /// Put.io refused or the move cannot be made as planned; tried again only by hand
    Refused(String),
    /// Put.io could not be reached or failed; worth another try
    Transient(String),
}

impl Failure {
    fn message(&self) -> &str {
        match self {
            Failure::Refused(m) | Failure::Transient(m) => m,
        }
    }
}

/// Plans, makes and undoes moves on Put.io
pub struct Organiser {
    client: Arc<PutioApiClient>,
    db: Arc<Database>,
    /// The sync's rules (a target must stay in stream mode) and plugins
    manager: Weak<DownloadManager>,
    config: RwLock<OrganiseConfig>,
    /// One apply, undo or scan at a time
    busy: Mutex<()>,
}

impl Organiser {
    pub fn new(
        client: Arc<PutioApiClient>,
        db: Arc<Database>,
        manager: &Arc<DownloadManager>,
        config: OrganiseConfig,
    ) -> Self {
        Self {
            client,
            db,
            manager: Arc::downgrade(manager),
            config: RwLock::new(config),
            busy: Mutex::new(()),
        }
    }

    /// The settings in force
    pub fn config(&self) -> OrganiseConfig {
        self.config.read().unwrap().clone()
    }

    /// Apply new settings; the next sync pass uses them
    pub fn configure(&self, config: OrganiseConfig) {
        *self.config.write().unwrap() = config;
    }

    /// Moves, newest change first
    pub async fn list(&self) -> DownloadResult<Vec<OrganiseMove>> {
        Ok(self.db.organise_moves().list(LIST_LIMIT).await?)
    }

    /// Record a move for `file` when a kind or a plugin places it somewhere else. A file
    /// that already has a move is left alone. The move, when one was recorded.
    pub async fn plan(&self, file: &ArrivedFile) -> DownloadResult<Option<OrganiseMove>> {
        let config = self.config();
        if !config.any() {
            return Ok(None);
        }
        let built_in = plan_target(&config, &file.name, &file.folder);
        let (mut folder, mut name, mut by_plugin) = match &built_in {
            Some(t) => (t.folder.clone(), t.name.clone(), false),
            None => (normalize_folder(&file.folder), file.name.clone(), false),
        };
        if config.use_plugins {
            if let Some((f, n)) = self.ask_plugins(file).await {
                if let Some(f) = f {
                    folder = normalize_folder(&f);
                    by_plugin = true;
                }
                if let Some(n) = n {
                    name = n;
                    by_plugin = true;
                }
            }
        }
        if built_in.is_none() && !by_plugin {
            return Ok(None);
        }
        if folder == normalize_folder(&file.folder) && name == file.name {
            return Ok(None);
        }
        // A move only a plugin placed counts as TV unless only movies are on
        let kind = built_in.map(|t| t.kind).unwrap_or(if config.tv {
            OrganiseKind::Tv
        } else {
            OrganiseKind::Movie
        });
        let repo = self.db.organise_moves();
        let recorded = repo
            .plan(&NewMove {
                file_id: file.file_id,
                kind: kind.as_str().to_string(),
                from_folder: file.folder.clone(),
                from_name: file.name.clone(),
                to_folder: folder.clone(),
                to_name: name.clone(),
                by_plugin,
            })
            .await?;
        if !recorded {
            return Ok(None);
        }
        count(kind, OrganiseResult::Planned);
        info!(file_id = file.file_id, from = %file.name, to = %join_folder(&[&folder, &name]), by_plugin, "Planned a move on Put.io");
        Ok(repo.get(file.file_id).await?)
    }

    /// The plugins' folder and name for `file`, as a stream-mode file
    async fn ask_plugins(&self, file: &ArrivedFile) -> Option<(Option<String>, Option<String>)> {
        let manager = self.manager.upgrade()?;
        let asked = hook_file(
            file.file_id,
            &file.name,
            file.size.max(0) as u64,
            &file.folder,
            &Decision::Stream { rule: None },
        );
        let folder = match manager.hooks_for(Hook::Folder) {
            Some(hooks) => hooks.folder(&asked).await,
            None => None,
        };
        let name = match manager.hooks_for(Hook::Rename) {
            Some(hooks) => hooks.rename(&asked).await,
            None => None,
        };
        (folder.is_some() || name.is_some()).then_some((folder, name))
    }

    /// Plan moves for streamed files recorded before organising was on. Files with a move
    /// in any state are skipped. How many were planned.
    pub async fn scan(&self) -> DownloadResult<usize> {
        let _busy = self.busy.lock().await;
        let known = self.db.organise_moves().ids().await?;
        let mut planned = 0;
        for row in self.db.stream_files().all(SCAN_LIMIT).await? {
            if known.contains(&row.file_id) {
                continue;
            }
            let file = ArrivedFile {
                file_id: row.file_id,
                name: row.filename,
                folder: row.folder_path,
                size: row.size,
            };
            if self.plan(&file).await?.is_some() {
                planned += 1;
            }
        }
        Ok(planned)
    }

    /// Make planned, failed or undone moves
    pub async fn apply(&self, file_ids: &[i64]) -> Vec<MoveOutcome> {
        let _busy = self.busy.lock().await;
        let config = self.download_config().await;
        let mut folders = HashMap::new();
        let mut outcomes = Vec::new();
        for &id in file_ids.iter().take(MAX_BATCH) {
            outcomes.push(match self.db.organise_moves().get(id).await {
                Ok(Some(m)) if m.status == MoveStatus::Applied => MoveOutcome::new(id, "skipped"),
                Ok(Some(m)) => match self.apply_one(&m, config.as_ref(), &mut folders).await {
                    Ok(()) => MoveOutcome::new(id, "applied"),
                    Err(f) => MoveOutcome::failed(id, f.message().to_string()),
                },
                Ok(None) => MoveOutcome::new(id, "missing"),
                Err(e) => MoveOutcome::failed(id, e.to_string()),
            });
        }
        outcomes
    }

    /// Put applied moves back
    pub async fn undo(&self, file_ids: &[i64]) -> Vec<MoveOutcome> {
        let _busy = self.busy.lock().await;
        let mut outcomes = Vec::new();
        for &id in file_ids.iter().take(MAX_BATCH) {
            outcomes.push(match self.db.organise_moves().get(id).await {
                Ok(Some(m)) if m.status == MoveStatus::Applied => match self.undo_one(&m).await {
                    Ok(()) => MoveOutcome::new(id, "undone"),
                    Err(message) => MoveOutcome::failed(id, message),
                },
                Ok(Some(_)) => MoveOutcome::new(id, "skipped"),
                Ok(None) => MoveOutcome::new(id, "missing"),
                Err(e) => MoveOutcome::failed(id, e.to_string()),
            });
        }
        outcomes
    }

    /// Forget moves that are not applied; an applied move is undone first
    pub async fn dismiss(&self, file_ids: &[i64]) -> Vec<MoveOutcome> {
        let _busy = self.busy.lock().await;
        let repo = self.db.organise_moves();
        let mut outcomes = Vec::new();
        for &id in file_ids.iter().take(MAX_BATCH) {
            outcomes.push(match repo.get(id).await {
                Ok(Some(m)) if m.status == MoveStatus::Applied => MoveOutcome::new(id, "skipped"),
                Ok(Some(_)) => match repo.remove(&[id]).await {
                    Ok(_) => MoveOutcome::new(id, "dismissed"),
                    Err(e) => MoveOutcome::failed(id, e.to_string()),
                },
                Ok(None) => MoveOutcome::new(id, "missing"),
                Err(e) => MoveOutcome::failed(id, e.to_string()),
            });
        }
        outcomes
    }

    async fn download_config(&self) -> Option<DownloadConfig> {
        match self.manager.upgrade() {
            Some(manager) => Some(manager.get_config().await),
            None => None,
        }
    }

    /// Rename, then move. A move that fails takes the rename back, so the file is never
    /// left half-organised.
    async fn apply_one(
        &self,
        m: &OrganiseMove,
        config: Option<&DownloadConfig>,
        folders: &mut HashMap<String, i64>,
    ) -> Result<(), Failure> {
        let kind = kind_of(m);
        let result = self.make_move(m, config, folders).await;
        let repo = self.db.organise_moves();
        match &result {
            Ok((name, parent)) => {
                if let Err(e) = repo.applied(m.file_id, name, *parent).await {
                    warn!(file_id = m.file_id, error = %e, "Failed to record an applied move");
                }
                count(kind, OrganiseResult::Applied);
                info!(file_id = m.file_id, to = %join_folder(&[&m.to_folder, &m.to_name]), "Moved on Put.io");
            }
            Err(f) => {
                if let Err(e) = repo.failed(m.file_id, f.message()).await {
                    warn!(file_id = m.file_id, error = %e, "Failed to record a failed move");
                }
                count(kind, OrganiseResult::Failed);
                warn!(file_id = m.file_id, error = %f.message(), "Move on Put.io failed");
            }
        }
        result.map(|_| ())
    }

    /// The name and folder id the file had before
    async fn make_move(
        &self,
        m: &OrganiseMove,
        config: Option<&DownloadConfig>,
        folders: &mut HashMap<String, i64>,
    ) -> Result<(String, i64), Failure> {
        // A file moved out of stream mode would be downloaded by the next sync
        match config {
            Some(c) if c.effective(&m.to_folder).mode.value == SyncMode::Stream => {}
            Some(_) => {
                return Err(Failure::Refused(format!(
                    "\"{}\" is not in stream mode, so the next sync would download the file",
                    display_folder(&m.to_folder)
                )))
            }
            None => return Err(Failure::Transient("the sync is not running".to_string())),
        }
        let file = match self.client.get_file_details(m.file_id).await {
            Ok(details) => details.file,
            Err(e) if e.is_not_found() => {
                return Err(Failure::Refused("no longer on Put.io".to_string()))
            }
            Err(e) => return Err(Failure::Transient(e.to_string())),
        };
        let from_parent = file.parent_id.unwrap_or(0);
        let to_parent = self.ensure_folder(&m.to_folder, folders).await?;
        if from_parent == to_parent && file.name == m.to_name {
            return Ok((file.name, from_parent));
        }
        self.check_free(to_parent, &m.to_name, m.file_id).await?;
        let renamed = file.name != m.to_name;
        if renamed {
            self.client
                .rename_file(m.file_id, &m.to_name)
                .await
                .map_err(|e| Failure::Transient(format!("rename: {e}")))?;
        }
        if from_parent != to_parent {
            if let Err(e) = self.client.move_files(&[m.file_id], to_parent).await {
                if renamed {
                    if let Err(back) = self.client.rename_file(m.file_id, &file.name).await {
                        warn!(file_id = m.file_id, error = %back, "Could not take a rename back");
                    }
                }
                return Err(Failure::Transient(format!("move: {e}")));
            }
        }
        Ok((file.name, from_parent))
    }

    /// Move the file back to its folder and name
    async fn undo_one(&self, m: &OrganiseMove) -> Result<(), String> {
        let kind = kind_of(m);
        let repo = self.db.organise_moves();
        let result = self.unmake_move(m).await;
        match &result {
            Ok(()) => {
                if let Err(e) = repo.undone(m.file_id).await {
                    warn!(file_id = m.file_id, error = %e, "Failed to record an undone move");
                }
                count(kind, OrganiseResult::Undone);
                info!(file_id = m.file_id, to = %join_folder(&[&m.from_folder, &m.from_name]), "Move undone on Put.io");
            }
            Err(message) => {
                // The file stays where the move put it
                if let Err(e) = repo.note(m.file_id, message).await {
                    warn!(file_id = m.file_id, error = %e, "Failed to record an undo error");
                }
                count(kind, OrganiseResult::Failed);
                warn!(file_id = m.file_id, error = %message, "Undo on Put.io failed");
            }
        }
        result
    }

    async fn unmake_move(&self, m: &OrganiseMove) -> Result<(), String> {
        let Some(parent) = m.from_parent_id else {
            return Err("where the file was is not known".to_string());
        };
        if parent != 0 {
            match self.client.get_file_details(parent).await {
                Ok(_) => {}
                Err(e) if e.is_not_found() => {
                    return Err(format!(
                        "the old folder \"{}\" is gone from Put.io",
                        display_folder(&m.from_folder)
                    ))
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        let file = match self.client.get_file_details(m.file_id).await {
            Ok(details) => details.file,
            Err(e) if e.is_not_found() => return Err("no longer on Put.io".to_string()),
            Err(e) => return Err(e.to_string()),
        };
        self.check_free(parent, &m.from_name, m.file_id)
            .await
            .map_err(|f| f.message().to_string())?;
        let moved = file.parent_id.unwrap_or(0) != parent;
        if moved {
            self.client
                .move_files(&[m.file_id], parent)
                .await
                .map_err(|e| format!("move: {e}"))?;
        }
        if file.name != m.from_name {
            if let Err(e) = self.client.rename_file(m.file_id, &m.from_name).await {
                if moved {
                    let to = file.parent_id.unwrap_or(0);
                    if let Err(back) = self.client.move_files(&[m.file_id], to).await {
                        warn!(file_id = m.file_id, error = %back, "Could not take a move back");
                    }
                }
                return Err(format!("rename: {e}"));
            }
        }
        Ok(())
    }

    /// The id of Put.io folder `path` (from the root), made where missing
    async fn ensure_folder(
        &self,
        path: &str,
        cache: &mut HashMap<String, i64>,
    ) -> Result<i64, Failure> {
        let mut parent = 0;
        let mut so_far = String::new();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            so_far = join_folder(&[&so_far, part]);
            if let Some(id) = cache.get(&so_far) {
                parent = *id;
                continue;
            }
            let listing = self
                .client
                .list_files(parent)
                .await
                .map_err(|e| Failure::Transient(format!("list \"{so_far}\": {e}")))?;
            parent = match listing.files.iter().find(|f| f.name == part) {
                Some(f) if f.is_folder => f.id,
                Some(_) => {
                    return Err(Failure::Refused(format!(
                        "\"{so_far}\" on Put.io is a file, not a folder"
                    )))
                }
                None => {
                    debug!(folder = %so_far, "Making folder on Put.io");
                    self.client
                        .create_folder(part, parent)
                        .await
                        .map_err(|e| Failure::Transient(format!("make \"{so_far}\": {e}")))?
                        .id
                }
            };
            cache.insert(so_far.clone(), parent);
        }
        Ok(parent)
    }

    /// Refuse when another file in `parent` already has `name`
    async fn check_free(&self, parent: i64, name: &str, file_id: i64) -> Result<(), Failure> {
        let listing = self
            .client
            .list_files(parent)
            .await
            .map_err(|e| Failure::Transient(e.to_string()))?;
        if listing
            .files
            .iter()
            .any(|f| f.name == name && f.id != file_id)
        {
            return Err(Failure::Refused(format!("\"{name}\" already exists there")));
        }
        Ok(())
    }

    /// The arrived step: plan, and with `apply = "auto"` make the move
    async fn arrived(&self, file: &ArrivedFile) -> StepResult {
        let planned = match self.plan(file).await {
            Ok(planned) => planned,
            Err(e) => return StepResult::Retry(format!("plan: {e}")),
        };
        let Some(m) = planned else {
            return StepResult::Done;
        };
        if self.config().apply != OrganiseApply::Auto {
            return StepResult::Done;
        }
        let _busy = self.busy.lock().await;
        let config = self.download_config().await;
        match self
            .apply_one(&m, config.as_ref(), &mut HashMap::new())
            .await
        {
            Ok(()) | Err(Failure::Refused(_)) => StepResult::Done,
            Err(Failure::Transient(message)) => {
                // Planned again so the retry applies it; the error stays for the page
                if let Err(e) = self.db.organise_moves().remove(&[m.file_id]).await {
                    warn!(file_id = m.file_id, error = %e, "Failed to reset a move for retry");
                }
                StepResult::Retry(message)
            }
        }
    }
}

fn kind_of(m: &OrganiseMove) -> OrganiseKind {
    m.kind.parse().unwrap_or(OrganiseKind::Tv)
}

fn count(kind: OrganiseKind, result: OrganiseResult) {
    let kind = match kind {
        OrganiseKind::Tv => OrganiseMoveKind::Tv,
        OrganiseKind::Movie => OrganiseMoveKind::Movie,
    };
    telemetry::sync::organise_move(kind, result);
}

fn display_folder(folder: &str) -> &str {
    if folder.is_empty() {
        "/"
    } else {
        folder
    }
}

/// The `organise` arrived step (feature 023)
pub struct OrganiseStep {
    organiser: Arc<Organiser>,
}

impl OrganiseStep {
    pub fn new(organiser: Arc<Organiser>) -> Self {
        Self { organiser }
    }
}

impl ArrivedStep for OrganiseStep {
    fn name(&self) -> &'static str {
        "organise"
    }

    fn enabled(&self, _config: &DownloadConfig) -> bool {
        self.organiser.config().any()
    }

    fn run<'a>(&'a self, file: &'a ArrivedFile) -> BoxFuture<'a, StepResult> {
        Box::pin(self.organiser.arrived(file))
    }
}

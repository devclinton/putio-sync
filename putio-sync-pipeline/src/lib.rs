//! # putio-sync-pipeline
//!
//! The processing pipeline (feature 038): the stages an item passes between Put.io and the
//! library, and the steps that run in them.
//!
//! The Active page (039) follows an item until its files are on the NAS. This crate follows
//! it on from there: pre-processing, post-processing and notifications, with the operator's
//! own hooks as the steps. It watches the Active list each tick ([`Pipeline::on_tick`]),
//! keeps one row per item and one per step, and runs due steps in a worker under a
//! concurrency limit. Radarr and Sonarr are held back from an unfinished item through
//! [`PipelineGate`], and a watch-folder item can be staged out of sight until it is done.
//!
//! `arr` and `download` do not depend on this crate: the gate trait lives in core, and the
//! staging redirect in `download`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use putio_sync_core::config::{Config, PipelineConfig};
use putio_sync_core::hooks::HookConfig;
use putio_sync_core::pipeline_gate::PipelineGate;
use putio_sync_db::pipeline::{FinishedOutcome, FinishedRow};
use putio_sync_db::Database;
use putio_sync_download::pipeline::{Origin, Pipeline as ActiveView, PipelineItem, State};
use tokio::sync::{Notify, Semaphore};
use tracing::{debug, info, warn};

pub mod gate;
pub mod locate;
pub mod model;
pub mod plan;
mod runner;
pub mod staging;
pub mod store;
pub mod timeline;

pub use gate::Gate;
pub use model::{ItemState, Stage, StepState};
pub use store::{ItemRow, StepRow, Store};
pub use timeline::{StepSummary, Timeline};

use model::StepKind;
use plan::{plan, PlanInput};
use store::NewItem;

/// How many ticks an item may be missing from Put.io, with no finished row, before it is
/// taken as deleted
const MISSED_TICKS: u32 = 2;

/// Why a retry or skip was refused
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    /// No such item or step
    NotFound,
    /// The step is in a state that does not allow it
    Conflict(String),
}

/// What the pipeline reads from the config
#[derive(Debug, Clone)]
pub(crate) struct Settings {
    pub pipeline: PipelineConfig,
    /// Hooks with a stage, enabled or not (the planner drops disabled ones)
    pub hooks: Vec<HookConfig>,
    pub target: PathBuf,
    pub retention_hours: u64,
}

impl Settings {
    fn from(config: &Config) -> Settings {
        Settings {
            pipeline: config.pipeline.clone(),
            hooks: config
                .hooks
                .iter()
                .filter(|h| h.stage.is_some())
                .cloned()
                .collect(),
            target: config.download.target_directory.clone(),
            retention_hours: config.transfers.finished_retention_hours,
        }
    }
}

pub(crate) struct Inner {
    pub db: Arc<Database>,
    pub store: Store,
    pub settings: RwLock<Settings>,
    pub client: reqwest::Client,
    pub permits: RwLock<Arc<Semaphore>>,
    pub wake: Notify,
    /// Items a worker task is advancing now
    pub busy: Mutex<HashSet<String>>,
    /// Cancels the running step of an item
    pub cancels: Mutex<HashMap<String, Arc<Notify>>>,
    pub gate: Gate,
    /// Ticks an item has been missing without a finished row
    pub missed: Mutex<HashMap<String, u32>>,
    /// The database's clock when the service was built; only later finishes are picked up
    pub started_at: String,
}

/// The processing pipeline service. Cheap to clone.
#[derive(Clone)]
pub struct Pipeline {
    pub(crate) inner: Arc<Inner>,
}

impl Pipeline {
    /// Build the service. Nothing runs until [`Pipeline::startup`] and [`Pipeline::start`].
    pub async fn new(db: Arc<Database>, config: &Config) -> Pipeline {
        let started_at: String = sqlx::query_scalar("SELECT datetime('now')")
            .fetch_one(db.pool())
            .await
            .unwrap_or_default();
        let settings = Settings::from(config);
        let permits = Arc::new(Semaphore::new(settings.pipeline.concurrency.max(1)));
        Pipeline {
            inner: Arc::new(Inner {
                store: Store::new(db.pool().clone()),
                db,
                settings: RwLock::new(settings),
                client: putio_sync_hooks::webhook::client(),
                permits: RwLock::new(permits),
                wake: Notify::new(),
                busy: Mutex::new(HashSet::new()),
                cancels: Mutex::new(HashMap::new()),
                gate: Gate::default(),
                missed: Mutex::new(HashMap::new()),
                started_at,
            }),
        }
    }

    /// Take new settings: the hooks, concurrency and staging mode
    pub fn configure(&self, config: &Config) {
        let settings = Settings::from(config);
        if let Ok(mut permits) = self.inner.permits.write() {
            if settings.pipeline.concurrency != self.concurrency() {
                *permits = Arc::new(Semaphore::new(settings.pipeline.concurrency.max(1)));
            }
        }
        if let Ok(mut s) = self.inner.settings.write() {
            *s = settings;
        }
        self.inner.wake.notify_one();
    }

    fn concurrency(&self) -> usize {
        self.inner
            .settings
            .read()
            .map(|s| s.pipeline.concurrency)
            .unwrap_or(2)
    }

    pub(crate) fn settings(&self) -> Settings {
        self.inner
            .settings
            .read()
            .map(|s| s.clone())
            .expect("settings lock")
    }

    /// The gate for Radarr and Sonarr
    pub fn gate(&self) -> Arc<dyn PipelineGate> {
        Arc::new(self.inner.gate.clone())
    }

    /// The store, for the API
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// At daemon start: steps that were running become interrupted (they run again; done
    /// steps never do), and the gate is filled
    pub async fn startup(&self) {
        match self.inner.store.interrupt_running().await {
            Ok(0) => {}
            Ok(n) => info!(
                steps = n,
                "Pipeline steps interrupted by the last stop will run again"
            ),
            Err(e) => warn!(error = %e, "Could not mark running pipeline steps interrupted"),
        }
        self.refresh_gate().await;
    }

    /// Run the worker: every second, or when woken, advance each item that has work. The
    /// task ends when the returned handle is aborted.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let _ =
                    tokio::time::timeout(Duration::from_secs(1), me.inner.wake.notified()).await;
                me.spawn_due().await;
            }
        })
    }

    /// One worker pass that waits for every step it started. The worker loop does not wait;
    /// this is for tests and for quickstart-style runs.
    pub async fn run_due(&self) {
        for handle in self.spawn_due().await {
            let _ = handle.await;
        }
    }

    /// Follow the Active list (research R1). `view` is the pipeline this tick built; call
    /// it after `record_finished`, so a transfer that just left the list has its row.
    pub async fn on_tick(&self, view: &ActiveView) {
        let settings = self.settings();
        let store = &self.inner.store;
        let known: HashMap<String, ItemRow> = match store.items().await {
            Ok(items) => items.into_iter().map(|i| (i.key.clone(), i)).collect(),
            Err(e) => {
                warn!(error = %e, "Could not read pipeline items");
                return;
            }
        };
        let finished: HashMap<String, FinishedRow> = self
            .inner
            .db
            .finished()
            .list()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.key.clone(), r))
            .collect();
        let present: HashMap<&str, &PipelineItem> =
            view.items.iter().map(|i| (i.key.as_str(), i)).collect();

        // New on the Active list
        for item in &view.items {
            if !known.contains_key(&item.key) {
                self.register_active(item, &settings).await;
            }
        }

        // Known and not finished: still there, or left
        // (a failed download can recover while its item is still listed)
        let recoverable = |r: &&ItemRow| {
            !r.state.is_terminal()
                || (r.state == ItemState::Failed
                    && r.stage == Stage::Downloading
                    && present.contains_key(r.key.as_str()))
        };
        for row in known.values().filter(recoverable) {
            match present.get(row.key.as_str()) {
                Some(item) => {
                    self.inner.missed.lock().unwrap().remove(&row.key);
                    self.follow(row, item).await;
                }
                None => self.resolve_departed(row, finished.get(&row.key)).await,
            }
            self.reflect_hold(row).await;
        }

        // Finished since the service started, and never seen on the list
        for row in finished.values() {
            if !known.contains_key(&row.key)
                && row.outcome == FinishedOutcome::Done
                && row.done_at >= self.inner.started_at
            {
                self.register_finished(row, &settings).await;
            }
        }

        if let Err(e) = store.prune(settings.retention_hours).await {
            warn!(error = %e, "Could not prune pipeline items");
        }
        self.refresh_gate().await;
        self.inner.wake.notify_one();
    }

    /// Start tracking an item that appeared on the Active list
    async fn register_active(&self, item: &PipelineItem, settings: &Settings) {
        let category = match &item.origin {
            Some(Origin::Arr(c)) => Some(c.clone()),
            _ => None,
        };
        let source = match &item.origin {
            Some(Origin::Arr(_)) => "arr",
            Some(Origin::Watch) => "watch",
            _ => "sync",
        };
        let hooks = &settings.hooks;
        let steps = plan(
            PlanInput {
                category: category.as_deref(),
                has_transfer: item.transfer_id.is_some() && item.key.starts_with("transfer:"),
                downloaded: false,
            },
            hooks,
        );
        let has_gate = steps
            .iter()
            .any(|s| s.stage == Stage::PreDownload && s.state == StepState::Waiting);
        // Hold back only a transfer whose files have not started (R2)
        let unstarted = item.stage == putio_sync_download::pipeline::Stage::Putio
            || item
                .files
                .map(|f| f.done == 0 && f.downloading == 0)
                .unwrap_or(true);
        let gated = has_gate && unstarted && item.transfer_id.is_some();
        let new = NewItem {
            key: item.key.clone(),
            name: item.name.clone(),
            category,
            source: source.to_string(),
            dir: String::new(),
            transfer_id: item.transfer_id,
            stage: if gated {
                Stage::PreDownload
            } else {
                Stage::Downloading
            },
            state: ItemState::Running,
        };
        // Steps of a gate-less item: the pre-download hooks that cannot run are skipped
        let steps = if gated {
            steps
        } else {
            steps
                .into_iter()
                .map(|mut s| {
                    if s.stage == Stage::PreDownload && s.state == StepState::Waiting {
                        s.state = StepState::Skipped;
                        s.skip_reason = Some(model::SkipReason::NotConfigured);
                    }
                    s
                })
                .collect()
        };
        match self.inner.store.insert_item(&new, &steps).await {
            Ok(true) => {
                debug!(key = %item.key, gated, "Pipeline item registered");
                if let (true, Some(id)) = (gated, item.transfer_id) {
                    if let Err(e) = self.inner.db.holds().add(id, &item.name).await {
                        warn!(key = %item.key, error = %e, "Could not hold the transfer for its pre-download steps");
                    }
                }
            }
            Ok(false) => {}
            Err(e) => warn!(key = %item.key, error = %e, "Could not register a pipeline item"),
        }
    }

    /// Start tracking an item that finished between two ticks, or in a run of the daemon
    /// before this one: it begins at pre-processing
    async fn register_finished(&self, row: &FinishedRow, settings: &Settings) {
        let transfer_id = row
            .key
            .strip_prefix("transfer:")
            .and_then(|s| s.parse::<i64>().ok());
        let paths = locate::local_paths(&self.inner.db, &row.key, settings.retention_hours).await;
        let loc = locate::derive(
            &row.name,
            &paths.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
            &settings.target,
        );
        let steps = plan(
            PlanInput {
                category: loc.category.as_deref(),
                has_transfer: transfer_id.is_some(),
                downloaded: true,
            },
            &settings.hooks,
        );
        let new = NewItem {
            key: row.key.clone(),
            name: row.name.clone(),
            category: loc.category,
            source: "sync".to_string(),
            dir: loc.dir.to_string_lossy().into_owned(),
            transfer_id,
            stage: Stage::PreProcessing,
            state: ItemState::Running,
        };
        if let Err(e) = self.inner.store.insert_item(&new, &steps).await {
            warn!(key = %row.key, error = %e, "Could not register a finished item");
        }
    }

    /// An item still on the Active list: follow a failed download, and a recovery
    async fn follow(&self, row: &ItemRow, item: &PipelineItem) {
        let store = &self.inner.store;
        let failed = matches!(item.state, State::Failed | State::Error);
        if failed && row.state == ItemState::Running {
            let _ = store
                .set_item(&row.key, Stage::Downloading, ItemState::Failed)
                .await;
        } else if !failed && row.state == ItemState::Failed && row.stage == Stage::Downloading {
            let _ = store
                .set_item(&row.key, Stage::Downloading, ItemState::Running)
                .await;
        }
    }

    /// An item left the Active list: it finished, failed, or was deleted
    async fn resolve_departed(&self, row: &ItemRow, finished: Option<&FinishedRow>) {
        let store = &self.inner.store;
        let outcome = match finished {
            Some(f) => Some(f.outcome),
            None if !row.key.starts_with("transfer:") => self.non_transfer_outcome(row).await,
            None => None,
        };
        match outcome {
            Some(FinishedOutcome::Done) if row.stage != Stage::PreDownload => {
                self.inner.missed.lock().unwrap().remove(&row.key);
                self.downloaded(row).await;
            }
            Some(FinishedOutcome::Failed) => {
                self.inner.missed.lock().unwrap().remove(&row.key);
                let _ = store
                    .set_item(&row.key, Stage::Downloading, ItemState::Failed)
                    .await;
            }
            _ => {
                let n = {
                    let mut missed = self.inner.missed.lock().unwrap();
                    let n = missed.entry(row.key.clone()).or_insert(0);
                    *n += 1;
                    *n
                };
                if n >= MISSED_TICKS {
                    self.cancel(&row.key).await;
                }
            }
        }
    }

    /// Whether a folder or file item that left the list finished: its files are no longer
    /// queued and it did not fail. `None` while any are still queued.
    async fn non_transfer_outcome(&self, row: &ItemRow) -> Option<FinishedOutcome> {
        let db = &self.inner.db;
        let queued = if let Some(id) = row.key.strip_prefix("file:").and_then(|s| s.parse().ok()) {
            db.queue().get_by_file_id(id).await.ok().flatten().is_some()
        } else if let Some(path) = row.key.strip_prefix("folder:") {
            !db.queue()
                .get_for_folder(path)
                .await
                .unwrap_or_default()
                .is_empty()
        } else {
            false
        };
        if queued {
            None
        } else if row.state == ItemState::Failed {
            Some(FinishedOutcome::Failed)
        } else {
            Some(FinishedOutcome::Done)
        }
    }

    /// The files are on the NAS: find them and start pre-processing
    async fn downloaded(&self, row: &ItemRow) {
        let settings = self.settings();
        let store = &self.inner.store;
        let paths = locate::local_paths(&self.inner.db, &row.key, settings.retention_hours).await;
        let loc = locate::derive(
            &row.name,
            &paths.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(),
            &settings.target,
        );
        if !loc.dir.as_os_str().is_empty() {
            let _ = store.set_dir(&row.key, &loc.dir.to_string_lossy()).await;
        }
        if row.category.is_none() {
            let _ = store.set_category(&row.key, loc.category.as_deref()).await;
        }
        if let Ok(steps) = store.steps(&row.key).await {
            for s in steps.iter().filter(|s| s.kind == StepKind::Download) {
                let _ = store.set_step(s.id, StepState::Done, None, None).await;
            }
        }
        let _ = store
            .set_item(&row.key, Stage::PreProcessing, ItemState::Running)
            .await;
        self.inner.wake.notify_one();
    }

    /// An item was deleted: stop its running step and mark it cancelled
    async fn cancel(&self, key: &str) {
        let _ = self
            .inner
            .store
            .set_item(key, Stage::Downloading, ItemState::Cancelled)
            .await;
        if let Some(notify) = self.inner.cancels.lock().unwrap().get(key) {
            notify.notify_waiters();
        }
        self.inner.missed.lock().unwrap().remove(key);
        info!(key, "Pipeline item cancelled: it is gone from Put.io");
    }

    /// FR-017: a hold on the item's transfer pauses its steps, and releasing it goes on
    async fn reflect_hold(&self, row: &ItemRow) {
        let Some(id) = row.transfer_id else { return };
        if row.stage <= Stage::Downloading {
            return;
        }
        let held = self.inner.db.holds().is_held(id).await.unwrap_or(false);
        let store = &self.inner.store;
        if held && row.state == ItemState::Running {
            let _ = store.set_item(&row.key, row.stage, ItemState::Held).await;
        } else if !held && row.state == ItemState::Held {
            let _ = store
                .set_item(&row.key, row.stage, ItemState::Running)
                .await;
        }
    }

    /// Rebuild what the gate answers, and the waiting gauge
    pub(crate) async fn refresh_gate(&self) {
        let Ok(items) = self.inner.store.items().await else {
            return;
        };
        let mut pending = HashMap::new();
        let mut waiting: HashMap<Stage, usize> = HashMap::new();
        for item in items.iter().filter(|i| !i.state.is_terminal()) {
            if item.state == ItemState::WaitingForOperator {
                *waiting.entry(item.stage).or_default() += 1;
            }
            if item.stage <= Stage::Downloading {
                continue;
            }
            let steps = self.inner.store.steps(&item.key).await.unwrap_or_default();
            let open = steps.iter().find(|s| {
                s.kind == StepKind::Hook
                    && !s.state.is_settled()
                    && (s.state != StepState::Failed || s.required)
            });
            let holding = matches!(item.state, ItemState::Held | ItemState::WaitingForOperator);
            if let Some(step) = open {
                pending.insert(
                    item.key.clone(),
                    format!("{}: {}", step.stage.as_str(), step.name),
                );
            } else if holding {
                pending.insert(item.key.clone(), item.stage.as_str().to_string());
            }
        }
        self.inner.gate.set(pending);
        for stage in Stage::ALL {
            putio_sync_telemetry::steps::set_waiting(
                stage.as_str(),
                waiting.get(&stage).copied().unwrap_or(0),
            );
        }
    }

    /// `GET /active/{key}/timeline`
    pub async fn timeline(&self, key: &str) -> Option<Timeline> {
        let store = &self.inner.store;
        let item = store.item(key).await.ok()??;
        let steps = store.steps(key).await.ok()?;
        let mut attempts = Vec::new();
        for s in &steps {
            attempts.extend(store.attempts(s.id).await.ok()?);
        }
        Some(timeline::timeline(&item, &steps, &attempts))
    }

    /// The item finished with warnings (an optional step gave up)
    pub async fn has_warnings(&self, key: &str) -> bool {
        matches!(
            self.inner.store.item(key).await,
            Ok(Some(item)) if item.state == ItemState::DoneWithWarnings
        )
    }

    /// The step line of an item's Active row, by key
    pub async fn summaries(&self) -> HashMap<String, StepSummary> {
        let mut out = HashMap::new();
        let store = &self.inner.store;
        for item in store.items().await.unwrap_or_default() {
            if item.state.is_terminal() {
                continue;
            }
            let steps = store.steps(&item.key).await.unwrap_or_default();
            if let Some(summary) = timeline::summary(&item, &steps) {
                out.insert(item.key, summary);
            }
        }
        out
    }

    /// Items to show on the Active page that the download view no longer lists: downloaded,
    /// not finished with processing
    pub async fn processing_items(&self) -> Vec<ItemRow> {
        self.inner
            .store
            .items()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|i| !i.state.is_terminal() && i.stage > Stage::Downloading)
            .collect()
    }

    /// Run a failed or interrupted step again
    pub async fn retry(&self, key: &str, step_id: i64) -> Result<(), ActionError> {
        let (item, step) = self.step_of(key, step_id).await?;
        if !matches!(step.state, StepState::Failed | StepState::Interrupted)
            || item.state.is_terminal()
        {
            return Err(ActionError::Conflict(format!(
                "the step is {}, not failed or interrupted",
                step.state.as_str()
            )));
        }
        let store = &self.inner.store;
        store
            .set_step(step.id, StepState::Waiting, None, None)
            .await
            .map_err(|e| ActionError::Conflict(e.to_string()))?;
        self.resume_item(&item).await;
        Ok(())
    }

    /// Skip a failed step; the item goes on (records `skip_reason = operator`)
    pub async fn skip(&self, key: &str, step_id: i64) -> Result<(), ActionError> {
        let (item, step) = self.step_of(key, step_id).await?;
        if step.state != StepState::Failed || item.state.is_terminal() {
            return Err(ActionError::Conflict(format!(
                "the step is {}, not failed",
                step.state.as_str()
            )));
        }
        self.inner
            .store
            .set_step(
                step.id,
                StepState::Skipped,
                Some(model::SkipReason::Operator),
                None,
            )
            .await
            .map_err(|e| ActionError::Conflict(e.to_string()))?;
        self.resume_item(&item).await;
        Ok(())
    }

    async fn step_of(&self, key: &str, step_id: i64) -> Result<(ItemRow, StepRow), ActionError> {
        let store = &self.inner.store;
        let item = store
            .item(key)
            .await
            .ok()
            .flatten()
            .ok_or(ActionError::NotFound)?;
        let step = store
            .step(step_id)
            .await
            .ok()
            .flatten()
            .filter(|s| s.item_key == key)
            .ok_or(ActionError::NotFound)?;
        Ok((item, step))
    }

    /// After a retry or skip: a waiting item goes on, unless another required step failed
    async fn resume_item(&self, item: &ItemRow) {
        let store = &self.inner.store;
        if item.state == ItemState::WaitingForOperator {
            let steps = store.steps(&item.key).await.unwrap_or_default();
            let blocked = steps
                .iter()
                .any(|s| s.state == StepState::Failed && s.required);
            if !blocked {
                let _ = store
                    .set_item(&item.key, item.stage, ItemState::Running)
                    .await;
            }
        }
        self.refresh_gate().await;
        self.inner.wake.notify_one();
    }
}

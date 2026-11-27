//! Put.io storage management (feature 033)
//!
//! One [`StorageService`] per process is shared by the daemon (gauges and the opt-in daily
//! run), the API (and so the web UI) and the CLI. It reports what fills the account, builds
//! a preview of the oldest files that are safe to delete, and deletes exactly the files of
//! a preview after checking each again. One preview or free-up runs at a time.
//!
//! Feature 064 adds the storage budget (keep the account under a share of its size, with
//! the same rules and the same per-file check) and the samples behind the Stats page's
//! forecast of when the download disk and the account fill.

pub mod plan;

use plan::{Candidate, Inputs};
use putio_sync_client::{ClientError, PutioApiClient};
use putio_sync_core::config::{Config, StorageConfig};
use putio_sync_core::events::{self, Payload};
use putio_sync_core::forecast::{self, Forecast};
use putio_sync_db::models::HistoryAction;
use putio_sync_db::{Database, DbError, NewRunFile, StorageRun};
use putio_sync_telemetry::{storage as metrics, SyncResult};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use thiserror::Error;
use tokio::time::Instant;
use tracing::{info, warn};

/// What started a run (the `trigger` label and the recorded run's `trigger`)
pub use putio_sync_telemetry::CleanupTrigger as Trigger;

/// Transfer statuses that no longer write into their folder
const FINISHED_TRANSFER_STATUSES: &[&str] = &["COMPLETED", "SEEDING"];
/// How long a preview can be confirmed
const PLAN_TTL: Duration = Duration::from_secs(30 * 60);
/// How long a preview's walk of the account may take
const WALK_DEADLINE: Duration = Duration::from_secs(120);
const WALK_CONCURRENCY: usize = 4;
const DELETE_BATCH: usize = 100;
/// Items on the storage card
const TOP_ITEMS: usize = 10;
/// Runs `runs()` lists
pub const RECENT_RUNS: i64 = 20;
/// The recorded run's `trigger` for a storage-budget run (feature 064)
pub const BUDGET_TRIGGER: &str = "budget";
/// The `reason` of the `cleanup.deleted` event a budget run sends
pub const BUDGET_REASON: &str = "storage_budget";
/// After a budget run that could delete nothing, the account is not walked again for
/// this long
const BUDGET_RETRY: Duration = Duration::from_secs(60 * 60);
/// Usage samples are at least this far apart
const SAMPLE_GAP_SECS: i64 = 3600;
const SAMPLE_PUTIO: &str = "putio";
const SAMPLE_LOCAL: &str = "local";

/// Why a preview or run did not happen
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("A free-up run is already going")]
    AlreadyRunning,
    #[error("Freeing space needs the database (download records)")]
    NoDatabase,
    #[error("The preview has expired or was replaced. Preview again.")]
    PlanExpired,
    #[error("No files were chosen")]
    NothingChosen,
    #[error("The storage budget must be between {min} and {max} percent")]
    BadBudget { min: u8, max: u8 },
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// A top-level item of the account
#[derive(Debug, Clone, Serialize)]
pub struct TopItem {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub is_folder: bool,
    /// Share of the used space, 0–100
    pub share_percent: f64,
}

/// Storage use and the settings that judge it
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub size: i64,
    pub used: i64,
    pub free: i64,
    pub used_percent: f64,
    /// Free space is below the warning level
    pub low: bool,
    pub warn_free_percent: u8,
    pub target_free_percent: u8,
    pub auto_free: bool,
    pub run_at: String,
    /// The storage budget deletes files (feature 064)
    pub budget_enabled: bool,
    /// The share of the account the budget keeps it under
    pub budget_used_percent: u8,
    /// Bytes used over the budget; 0 at or under it. Reported whether the budget is on
    /// or off.
    pub over_budget_bytes: i64,
    /// Up to 10 root items, largest first
    pub top: Vec<TopItem>,
}

/// A preview: what a free-up would delete. It deletes nothing.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub id: String,
    pub free_before: i64,
    pub target_bytes: i64,
    pub needed_bytes: i64,
    pub total_bytes: i64,
    pub free_after: i64,
    pub shortfall_bytes: i64,
    /// False when the walk of the account was cut short or a folder failed to list
    pub complete: bool,
    pub files: Vec<Candidate>,
}

/// A file a run left alone, and why
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Skipped {
    pub id: i64,
    pub path: String,
    pub reason: String,
}

/// A file Put.io refused to delete
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Failed {
    pub id: i64,
    pub path: String,
    pub error: String,
}

/// What a free-up did
#[derive(Debug, Clone, Serialize)]
pub struct FreeResult {
    /// The recorded run; `None` when nothing was deleted
    pub run_id: Option<i64>,
    pub bytes_deleted: i64,
    pub free_after: Option<i64>,
    pub deleted: Vec<Candidate>,
    pub skipped: Vec<Skipped>,
    pub failed: Vec<Failed>,
}

/// When a disk fills, with the numbers it was worked out from (feature 064)
#[derive(Debug, Clone, Serialize)]
pub struct DiskForecast {
    /// The latest reading; `None` before the first one
    pub total_bytes: Option<i64>,
    pub used_bytes: Option<i64>,
    #[serde(flatten)]
    pub forecast: Forecast,
}

/// When the download disk and the Put.io account fill at the rate of the last week
#[derive(Debug, Clone, Serialize)]
pub struct Forecasts {
    pub local: DiskForecast,
    pub putio: DiskForecast,
}

/// `(free, total)` bytes of the disk holding a folder; `None` when it can't be read
pub type DiskReader = Arc<dyn Fn(&Path) -> Option<(u64, u64)> + Send + Sync>;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

struct StoredPlan {
    plan: Plan,
    made: Instant,
}

/// Put.io storage use and free-up runs; one run at a time
pub struct StorageService {
    client: Arc<PutioApiClient>,
    db: Option<Arc<Database>>,
    config: RwLock<StorageConfig>,
    running: tokio::sync::Mutex<()>,
    latest: Mutex<Option<StoredPlan>>,
    /// The last reading was low, so `storage.low` is not sent again until it recovers
    /// (feature 027, R7)
    low: std::sync::atomic::AtomicBool,
    /// The download folder, whose disk is sampled for the forecast (feature 064)
    download_dir: RwLock<PathBuf>,
    disk: DiskReader,
    /// A budget run found nothing it may delete; no new walk before this
    budget_retry_at: Mutex<Option<Instant>>,
}

fn free_of(size: i64, used: i64) -> i64 {
    (size - used).max(0)
}

/// Free space is below `warn_percent` of `size`
pub fn is_low(size: i64, free: i64, warn_percent: u8) -> bool {
    size > 0 && (free as i128) * 100 < (size as i128) * warn_percent as i128
}

fn local_size(path: &str) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

impl StorageService {
    /// A service using `client` and `db`, with `[storage]` from `config`
    pub fn from_config(
        client: Arc<PutioApiClient>,
        db: Option<Arc<Database>>,
        config: &Config,
    ) -> Self {
        Self {
            client,
            db,
            config: RwLock::new(config.storage.clone()),
            running: tokio::sync::Mutex::new(()),
            latest: Mutex::new(None),
            low: std::sync::atomic::AtomicBool::new(false),
            download_dir: RwLock::new(config.download.target_directory.clone()),
            disk: Arc::new(|dir| putio_sync_telemetry::gauges::disk_usage(dir).ok()),
            budget_retry_at: Mutex::new(None),
        }
    }

    /// Read the download disk with `disk` instead of the filesystem (tests)
    pub fn with_disk_reader(mut self, disk: DiskReader) -> Self {
        self.disk = disk;
        self
    }

    /// Take new settings after a settings save
    pub fn configure(&self, config: &Config) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = config.storage.clone();
        *self.download_dir.write().unwrap_or_else(|e| e.into_inner()) =
            config.download.target_directory.clone();
        *self
            .budget_retry_at
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// The current `[storage]` settings
    pub fn config(&self) -> StorageConfig {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn db(&self) -> Result<&Database, StorageError> {
        self.db.as_deref().ok_or(StorageError::NoDatabase)
    }

    /// Account numbers only, setting the gauges; `(size, used)`. Sends `storage.low`
    /// when free space first drops below `warn_free_percent`, and again only after a
    /// reading above it, which sends `storage.recovered`.
    pub async fn refresh(&self) -> Result<(i64, i64), StorageError> {
        let disk = self.client.get_account_info().await?.disk;
        metrics::set_storage(disk.size, disk.used);
        let warn = self.config().warn_free_percent;
        let free = free_of(disk.size, disk.used);
        let low = is_low(disk.size, free, warn);
        let was_low = self.low.swap(low, std::sync::atomic::Ordering::SeqCst);
        if low != was_low {
            let reading = events::StorageLow {
                total_bytes: disk.size,
                used_bytes: disk.used,
                free_bytes: free,
                free_percent: (free as f64 * 1000.0 / disk.size as f64).round() / 10.0,
                warn_free_percent: warn,
            };
            events::emit(if low {
                Payload::StorageLow(reading)
            } else {
                // Back above the warning level (feature 057)
                Payload::StorageRecovered(reading)
            });
        }
        self.sample(disk.size, disk.used, now_secs()).await;
        Ok((disk.size, disk.used))
    }

    /// Keep one reading an hour of the account and of the download disk, for the
    /// forecast. A failure only costs a sample.
    async fn sample(&self, size: i64, used: i64, at: i64) {
        let Some(db) = self.db.as_deref() else {
            return;
        };
        let repo = db.storage();
        if let Err(e) = repo
            .add_sample(SAMPLE_PUTIO, at, used, size, SAMPLE_GAP_SECS)
            .await
        {
            warn!(error = %e, "Could not record Put.io storage use");
        }
        let dir = self
            .download_dir
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some((free, total)) = (self.disk)(&dir) {
            let (free, total) = (free.min(i64::MAX as u64), total.min(i64::MAX as u64));
            let used = total.saturating_sub(free) as i64;
            if let Err(e) = repo
                .add_sample(SAMPLE_LOCAL, at, used, total as i64, SAMPLE_GAP_SECS)
                .await
            {
                warn!(error = %e, "Could not record download disk use");
            }
        }
    }

    /// When the download disk and the account fill, from the samples of the last week.
    /// Reads the database only.
    pub async fn forecast(&self) -> Result<Forecasts, StorageError> {
        self.forecast_at(now_secs()).await
    }

    async fn forecast_at(&self, now: i64) -> Result<Forecasts, StorageError> {
        Ok(Forecasts {
            local: self.forecast_of(SAMPLE_LOCAL, now).await?,
            putio: self.forecast_of(SAMPLE_PUTIO, now).await?,
        })
    }

    async fn forecast_of(&self, kind: &str, now: i64) -> Result<DiskForecast, StorageError> {
        let rows = match self.db.as_deref() {
            Some(db) => {
                db.storage()
                    .samples(kind, now - forecast::WINDOW_SECS)
                    .await?
            }
            None => Vec::new(),
        };
        let latest = rows.last().copied();
        let samples: Vec<forecast::Sample> = rows
            .iter()
            .map(|r| forecast::Sample {
                at: r.taken_at,
                used: r.used.max(0) as u64,
            })
            .collect();
        let total = latest.map_or(0, |r| r.total.max(0) as u64);
        Ok(DiskForecast {
            total_bytes: latest.map(|r| r.total),
            used_bytes: latest.map(|r| r.used),
            forecast: forecast::fill(&samples, total, now),
        })
    }

    /// Storage use and the largest top-level items; sets the gauges
    pub async fn summary(&self) -> Result<Summary, StorageError> {
        let (size, used) = self.refresh().await?;
        let mut top = self.client.list_files(0).await?.files;
        top.sort_by(|a, b| b.size.cmp(&a.size).then(a.id.cmp(&b.id)));
        top.truncate(TOP_ITEMS);
        let config = self.config();
        let free = free_of(size, used);
        let percent = |part: i64, of: i64| {
            if of > 0 {
                (part as f64 * 1000.0 / of as f64).round() / 10.0
            } else {
                0.0
            }
        };
        Ok(Summary {
            size,
            used,
            free,
            used_percent: percent(used, size),
            low: is_low(size, free, config.warn_free_percent),
            warn_free_percent: config.warn_free_percent,
            target_free_percent: config.target_free_percent,
            auto_free: config.auto_free,
            run_at: config.run_at,
            budget_enabled: config.budget_enabled,
            budget_used_percent: config.budget_used_percent,
            over_budget_bytes: plan::over_budget(size, used, config.budget_used_percent),
            top: top
                .into_iter()
                .map(|f| TopItem {
                    share_percent: percent(f.size, used),
                    id: f.id,
                    name: f.name,
                    size: f.size,
                    is_folder: f.is_folder,
                })
                .collect(),
        })
    }

    /// Ids of unfinished transfers' save folders and files, never the root
    async fn unfinished(&self) -> Result<HashSet<i64>, StorageError> {
        let transfers = self.client.list_transfers().await?.transfers;
        Ok(transfers
            .iter()
            .filter(|t| !FINISHED_TRANSFER_STATUSES.contains(&t.status.as_str()))
            .flat_map(|t| [Some(t.save_parent_id), t.file_id])
            .flatten()
            .filter(|&id| id != 0)
            .collect())
    }

    async fn completed(
        &self,
        db: &Database,
    ) -> Result<HashMap<i64, Vec<(i64, String)>>, StorageError> {
        let mut map: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
        for (id, size, path) in db.downloads().completed().await? {
            map.entry(id).or_default().push((size, path));
        }
        Ok(map)
    }

    async fn queued(&self, db: &Database) -> Result<HashSet<i64>, StorageError> {
        Ok(db
            .queue()
            .claimed()
            .await?
            .into_iter()
            .map(|q| q.file_id)
            .collect())
    }

    /// Build a preview of what a free-up would delete, keep it for 30 minutes, and
    /// return it. Deletes nothing.
    pub async fn plan(&self) -> Result<Plan, StorageError> {
        let _guard = self
            .running
            .try_lock()
            .map_err(|_| StorageError::AlreadyRunning)?;
        self.plan_locked().await
    }

    async fn plan_locked(&self) -> Result<Plan, StorageError> {
        let target_percent = self.config().target_free_percent;
        let (size, used) = self.refresh().await?;
        let free = free_of(size, used);
        let target = plan::target_bytes(size, target_percent);
        let needed = plan::needed_bytes(size, free, target_percent);
        let plan = self.build_plan(free, target, needed).await?;
        self.keep(&plan);
        Ok(plan)
    }

    /// The preview that frees `needed` bytes: the oldest files that pass every rule of
    /// [`plan::eligible`]. Deletes nothing and is not kept.
    async fn build_plan(&self, free: i64, target: i64, needed: i64) -> Result<Plan, StorageError> {
        let db = self.db()?;

        let (files, total, complete) = if needed == 0 {
            (Vec::new(), 0, true)
        } else {
            let walk = self
                .client
                .walk_files(WALK_CONCURRENCY, Instant::now() + WALK_DEADLINE)
                .await;
            let unfinished = self.unfinished().await?;
            let completed = self.completed(db).await?;
            let queued = self.queued(db).await?;
            let inputs = Inputs {
                files: &walk.files,
                completed: &completed,
                queued: &queued,
                unfinished: &unfinished,
            };
            let candidates = plan::eligible(&inputs, local_size);
            let (files, total) = plan::select(candidates, needed);
            (files, total, walk.complete && walk.errors.is_empty())
        };

        let plan = Plan {
            id: uuid::Uuid::new_v4().to_string(),
            free_before: free,
            target_bytes: target,
            needed_bytes: needed,
            total_bytes: total,
            free_after: free + total,
            shortfall_bytes: (needed - total).max(0),
            complete,
            files,
        };
        info!(
            files = plan.files.len(),
            total_bytes = plan.total_bytes,
            needed_bytes = plan.needed_bytes,
            shortfall_bytes = plan.shortfall_bytes,
            complete = plan.complete,
            "Put.io free-up preview built"
        );
        Ok(plan)
    }

    /// Make `plan` the one a free-up may confirm
    fn keep(&self, plan: &Plan) {
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(StoredPlan {
            plan: plan.clone(),
            made: Instant::now(),
        });
    }

    /// What the storage budget would delete now to bring the account under `percent` of
    /// its size (the saved `budget_used_percent` when `None`). Deletes nothing, works
    /// with the budget on or off, and cannot be confirmed by [`Self::free`].
    pub async fn budget_preview(&self, percent: Option<u8>) -> Result<Plan, StorageError> {
        let percent = percent.unwrap_or_else(|| self.config().budget_used_percent);
        let range = putio_sync_core::config::STORAGE_BUDGET_RANGE;
        if !range.contains(&percent) {
            return Err(StorageError::BadBudget {
                min: *range.start(),
                max: *range.end(),
            });
        }
        let _guard = self
            .running
            .try_lock()
            .map_err(|_| StorageError::AlreadyRunning)?;
        let (size, used) = self.refresh().await?;
        self.budget_plan(size, used, percent).await
    }

    async fn budget_plan(&self, size: i64, used: i64, percent: u8) -> Result<Plan, StorageError> {
        let needed = plan::over_budget(size, used, percent);
        let free = free_of(size, used);
        self.build_plan(free, free + needed, needed).await
    }

    /// The storage budget: when `budget_enabled` is on and the account is over
    /// `budget_used_percent`, delete the oldest downloaded files until it is under.
    /// Every file passes the rules of a free-up and is checked again right before its
    /// delete. `None` when it is off, the account is under budget, or nothing may be
    /// deleted.
    pub async fn budget_run(&self) -> Result<Option<FreeResult>, StorageError> {
        let config = self.config();
        if !config.budget_enabled {
            return Ok(None);
        }
        let waiting = |at: &Option<Instant>| at.is_some_and(|at| Instant::now() < at);
        if waiting(
            &self
                .budget_retry_at
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        ) {
            return Ok(None);
        }
        let _guard = self
            .running
            .try_lock()
            .map_err(|_| StorageError::AlreadyRunning)?;
        let (size, used) = self.refresh().await?;
        if plan::over_budget(size, used, config.budget_used_percent) == 0 {
            return Ok(None);
        }
        let plan = self
            .budget_plan(size, used, config.budget_used_percent)
            .await?;
        info!(
            budget_used_percent = config.budget_used_percent,
            over_bytes = plan.needed_bytes,
            files = plan.files.len(),
            "Put.io account is over the storage budget"
        );
        let result = if plan.files.is_empty() {
            None
        } else {
            let ids: Vec<i64> = plan.files.iter().map(|f| f.id).collect();
            self.keep(&plan);
            Some(
                self.free_locked(Trigger::Schedule, BUDGET_TRIGGER, &plan.id, &ids)
                    .await?,
            )
        };
        let deleted: Vec<i64> = result
            .iter()
            .flat_map(|r| r.deleted.iter().map(|c| c.id))
            .collect();
        if deleted.is_empty() {
            info!(
                over_bytes = plan.needed_bytes,
                "The storage budget found no file it may delete; it looks again in an hour"
            );
            *self
                .budget_retry_at
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + BUDGET_RETRY);
        } else {
            events::emit(Payload::CleanupDeleted(events::CleanupDeleted {
                count: deleted.len() as u64,
                file_ids: deleted,
                reason: BUDGET_REASON.into(),
            }));
        }
        Ok(result)
    }

    /// Delete the chosen files of preview `plan_id`, checking each again first
    pub async fn free(
        &self,
        trigger: Trigger,
        plan_id: &str,
        file_ids: &[i64],
    ) -> Result<FreeResult, StorageError> {
        let _guard = self
            .running
            .try_lock()
            .map_err(|_| StorageError::AlreadyRunning)?;
        self.free_locked(trigger, trigger.as_str(), plan_id, file_ids)
            .await
    }

    /// `label` is the recorded run's trigger and the one in the log; `trigger` the metric's
    async fn free_locked(
        &self,
        trigger: Trigger,
        label: &str,
        plan_id: &str,
        file_ids: &[i64],
    ) -> Result<FreeResult, StorageError> {
        if file_ids.is_empty() {
            return Err(StorageError::NothingChosen);
        }
        let db = self.db()?;
        let plan = {
            let mut latest = self.latest.lock().unwrap_or_else(|e| e.into_inner());
            match latest.take() {
                Some(stored) if stored.plan.id == plan_id && stored.made.elapsed() < PLAN_TTL => {
                    stored.plan
                }
                other => {
                    *latest = other;
                    return Err(StorageError::PlanExpired);
                }
            }
        };
        info!(
            trigger = label,
            files = file_ids.len(),
            "Put.io free-up started"
        );

        let chosen: HashSet<i64> = file_ids.iter().copied().collect();
        let mut skipped: Vec<Skipped> = file_ids
            .iter()
            .filter(|id| !plan.files.iter().any(|f| f.id == **id))
            .map(|&id| Skipped {
                id,
                path: String::new(),
                reason: "not in the preview".into(),
            })
            .collect();

        // Read the rules' inputs again: a file may have been queued, a transfer started
        // or a local copy removed since the preview
        let unfinished = self.unfinished().await?;
        let completed = self.completed(db).await?;
        let queued = self.queued(db).await?;

        let mut ready = Vec::new();
        for c in plan.files.into_iter().filter(|f| chosen.contains(&f.id)) {
            match self.recheck(&c, &unfinished, &completed, &queued).await {
                Ok(()) => ready.push(c),
                Err(reason) => {
                    info!(file_id = c.id, path = %c.path, %reason, "Put.io free-up skipped a file");
                    skipped.push(Skipped {
                        id: c.id,
                        path: c.path,
                        reason,
                    });
                }
            }
        }

        let mut deleted = Vec::new();
        let mut failed = Vec::new();
        for batch in ready.chunks(DELETE_BATCH) {
            match self
                .client
                .delete_files(batch.iter().map(|c| c.id).collect())
                .await
            {
                Ok(()) => {
                    for c in batch {
                        info!(
                            file_id = c.id,
                            path = %c.path,
                            size = c.size,
                            local_path = %c.local_path,
                            trigger = label,
                            "Deleted Put.io file to free space"
                        );
                        if let Err(e) = db
                            .history()
                            .insert(
                                c.id,
                                file_name(&c.path),
                                c.size,
                                HistoryAction::Deleted,
                                None,
                                None,
                            )
                            .await
                        {
                            warn!(file_id = c.id, error = %e, "Could not add the deleted file to History");
                        }
                    }
                    deleted.extend_from_slice(batch);
                }
                Err(e) => {
                    warn!(files = batch.len(), error = %e, "Put.io refused a free-up delete");
                    failed.extend(batch.iter().map(|c| Failed {
                        id: c.id,
                        path: c.path.clone(),
                        error: e.to_string(),
                    }));
                }
            }
        }

        let free_after = match self.refresh().await {
            Ok((size, used)) => Some(free_of(size, used)),
            Err(e) => {
                warn!(error = %e, "Could not read Put.io storage after the free-up");
                None
            }
        };
        let bytes_deleted: i64 = deleted.iter().map(|c| c.size).sum();
        let run_id = if deleted.is_empty() {
            None
        } else {
            let files: Vec<NewRunFile> = deleted
                .iter()
                .map(|c| NewRunFile {
                    file_id: c.id,
                    path: c.path.clone(),
                    size: c.size,
                    created_at: c.created_at.clone(),
                    local_path: c.local_path.clone(),
                })
                .collect();
            Some(
                db.storage()
                    .record_run(label, &files, failed.len(), free_after)
                    .await?,
            )
        };
        let result = if failed.is_empty() {
            SyncResult::Success
        } else {
            SyncResult::Failure
        };
        metrics::record_run(trigger, result, deleted.len(), bytes_deleted.max(0) as u64);
        info!(
            trigger = label,
            deleted = deleted.len(),
            bytes_deleted,
            skipped = skipped.len(),
            failed = failed.len(),
            free_after,
            "Put.io free-up finished"
        );
        Ok(FreeResult {
            run_id,
            bytes_deleted,
            free_after,
            deleted,
            skipped,
            failed,
        })
    }

    /// Check one planned file against the rules again; the reason it must be kept
    async fn recheck(
        &self,
        c: &Candidate,
        unfinished: &HashSet<i64>,
        completed: &HashMap<i64, Vec<(i64, String)>>,
        queued: &HashSet<i64>,
    ) -> Result<(), String> {
        let file = match self.client.get_file_details(c.id).await {
            Ok(details) => details.file,
            Err(e) if e.is_not_found() => return Err("no longer on Put.io".into()),
            Err(e) => return Err(format!("could not check it on Put.io: {e}")),
        };
        if file.size != c.size {
            return Err("size changed on Put.io".into());
        }
        if file.is_shared {
            return Err("shared".into());
        }
        // The planner protected every folder above an unfinished transfer's files; do the same
        // with the transfers as they are now
        if unfinished.contains(&c.id)
            || file.parent_id.is_some_and(|p| unfinished.contains(&p))
            || c.ancestors.iter().any(|a| unfinished.contains(a))
        {
            return Err("part of an unfinished transfer".into());
        }
        if queued.contains(&c.id) {
            return Err("queued for download".into());
        }
        let recorded = completed.get(&c.id).is_some_and(|r| {
            r.iter()
                .any(|(size, path)| *size == c.size && *path == c.local_path)
        });
        if !recorded || local_size(&c.local_path) != Some(c.size as u64) {
            return Err("local copy missing or changed".into());
        }
        Ok(())
    }

    /// The latest recorded runs, newest first; empty without a database
    pub async fn runs(&self, limit: i64) -> Result<Vec<StorageRun>, StorageError> {
        match &self.db {
            Some(db) => Ok(db.storage().recent_runs(limit).await?),
            None => Ok(Vec::new()),
        }
    }

    /// The daily run: when `auto_free` is on and free space is below the warning level,
    /// preview and delete all of it. `None` when nothing was due.
    pub async fn auto_run(&self) -> Result<Option<FreeResult>, StorageError> {
        let config = self.config();
        if !config.auto_free {
            return Ok(None);
        }
        let _guard = self
            .running
            .try_lock()
            .map_err(|_| StorageError::AlreadyRunning)?;
        let (size, used) = self.refresh().await?;
        if !is_low(size, free_of(size, used), config.warn_free_percent) {
            info!("Put.io free space is above the warning level; no automatic free-up");
            return Ok(None);
        }
        let plan = self.plan_locked().await?;
        if plan.files.is_empty() {
            info!(
                needed_bytes = plan.needed_bytes,
                "Automatic free-up found no files it may delete"
            );
            return Ok(None);
        }
        let ids: Vec<i64> = plan.files.iter().map(|f| f.id).collect();
        self.free_locked(
            Trigger::Schedule,
            Trigger::Schedule.as_str(),
            &plan.id,
            &ids,
        )
        .await
        .map(Some)
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests;

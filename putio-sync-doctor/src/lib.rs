//! The Put.io doctor (feature 010): health checks for the account, the sync state, the
//! NAS side and the connection, with safe fixes
//!
//! [`Doctor`] is shared by the daemon (scheduled runs), the API (on-demand runs and fixes)
//! and the CLI (one run per command). Each run fetches Put.io data once and shares it
//! between the checks.

mod checks;
mod fixes;
pub mod report;
mod schedule;

pub use fixes::{FixError, MAX_FIX_ITEMS};
pub use report::{
    Area, CheckId, CheckResult, Counts, FixId, FixOutcome, FixPreview, FixRequest, Item, Report,
    Status,
};
pub use schedule::{run_scheduled, STARTUP_DELAY};

use checks::Ctx;
use chrono::Utc;
use futures_util::future::join_all;
use putio_sync_client::PutioApiClient;
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::{Config, DoctorConfig};
use putio_sync_core::stall::StallTracker;
use putio_sync_db::Database;
use putio_sync_telemetry::doctor as metrics;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tracing::info;

/// Runs the checks and fixes, and keeps the latest report
pub struct Doctor {
    client: Arc<PutioApiClient>,
    db: Option<Arc<Database>>,
    cleanup: Arc<EmptyFolderCleanup>,
    stalls: Option<Arc<StallTracker>>,
    config: RwLock<Config>,
    config_path: Option<PathBuf>,
    latest: RwLock<Option<Report>>,
    /// Single-flight guard for [`Doctor::run`]
    running: tokio::sync::Mutex<()>,
    /// Finished runs, so a waiter can tell a run finished while it waited
    runs: AtomicU64,
    metrics: bool,
    /// Tailscale (feature 036); built from the config per run when not shared
    tailscale: Option<Arc<putio_sync_tailscale::TailscaleService>>,
}

impl Doctor {
    pub fn new(
        config: Config,
        client: Arc<PutioApiClient>,
        db: Option<Arc<Database>>,
        cleanup: Arc<EmptyFolderCleanup>,
    ) -> Self {
        Self {
            client,
            db,
            cleanup,
            stalls: None,
            config: RwLock::new(config),
            config_path: None,
            latest: RwLock::new(None),
            running: tokio::sync::Mutex::new(()),
            runs: AtomicU64::new(0),
            metrics: false,
            tailscale: None,
        }
    }

    /// Ask this Tailscale service (the API's), so the check shares its cache
    pub fn with_tailscale(
        mut self,
        tailscale: Arc<putio_sync_tailscale::TailscaleService>,
    ) -> Self {
        self.tailscale = Some(tailscale);
        self
    }

    /// Use the daemon's stall tracker, so the transfers check can report stalls
    pub fn with_stalls(mut self, stalls: Arc<StallTracker>) -> Self {
        self.stalls = Some(stalls);
        self
    }

    /// The config file to lint
    pub fn with_config_path(mut self, path: Option<PathBuf>) -> Self {
        self.config_path = path;
        self
    }

    /// Record results on the global metrics (the daemon and the API, not the CLI)
    pub fn with_metrics(mut self) -> Self {
        self.metrics = true;
        self
    }

    /// Pick up saved settings
    pub fn configure(&self, config: &Config) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = config.clone();
    }

    /// The current `[doctor]` settings
    pub fn settings(&self) -> DoctorConfig {
        self.config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .doctor
            .clone()
    }

    fn ctx(&self) -> Ctx {
        let config = self
            .config
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut ctx = Ctx::new(
            config,
            self.config_path.clone(),
            self.client.clone(),
            self.db.clone(),
            self.cleanup.clone(),
            self.stalls.clone(),
        );
        ctx.tailscale = self.tailscale.clone();
        ctx
    }

    /// The last finished run's report
    pub fn latest(&self) -> Option<Report> {
        self.latest
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Run every check. A call made while a run is in progress waits for it and returns
    /// its report instead of starting another.
    pub async fn run(&self) -> Report {
        let before = self.runs.load(Ordering::SeqCst);
        let _guard = self.running.lock().await;
        if self.runs.load(Ordering::SeqCst) != before {
            if let Some(report) = self.latest() {
                return report;
            }
        }
        let ctx = self.ctx();
        let started_at = Utc::now();
        let clock = Instant::now();
        let checks = join_all(CheckId::ALL.iter().map(|id| checks::run_check(&ctx, *id))).await;
        let report = Report::new(started_at, clock.elapsed(), checks);
        info!(
            overall = report.overall.as_str(),
            problems = report.counts.problem,
            warnings = report.counts.warning,
            duration_ms = report.duration_ms,
            "Doctor run finished"
        );
        if self.metrics {
            for check in &report.checks {
                record(check);
            }
            metrics::record_run(clock.elapsed());
        }
        *self.latest.write().unwrap_or_else(|e| e.into_inner()) = Some(report.clone());
        self.runs.fetch_add(1, Ordering::SeqCst);
        report
    }

    /// Detect again and list what `fix` would act on. Changes nothing.
    pub async fn preview(&self, fix: FixId) -> FixPreview {
        let ctx = self.ctx();
        let items = checks::finding(&ctx, fix.check()).await.items_for(fix);
        FixPreview {
            fix,
            destructive: fix.destructive(),
            item_count: items.len() as u64,
            items,
        }
    }

    /// Apply `fix` to the requested items that still qualify, then run its check again
    pub async fn apply(&self, fix: FixId, req: &FixRequest) -> Result<FixOutcome, FixError> {
        let keys = fixes::validate(fix, req)?;
        let applied = fixes::apply(&self.ctx(), fix, &keys).await?;
        info!(
            fix = fix.as_str(),
            items = keys.len(),
            done = applied.done.len(),
            skipped = applied.skipped.len(),
            failed = applied.failed.len(),
            "Doctor fix applied"
        );
        // A fresh context, so the check sees the result instead of cached data
        let check = checks::run_check(&self.ctx(), fix.check()).await;
        if self.metrics {
            record(&check);
        }
        if let Some(report) = self
            .latest
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            report.replace(check.clone());
        }
        Ok(FixOutcome {
            fix,
            done: applied.done,
            skipped: applied.skipped,
            failed: applied.failed,
            check,
        })
    }
}

fn record(check: &CheckResult) {
    metrics::record_check(
        check.id.as_str(),
        check.status.metric_value(),
        check.item_count,
    );
}

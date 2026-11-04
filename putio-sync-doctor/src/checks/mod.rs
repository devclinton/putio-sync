//! The checks: shared data for one run, and dispatch by [`CheckId`]

pub(crate) mod hooks;
pub(crate) mod local;
pub(crate) mod net;
pub(crate) mod plugins;
pub(crate) mod putio;
pub(crate) mod sync;

use crate::report::{CheckId, CheckResult, Finding, Status};
use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use putio_sync_client::{AccountProbe, FileWalk, PutioApiClient, Transfer};
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::Config;
use putio_sync_core::stall::StallTracker;
use putio_sync_db::Database;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;
use tokio::time::Instant;
use tracing::warn;

/// Folders listed at once by the account walk
pub(crate) const WALK_CONCURRENCY: usize = 4;
/// A whole run gives up after this
pub(crate) const RUN_DEADLINE: Duration = Duration::from_secs(5 * 60);
/// The walk stops early enough to leave the other checks time
const WALK_SHARE: Duration = Duration::from_secs(4 * 60);
/// Watch and partial files younger than this are still being handled
pub(crate) const STALE_AGE: Duration = Duration::from_secs(3600);

/// Everything the checks of one run share. Put.io data is fetched once, on first use.
pub(crate) struct Ctx {
    pub config: Config,
    pub config_path: Option<PathBuf>,
    pub client: Arc<PutioApiClient>,
    pub db: Option<Arc<Database>>,
    pub cleanup: Arc<EmptyFolderCleanup>,
    pub stalls: Option<Arc<StallTracker>>,
    /// The shared Tailscale service; the check builds one from the config without it
    pub tailscale: Option<Arc<putio_sync_tailscale::TailscaleService>>,
    /// The running hooks, for their last runs; the check reads the config alone without it
    pub hooks: Option<Arc<putio_sync_hooks::HooksService>>,
    /// The running plugins, for their state and last runs
    pub plugins: Option<Arc<putio_sync_plugins::PluginsService>>,
    pub started: Instant,
    probe: OnceCell<(AccountProbe, DateTime<Utc>)>,
    transfers: OnceCell<Result<Vec<Transfer>, String>>,
    walk: OnceCell<FileWalk>,
}

impl Ctx {
    pub fn new(
        config: Config,
        config_path: Option<PathBuf>,
        client: Arc<PutioApiClient>,
        db: Option<Arc<Database>>,
        cleanup: Arc<EmptyFolderCleanup>,
        stalls: Option<Arc<StallTracker>>,
    ) -> Self {
        Self {
            config,
            config_path,
            client,
            db,
            cleanup,
            stalls,
            tailscale: None,
            hooks: None,
            plugins: None,
            started: Instant::now(),
            probe: OnceCell::new(),
            transfers: OnceCell::new(),
            walk: OnceCell::new(),
        }
    }

    /// The account probe and the local time it was answered
    pub async fn probe(&self) -> &(AccountProbe, DateTime<Utc>) {
        self.probe
            .get_or_init(|| async {
                let probe = self.client.probe_account().await;
                (probe, Utc::now())
            })
            .await
    }

    pub async fn transfers(&self) -> Result<&[Transfer], &str> {
        self.transfers
            .get_or_init(|| async {
                self.client
                    .list_transfers()
                    .await
                    .map(|r| r.transfers)
                    .map_err(|e| e.to_string())
            })
            .await
            .as_ref()
            .map(Vec::as_slice)
            .map_err(String::as_str)
    }

    pub async fn walk(&self) -> &FileWalk {
        self.walk
            .get_or_init(|| {
                self.client
                    .walk_files(WALK_CONCURRENCY, self.started + WALK_SHARE)
            })
            .await
    }

    /// `Some(skipped)` when Put.io cannot be used for this run
    async fn putio_unusable(&self) -> Option<Finding> {
        let (probe, _) = self.probe().await;
        probe.result.as_ref().err().map(|_| {
            Finding::skipped("Skipped because Put.io could not be used; see the sign-in check.")
        })
    }
}

/// Run check `id` and grade it
async fn detect(ctx: &Ctx, id: CheckId) -> Finding {
    use CheckId::*;
    let needs_putio = matches!(
        id,
        Storage | Transfers | EmptyFolders | Duplicates | OrphanedTorrents | DeadQueueRows
    );
    if needs_putio {
        if let Some(skipped) = ctx.putio_unusable().await {
            return skipped;
        }
    }
    match id {
        Auth => putio::auth(ctx).await,
        Storage => putio::storage(ctx).await,
        Transfers => putio::transfers(ctx).await,
        EmptyFolders => putio::empty_folders(ctx).await,
        Duplicates => putio::duplicates(ctx).await,
        OrphanedTorrents => putio::orphaned_torrents(ctx).await,
        StuckWatchFiles => putio::stuck_watch_files(ctx).await,
        DeadQueueRows => sync::dead_queue_rows(ctx).await,
        FailingDownloads => sync::failing_downloads(ctx).await,
        OrphanPartials => sync::orphan_partials(ctx).await,
        Bandwidth => sync::bandwidth(ctx).await,
        DownloadDir => local::download_dir(ctx).await,
        DiskSpace => local::disk_space(ctx).await,
        Database => local::database(ctx).await,
        Config => local::config(ctx).await,
        Reachability => net::reachability(ctx).await,
        RateLimit => net::rate_limit(ctx).await,
        ClockSkew => net::clock_skew(ctx).await,
        Tailscale => net::tailscale(ctx).await,
        Listen => net::listen(ctx).await,
        Hooks => hooks::hooks(ctx).await,
        Plugins => plugins::plugins(ctx).await,
    }
}

/// Run check `id` to a finding; a panic or the run deadline becomes a Problem
pub(crate) async fn finding(ctx: &Ctx, id: CheckId) -> Finding {
    let deadline = ctx.started + RUN_DEADLINE;
    let run = AssertUnwindSafe(detect(ctx, id)).catch_unwind();
    match tokio::time::timeout_at(deadline, run).await {
        Ok(Ok(finding)) => finding,
        Ok(Err(_)) => {
            warn!(check = id.as_str(), "Doctor check panicked");
            Finding::new(Status::Problem, "The check could not complete: it crashed.")
                .suggest("See the putio-sync logs for details.")
        }
        Err(_) => Finding::new(
            Status::Problem,
            "The check could not complete: the doctor ran out of time.",
        )
        .suggest("Run the doctor again; if it keeps happening, Put.io may be slow."),
    }
}

/// Run check `id` into its report entry
pub(crate) async fn run_check(ctx: &Ctx, id: CheckId) -> CheckResult {
    finding(ctx, id).await.into_result(id)
}

/// `n` with a plural `noun`: "1 file", "3 files", "2 entries"
pub(crate) fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        return format!("1 {noun}");
    }
    match noun.strip_suffix('y') {
        Some(stem) if !stem.ends_with(['a', 'e', 'o', 'u']) => format!("{n} {stem}ies"),
        _ => format!("{n} {noun}s"),
    }
}

/// The verb form that agrees with `n`: `verb(1, "points", "point")`
pub(crate) fn verb<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 {
        one
    } else {
        many
    }
}

/// Bytes for people: "912 GB", "1.2 MB"
pub(crate) fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// An age for people: "3 days", "5 hours", "12 minutes"
pub(crate) fn age(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 2 * 86_400 {
        format!("{} days", secs / 86_400)
    } else if secs >= 2 * 3600 {
        format!("{} hours", secs / 3600)
    } else {
        count((secs / 60) as usize, "minute")
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use putio_sync_client::RetryPolicy;

    /// A context against `url` with a test config rooted in `dir`
    pub fn ctx(url: &str, dir: &std::path::Path, db: Option<Arc<Database>>) -> Ctx {
        let mut config = Config::default();
        config.download.target_directory = dir.join("downloads");
        std::fs::create_dir_all(&config.download.target_directory).unwrap();
        let client = Arc::new(
            PutioApiClient::with_base_url("t".into(), url.to_string()).with_retry_policy(
                RetryPolicy {
                    max_attempts: 1,
                    ..RetryPolicy::default()
                },
            ),
        );
        let cleanup = Arc::new(EmptyFolderCleanup::from_config(client.clone(), &config));
        Ctx::new(config, None, client, db, cleanup, None)
    }

    pub async fn db(dir: &std::path::Path) -> Arc<Database> {
        Arc::new(Database::new(dir.join(".putiosync.db")).await.unwrap())
    }

    pub const ACCOUNT: &str = r#"{"status":"OK","info":{"user_id":1,"username":"u",
        "mail":"u@example.com","account_active":true,
        "disk":{"size":1000000000000,"used":100000000000,"avail":900000000000}}}"#;

    pub fn transfer(id: i64, name: &str, status: &str) -> String {
        format!(
            r#"{{"id":{id},"name":"{name}","status":"{status}","created_at":"2026-01-01T00:00:00",
               "save_parent_id":0,"is_private":false,"downloaded":10,
               "magneturi":"magnet:?xt=urn:btih:{id}"}}"#
        )
    }

    pub fn transfer_list(list: &[String]) -> String {
        format!(r#"{{"status":"OK","transfers":[{}]}}"#, list.join(","))
    }

    pub fn entry(id: i64, name: &str, size: i64, folder: bool) -> String {
        format!(
            r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"{}","parent_id":null,
               "is_folder":{folder},"created_at":"2020-01-01T00:00:00"}}"#,
            if folder { "FOLDER" } else { "VIDEO" }
        )
    }

    pub fn listing(entries: &[String]) -> String {
        format!(
            r#"{{"status":"OK","files":[{}],"parent":{{"id":0,"name":"root","size":0,
               "file_type":"FOLDER","parent_id":null,"is_folder":true}}}}"#,
            entries.join(",")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_formats() {
        assert_eq!(bytes(912_000_000_000), "912 GB");
        assert_eq!(bytes(1_200_000), "1.2 MB");
        assert_eq!(bytes(12), "12 B");
        assert_eq!(age(Duration::from_secs(3 * 86_400)), "3 days");
        assert_eq!(age(Duration::from_secs(5 * 3600)), "5 hours");
        assert_eq!(age(Duration::from_secs(60)), "1 minute");
        assert_eq!(count(2, "file"), "2 files");
        assert_eq!(count(2, "queue entry"), "2 queue entries");
        assert_eq!(count(0, "retry"), "0 retries");
    }
}

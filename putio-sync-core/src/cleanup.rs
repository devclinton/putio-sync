//! The Put.io empty-folder cleanup service
//!
//! One [`EmptyFolderCleanup`] per process is shared by the daemon's daily schedule and the
//! API (and so the web UI). It holds the `[cleanup]` settings and the protected folder ids
//! from the config, lets one run happen at a time, and logs and counts each run.
//!
//! Feature 021: folders kept by a folder rule (`keep = true`) or a `[cleanup]
//! keep_patterns` entry are passed to the walk as [`KeepFolders`].

use crate::config::{CleanupConfig, Config};
use crate::pattern::Pattern;
use crate::rules::{sanitize_name, RuleSet};
use putio_sync_client::{
    ClientError, EmptyFolderOptions, EmptyFolderReport, FailedStep, KeepFolders, PutioApiClient,
};
use putio_sync_telemetry::{cleanup as metrics, SyncResult};

/// What started a run (the `trigger` metric label)
pub use putio_sync_telemetry::CleanupTrigger;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use thiserror::Error;
use tracing::{info, warn};

/// Why a cleanup run did not happen or did not finish
#[derive(Debug, Error)]
pub enum CleanupError {
    /// Another run holds the guard
    #[error("Empty-folder cleanup is already running")]
    AlreadyRunning,
    /// The run could not start: Put.io refused the transfer list or the root listing
    #[error(transparent)]
    Client(#[from] ClientError),
}

#[derive(Debug, Clone)]
struct Settings {
    config: CleanupConfig,
    protected: HashSet<i64>,
    keep: Option<KeepFolders>,
}

impl Settings {
    fn from_config(config: &Config) -> Self {
        Self {
            config: config.cleanup.clone(),
            protected: protected_ids(config),
            keep: keep_folders(config),
        }
    }
}

/// The folders the user keeps, `None` when no rule sets `keep` and no keep pattern is set
fn keep_folders(config: &Config) -> Option<KeepFolders> {
    let rules = config.download.rules.iter().any(|r| r.keep.is_some());
    let patterns: Vec<Pattern> = config
        .cleanup
        .keep_patterns
        .iter()
        .map(|p| Pattern::parse(p))
        .filter(|p| !p.is_empty())
        .collect();
    if !rules && patterns.is_empty() {
        return None;
    }
    let rules = RuleSet::from_config(&config.download);
    Some(KeepFolders::new(move |path| {
        keep_reason(&rules, &patterns, path)
    }))
}

/// Why the folder at `path` (Put.io names from the root) is kept: its effective rule
/// keeps it, or it or a folder above it matches a keep pattern
fn keep_reason(rules: &RuleSet, patterns: &[Pattern], path: &str) -> Option<String> {
    let segments: Vec<String> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(sanitize_name)
        .collect();
    let keep = rules.resolve(&segments.join("/")).keep;
    if keep.value {
        return Some(format!(
            "kept by the rule on {}",
            keep.from.unwrap_or_default()
        ));
    }
    for depth in 1..=segments.len() {
        let prefix = segments[..depth].join("/");
        let name = &segments[depth - 1];
        if let Some(p) = patterns.iter().find(|p| p.matches(name, &prefix)) {
            return Some(format!("matches keep pattern \"{}\"", p.as_str()));
        }
    }
    None
}

/// Folder ids from the config that are never deleted: the watch upload folder and the
/// sync folder
fn protected_ids(config: &Config) -> HashSet<i64> {
    [config.watch.parent_folder_id, config.sync.remote_parent_id]
        .into_iter()
        .flatten()
        .collect()
}

/// Removes empty folders from the Put.io account; one run at a time
pub struct EmptyFolderCleanup {
    client: Arc<PutioApiClient>,
    settings: RwLock<Settings>,
    running: tokio::sync::Mutex<()>,
}

impl EmptyFolderCleanup {
    /// A service using `client`, with settings and protected folders from `config`
    pub fn from_config(client: Arc<PutioApiClient>, config: &Config) -> Self {
        Self {
            client,
            settings: RwLock::new(Settings::from_config(config)),
            running: tokio::sync::Mutex::new(()),
        }
    }

    /// Take new settings and protected folders after a settings save; the next run (and
    /// the schedule's next check) uses them
    pub fn configure(&self, config: &Config) {
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = Settings::from_config(config);
    }

    /// The current `[cleanup]` settings
    pub fn config(&self) -> CleanupConfig {
        self.settings().config
    }

    fn settings(&self) -> Settings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Run the cleanup now. Fails with [`CleanupError::AlreadyRunning`] while another run
    /// is going. A dry run deletes nothing and records no metrics.
    pub async fn run(
        &self,
        trigger: CleanupTrigger,
        dry_run: bool,
    ) -> Result<EmptyFolderReport, CleanupError> {
        let _guard = self
            .running
            .try_lock()
            .map_err(|_| CleanupError::AlreadyRunning)?;
        let settings = self.settings();
        let options = EmptyFolderOptions {
            dry_run,
            min_age: Duration::from_secs(settings.config.min_age_hours.saturating_mul(3600)),
            protected: settings.protected,
            now: chrono::Utc::now(),
            nested: settings.config.nested,
            keep: settings.keep,
        };
        info!(
            trigger = trigger.as_str(),
            dry_run,
            min_age_hours = settings.config.min_age_hours,
            nested = settings.config.nested,
            "Empty-folder cleanup started"
        );

        let report = match self.client.remove_empty_folders(&options).await {
            Ok(report) => report,
            Err(e) => {
                warn!(trigger = trigger.as_str(), dry_run, error = %e, "Empty-folder cleanup failed");
                if !dry_run {
                    metrics::record_run(trigger, SyncResult::Failure);
                }
                return Err(e.into());
            }
        };

        for folder in &report.removed {
            info!(
                folder_id = folder.id,
                path = %folder.path,
                age_hours = folder.age_hours,
                dry_run,
                "{}",
                if dry_run {
                    "Would remove empty Put.io folder"
                } else {
                    "Removed empty Put.io folder"
                }
            );
        }
        info!(
            trigger = trigger.as_str(),
            dry_run,
            scanned = report.scanned,
            removed = report.removed.len(),
            kept_protected = report.kept_protected,
            kept_too_new = report.kept_too_new,
            kept_by_user = report.kept_by_user,
            failures = report.failures.len(),
            "Empty-folder cleanup finished"
        );

        if !dry_run {
            metrics::deleted(report.removed.len());
            for _ in report
                .failures
                .iter()
                .filter(|f| f.step == FailedStep::Delete)
            {
                metrics::delete_failed();
            }
            let result = if report.failures.is_empty() {
                SyncResult::Success
            } else {
                SyncResult::Failure
            };
            metrics::record_run(trigger, result);
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn service(base_url: String, config: &Config) -> EmptyFolderCleanup {
        let client = Arc::new(PutioApiClient::with_base_url(
            "test_token".to_string(),
            base_url,
        ));
        EmptyFolderCleanup::from_config(client, config)
    }

    #[test]
    fn protected_ids_come_from_watch_and_sync() {
        let mut config = Config::default();
        config.watch.parent_folder_id = Some(7);
        config.sync.remote_parent_id = Some(9);
        assert_eq!(protected_ids(&config), HashSet::from([7, 9]));

        config.watch.parent_folder_id = None;
        assert_eq!(protected_ids(&config), HashSet::from([9]));
    }

    fn keep_config(rules: Vec<crate::rules::FolderRule>, patterns: &[&str]) -> Config {
        let mut config = Config::default();
        config.download.rules = rules;
        config.cleanup.keep_patterns = patterns.iter().map(|p| p.to_string()).collect();
        config
    }

    fn reason(config: &Config, path: &str) -> Option<String> {
        keep_folders(config).and_then(|k| k.reason(path))
    }

    #[test]
    fn no_keep_settings_means_no_callback() {
        assert!(keep_folders(&Config::default()).is_none());
        let rule = crate::rules::FolderRule {
            path: "Movies".into(),
            skip: Some(true),
            ..Default::default()
        };
        assert!(keep_folders(&keep_config(vec![rule], &[])).is_none());
    }

    #[test]
    fn rules_keep_folders_and_their_children() {
        let config = keep_config(
            vec![
                crate::rules::FolderRule {
                    path: "Movies".into(),
                    keep: Some(true),
                    ..Default::default()
                },
                crate::rules::FolderRule {
                    path: "Movies/Old".into(),
                    keep: Some(false),
                    ..Default::default()
                },
            ],
            &[],
        );
        let kept = Some("kept by the rule on Movies".to_string());
        assert_eq!(reason(&config, "Movies"), kept);
        assert_eq!(reason(&config, "Movies/Kids"), kept);
        assert_eq!(reason(&config, "Movies/Old"), None);
        assert_eq!(reason(&config, "TV"), None);
    }

    #[test]
    fn patterns_match_the_folder_or_a_folder_above() {
        let config = keep_config(vec![], &["Season*", "Software/*"]);
        let season = Some("matches keep pattern \"Season*\"".to_string());
        assert_eq!(reason(&config, "TV/Show/Season 1"), season);
        assert_eq!(reason(&config, "TV/Show/Season 1/Extras"), season);
        assert_eq!(reason(&config, "TV/Old"), None);
        assert!(reason(&config, "Software/Tools/Old").is_some());
        assert_eq!(reason(&config, "Software"), None);
        // Names are cleaned like rule paths
        let config = keep_config(vec![], &["Movies_*"]);
        assert!(reason(&config, "Movies: Old").is_some());
    }

    #[test]
    fn configure_replaces_settings() {
        let svc = service("http://127.0.0.1:9".to_string(), &Config::default());
        assert_eq!(svc.config(), CleanupConfig::default());

        let mut config = Config::default();
        config.cleanup.run_at = "04:30".to_string();
        config.watch.parent_folder_id = Some(5);
        svc.configure(&config);
        assert_eq!(svc.config().run_at, "04:30");
        assert!(svc.settings().protected.contains(&5));
    }

    #[tokio::test]
    async fn second_run_is_refused_while_one_is_going() {
        // No server behind this address: a run that got past the guard would fail
        let svc = service("http://127.0.0.1:9".to_string(), &Config::default());
        let _held = svc.running.try_lock().unwrap();

        let err = svc.run(CleanupTrigger::Manual, true).await.unwrap_err();
        assert!(matches!(err, CleanupError::AlreadyRunning));
    }

    #[tokio::test]
    async fn run_passes_protected_ids_to_the_walk() {
        let mut server = Server::new_async().await;
        let _t = server
            .mock("GET", "/transfers/list")
            .with_status(200)
            .with_body(r#"{"status":"OK","transfers":[]}"#)
            .create_async()
            .await;
        let folder = |id: i64| {
            serde_json::json!({
                "id": id, "name": format!("f{id}"), "size": 0, "file_type": "FOLDER",
                "parent_id": 0, "created_at": "2020-01-01T00:00:00", "is_folder": true
            })
        };
        let listing = |files: Vec<serde_json::Value>| {
            serde_json::json!({"status": "OK", "files": files, "parent": folder(0)}).to_string()
        };
        let _root = server
            .mock("GET", "/files/list?parent_id=0")
            .with_status(200)
            .with_body(listing(vec![folder(5), folder(6)]))
            .create_async()
            .await;
        for id in [5, 6] {
            server
                .mock("GET", format!("/files/list?parent_id={id}").as_str())
                .with_status(200)
                .with_body(listing(vec![]))
                .create_async()
                .await;
        }
        let deletes = server
            .mock("POST", "/files/delete")
            .expect(0)
            .create_async()
            .await;

        let mut config = Config::default();
        config.watch.parent_folder_id = Some(5);
        let svc = service(server.url(), &config);
        let report = svc.run(CleanupTrigger::Manual, true).await.unwrap();

        deletes.assert_async().await;
        let ids: Vec<i64> = report.removed.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![6]);
        assert_eq!(report.kept_protected, 1);
        // The guard is released after a run
        assert!(svc.running.try_lock().is_ok());
    }
}

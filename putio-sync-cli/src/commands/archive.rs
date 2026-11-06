//! `archive metrics|db|summary`: long-term copies of metrics, database and usage
//! (feature 041, contracts/archive-cli.md)
//!
//! Exit codes: 0 done, 1 bad arguments or config, 2 Prometheus, 3 database, 4 archive
//! folder. A failure is one line on stderr, `archive <cmd>: <what failed>: <cause>`.

use chrono::{DateTime, NaiveDate, Utc};
use putio_sync_archive::db::DbOptions;
use putio_sync_archive::metrics::MetricsOptions;
use putio_sync_archive::summary::SummaryOptions;
use putio_sync_archive::{ArchiveError, ArchiveResult};
use putio_sync_core::config::Config;
use putio_sync_core::log_redact;
use std::io::Write;
use std::path::PathBuf;

/// Options every archive command takes
#[derive(Debug, Clone, Default)]
pub struct ArchiveCommon {
    /// Archive folder; default `<config dir>/archive`
    pub dir: Option<PathBuf>,
    /// Fail when the folder has no `manifest.jsonl`
    pub require_manifest: bool,
}

/// One archive command with its options
#[derive(Debug, Clone)]
pub enum ArchiveJob {
    Metrics {
        common: ArchiveCommon,
        prometheus: Option<String>,
        instance: Option<String>,
        keep_days: Option<u32>,
        since: Option<NaiveDate>,
    },
    Db {
        common: ArchiveCommon,
        keep_days: Option<u32>,
    },
    Summary {
        common: ArchiveCommon,
        prometheus: Option<String>,
        instance: Option<String>,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    },
}

impl ArchiveJob {
    fn name(&self) -> &'static str {
        match self {
            Self::Metrics { .. } => "metrics",
            Self::Db { .. } => "db",
            Self::Summary { .. } => "summary",
        }
    }
}

/// Run the command and return its exit code
pub async fn execute_archive(job: ArchiveJob, config_path: Option<PathBuf>) -> i32 {
    let name = job.name();
    let mut out = std::io::stdout().lock();
    match run(job, config_path, Utc::now(), &mut out).await {
        Ok(()) => 0,
        Err(e) => {
            // Known secrets are masked and URLs shortened, whatever the cause says
            eprintln!("archive {name}: {}", log_redact::redact(&e.to_string()));
            e.exit_code()
        }
    }
}

/// Run the command at `now`, writing its report lines to `out`
pub async fn run(
    job: ArchiveJob,
    config_path: Option<PathBuf>,
    now: DateTime<Utc>,
    out: &mut dyn Write,
) -> ArchiveResult<()> {
    let config_path = match config_path {
        Some(path) => path,
        None => Config::default_path()
            .map_err(|e| ArchiveError::Args(format!("cannot find the config file: {e}")))?,
    };
    match job {
        ArchiveJob::Metrics {
            common,
            prometheus,
            instance,
            keep_days,
            since,
        } => {
            let prometheus = prometheus.ok_or_else(|| {
                ArchiveError::Args(
                    "--prometheus is required: pass --prometheus URL or set \
                     PUTIO_SYNC_ARCHIVE_PROMETHEUS_URL"
                        .to_string(),
                )
            })?;
            let opts = MetricsOptions {
                dir: archive_dir(&common, &config_path),
                prometheus,
                instance,
                keep_days,
                require_manifest: common.require_manifest,
                since,
                now,
            };
            putio_sync_archive::metrics::run(&opts, out).await
        }
        ArchiveJob::Db { common, keep_days } => {
            let opts = DbOptions {
                dir: archive_dir(&common, &config_path),
                db_path: database_path(&config_path)?,
                keep_days,
                require_manifest: common.require_manifest,
                now,
            };
            putio_sync_archive::db::run(&opts, out).await
        }
        ArchiveJob::Summary {
            common,
            prometheus,
            instance,
            from,
            to,
        } => {
            let opts = SummaryOptions {
                dir: archive_dir(&common, &config_path),
                db_path: database_path(&config_path)?,
                prometheus,
                instance,
                from,
                to,
                require_manifest: common.require_manifest,
                now,
            };
            putio_sync_archive::summary::run(&opts, out).await
        }
    }
}

/// `--dir`, else `archive` beside the config file (`/config/archive` in Docker)
fn archive_dir(common: &ArchiveCommon, config_path: &std::path::Path) -> PathBuf {
    common.dir.clone().unwrap_or_else(|| {
        config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("archive")
    })
}

/// The database the daemon uses: in the download folder of the config
fn database_path(config_path: &std::path::Path) -> ArchiveResult<PathBuf> {
    let config = Config::load(config_path).map_err(|e| {
        ArchiveError::Args(format!(
            "cannot load the config {}: {e}",
            config_path.display()
        ))
    })?;
    log_redact::set_secrets(config.secret_values());
    Ok(config.download.target_directory.join(".putiosync.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_folder_is_beside_the_config() {
        let common = ArchiveCommon::default();
        assert_eq!(
            archive_dir(&common, std::path::Path::new("/config/config.toml")),
            PathBuf::from("/config/archive")
        );
        let common = ArchiveCommon {
            dir: Some(PathBuf::from("/mnt/share")),
            require_manifest: false,
        };
        assert_eq!(
            archive_dir(&common, std::path::Path::new("/config/config.toml")),
            PathBuf::from("/mnt/share")
        );
    }
}
